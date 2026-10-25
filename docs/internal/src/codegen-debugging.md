# Debugging code generation

The backend separates source selection, allocation, frame assignment, lowering expansion,
peephole optimization, and independent physical verification. Diagnose a failure by finding the
first representation that violates its contract. A physically valid program can still compute the
wrong result: instruction semantics and host effects also need execution tests.

## Capture the plan and assembly

Use a compiler built from the revision under investigation and retain the failing compilation's
target, optimization, debug, and library settings. Configure `MIDEN_SYSROOT` or `-L` as usual.

```sh
MIDENC_TRACE='codegen=trace,peephole=trace' \
MIDENC_TRACE_STYLE=never \
midenc repro.hir --stop-after=lower --emit=masm=- \
  > repro.masm 2> repro.trace
```

`--stop-after=lower` stops before packaging. `--emit=masm` alone still runs downstream stages.
The trace includes completed physical plans, frame layouts, operand-solver tactic attempts,
peephole rewrite names and positions, and per-procedure allocation and verification timings.

The [plan printer](../../../codegen/masm/src/plan/actions.rs) reports:

| Field | Meaning |
| --- | --- |
| `segment bN`, `entry` | Segment identity and required top-first stack; the tail describes the suffix contract. |
| `require` | Spill-slot contents required on entry. |
| `contract`, `costs` | Selected contract origin and viable candidate scores. These are heuristic costs, not whole-program cycle counts. |
| `unit sN`, `kind`, `source`, `origin` | Selected unit, registered lowering kind, source identity, source span and inline ancestry. |
| `stack`, `stored`, `outputs` | The unit's required inputs and published outputs, **not** a snapshot of the current stack. |
| `tier`, `victims` | Chosen preparation tier and evictions, including reasons and next-use distances. |
| `semantic-storage-reads` | Backing cells needed by the unit's semantic inputs; metadata for storage liveness, not emitted loads or verifier authority. |
| `operand-traffic`, `edge-traffic` | Lane loads/stores for operand preparation versus control-flow repair. |
| `edge-preparation` | Control-predicate preparation: edge identity, tier, victims and load/store counts. |
| `dup`, `store`, `swap`, `run`, etc. | Ordered allocator actions; `run sN` expands the selected lowering unit. |

`entry` and `require` use `v3.0` for lane zero of semantic value 3. Unit input/output
lists use Debug notation such as `Atom { value: ValueId(3), lane: 0 }`. Stack depths are
zero-based; only depths 0–15 are addressable, although a deeper stack is legal. The [frame trace](../../../codegen/masm/src/plan/diagnostics.rs)
separates source locals, scalar spill storage, spanning objects, reusable workspace and padding,
then lists physical allocations.

The trace is observational, not a replayable plan file. It does not currently include the complete
state before/after every action, every rejected scheduling candidate, all liveness facts, or the
complete lowering bindings. In particular, inspect `ProcedurePlan::lowering_bindings` to see the
recorded commutative input order. Production prints the plan only after allocation and frame
resolution succeed. Failures before that point require inspecting the intermediate structures.
Plan records also lack symbol-filter metadata; do not rely on `MIDENC_TRACE_FILTER=symbol:...`
to isolate a procedure's plan.

## Isolate HIR transformations

Use pass dumps to determine whether the backend received correct HIR:

```sh
MIDENC_TRACE='pass:.*=trace' \
midenc repro.wasm \
  -Zprint-ir-after-pass=canonicalizer,lift-control-flow \
  -Zprint-ir-filter=symbol:my_function \
  --stop-after=transform --emit=hir=-
```

Other useful options include `-Zprint-ir-after-modified`,
`-Zprint-ir-before-stage=codegen`, `-Zprint-hir-source-locations`, and
`-Zprint-cfg-after-pass=<pass>`. Run `midenc -Z help` for supported options. The logger's
`MIDENC_TRACE` controls which trace records are emitted; the IR printer options control when and
what the pass instrumentation prints. Pass filters match `Pass::name`, such as
`lift-control-flow` and `sparse-conditional-constant-propagation`; pass-pipeline registration
arguments use `cfg-to-scf` and `sccp`, respectively.

Reduce a transformation failure to HIR and run only the relevant passes:

```sh
hir-opt repro.hir --verify \
  --pass-pipeline='builtin.world(builtin.module(builtin.function(canonicalizer,cfg-to-scf,canonicalizer)))'
```

Keep the original pipeline in a regression test if pass interactions are necessary to reproduce
the failure. A reduced test is useful evidence, but does not replace the production reproducer.

## Inspect backend stages in focused tests

These are crate-internal test facilities, not public APIs or CLI switches. Paths below are
relative to `codegen/masm/src`.

| Stage | Inspection point |
| --- | --- |
| Selection | `select/tests/mod.rs::checked` separates selection and source correspondence checking from allocation. Inspect the selected evidence and the source-owned lowering contract; reconstruction shares the trusted selector implementation. |
| Analysis | `allocate/analysis.rs::analyze` produces liveness before/after units, edge demand, next-use distances, loop structure and scratch dependencies. These structures implement `Debug`. |
| Scheduling and contracts | Inspect `ProcedurePlan`, its action streams, entry contracts and lowering bindings. `opt/operands/solver.rs` traces local tactic attempts; those attempts are not a complete allocator decision history. |
| Storage | Inspect the symbolic slots and `allocate/slots.rs::assign_frame` result. Distinguish incorrect content liveness from incorrect physical reuse or addressing. |
| Expansion | Inspect `AnnotatedMasmBody` before optimization. Instructions retain unit ownership and segment, edge, control and unit boundaries. |
| Peepholes | Compare the body before/after `opt/peephole.rs::simplify_annotated`, then verify the resulting body. Trace records identify individual rewrites. |
| Verification | Inspect the replay cursor and symbolic stack/storage at the failed obligation in `verify/replay.rs`. |

[`select::tests::ready`](../../../codegen/masm/src/select/tests/mod.rs) returns the plan, final frame
and annotated body after peepholes and immediately before physical verification. Split the helper
at its `simplify_annotated` call when comparing pre/post-optimization bodies. There is no CLI toggle
for this comparison; anchor normalization is also part of that function, so simply omitting it
can cause an unrelated missing-anchor failure.

[`plan::tests::lowering_units::ready`](../../../codegen/masm/src/plan/tests/lowering_units.rs)
accepts explicit allocation options. Compare default allocation with:

- `force_tier: Some(SchedulingTier::Normalization)` to bypass optimized unit preparation
  attempts. Control-predicate preparation still uses its normal tier cascade.
- `refuse_solver: true` to make the grouped operand solver decline; other optimized preparation
  remains available.
- Different `shuffle_fuel` values to investigate search sensitivity.

The first two options exist only under `cfg(test)`. Forcing another tier still permits
normalization fallback. A default-versus-normalization difference narrows the investigation; it
does not by itself prove the optimized scheduler is wrong.

Run only the relevant tests, for example:

```sh
cargo make test-rust -p midenc-codegen-masm -E 'test(exact_reproducer_name)'
cargo make test-release-rust -p midenc-codegen-masm -E 'test(exact_reproducer_name)'
```

These repository tasks forward the filter to nextest; the second uses the `test-release`
Cargo profile. Check that the filter selects the intended test. Use the relevant normal and
release checks when assertions, arithmetic or optimizer behavior could differ between profiles.

## Interpret verification failures

The physical verifier independently checks stack access, value identity, storage, transfers,
ABI requirements and publication. Do not suppress a verification error to get a runtime test
through: determine whether the plan, expansion, or verifier's model is wrong.

[`PlanError`](../../../codegen/masm/src/plan/diagnostics.rs) has optional source, step, depth and
expected/actual atom fields. Selection and replay attach the source span, inline ancestry and
selected step where available. Replay also attributes pre-unit preparation failures to the
pending unit; edge transfers and trailing cleanup do not acquire an unrelated unit's context.
Identity mismatches report expected lanes and actual identities established by replay. Some
failures still lack a source location or step, and errors do not include a complete stack diff.
The replay cursor retains the segment, edge, action position and active unit internally. A Rust
debugger breakpoint at the failure site, or temporary focused instrumentation, can expose it.
Expected/actual atom fields describe semantic lanes, not the verifier's internal symbolic tokens.

Selection checking, frame allocation and raw replay can report related categories, so identify
the producing stage before changing code. The timing records only cover allocation and physical
verification; they are not a complete stage profiler.

## Check runtime semantics and effects

Physical verification is deliberately not a proof of every instruction's numerical algorithm.
The `physically_valid_direction_mutation_needs_execution` test in `plan/tests/lowering_units.rs`
changes Memmove's direction predicate, passes physical verification, then detects incorrect memory
through VM execution. Use independent expected results, distinct lane values, trap assertions,
memory observations and caller-suffix sentinels for this class of bug.

Useful helpers in `select/tests/mod.rs` include:

- `execute_linked_memory_mutate`: mutate an annotated body before verification, link helpers,
  initialize/observe memory, and check a twenty-value caller suffix.
- `execute_failure_state`: retain processor state and the execution error at a trap.

For a production reproducer,
[`compile_test_module_with_masm` and `compile_miden_component_to_package_with_masm`](../../../tests/support/src/testing/eval.rs)
capture the actual assembly sent to the assembler and return the compiled package. Inspect and
execute that same artifact instead of reconstructing a similar program by hand.

Use a compatible `miden-debug` to inspect a package interactively:

```sh
miden-debug --repl repro.masp
```

See the [debugger guide](../../external/src/guides/debugger.md) for stack/advice inputs and command
scripts. Allocation proof boundaries disappear during serialization; they are not VM breakpoints.
Source/debug metadata helps correlate execution with source, but is not a per-action plan trace.

## Investigate code-quality regressions

Compare plan traffic, expanded MASM, and executed VM cost separately. Extra instructions may
come from preparation, edge repair, a lowering body, call windows or generated root procedures.
One MASM instruction need not equal one VM operation, and runtime branch/loop frequency matters.

[`opt/cost.rs`](../../../codegen/masm/src/opt/cost.rs) contains the local stack-instruction cost
model and an assembler-backed test oracle. That oracle excludes basic-block packing noops; it is
not a whole-program cost estimator. Confirm improvements with actual assembly/execution, retain
semantic assertions, and understand each changed expectation before updating cost snapshots.

Capture expectation updates with `--test-threads 1` when tests can edit the same source file.
The expectation updater retains whole-file state per process, so concurrent writers can lose
one another's changes. Then rerun the selected tests with expectation updating disabled.

The [performance report](../../design/unified-stack-allocator-performance.md) records historical
cycle, serialized-size, frame and compile-time observations, their limitations, and reproduction
commands. Keep current measurements distinct from those historical snapshots.
