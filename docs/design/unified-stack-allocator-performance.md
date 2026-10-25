# Unified stack allocator performance

The correctness cutover increased execution and serialized code costs on five small production-pipeline workloads. The cutover accepted those costs, with backend scheduling and generated-root optimization considered separately. Subsequent optimizations improve several real protocol execution paths while leaving some cycle and encoded-size increases. This report records historical measurements; it is not a measurement of the current checkout or a promise of globally optimal scheduling.

## Measurement boundaries

The initial comparison uses archived pre-cutover production output and the cutover capture. Adjacent historical investigation records associate the baseline with `f3d47420`; the measurement report itself identifies capture logs and hashes, so that revision attribution is corroborated rather than directly established. The cutover capture was a working-source snapshot preserved by source/artifact hash manifests, not a separately identified final commit. It predates the completed unified-backend snapshot `c46ec2e3`. Names, inputs and expected results were retained. Original baseline WAT was not captured, so original-WAT hash equality is not established. The data/global fixture has no Wasm start function; a distinct raw-start experiment is excluded.

The optimization comparison uses accepted unified-backend output (`c46ec2e3`) against final optimization/storage-repair evidence (`f89b6b0e`, followed by metric refresh `5160d439`). All revision IDs here identify historical snapshots; rebasing the review stack may give current changes different IDs. These snapshots precede the later debugger context-call marker repair and its October 24 metric refresh. Do not combine their numbers with post-marker expectations as though they were the same compiler. No fresh measurement was made for this document.

## Initial cutover

| Workload | VM cycles, baseline → cutover | Serialized MAST bytes, baseline → cutover | Nonzero procedure frame cells, baseline → cutover |
| --- | ---: | ---: | --- |
| Pressure | 381 → 503 (+32.0%) | 2166 → 2500 (+15.4%) | 16, 16 → 16, 16 |
| Join, either input | 289 → 322 (+11.4%) | 2218 → 2494 (+12.4%) | 3 → 4 |
| Loop | 599 → 637 (+6.3%) | 1997 → 2321 (+16.2%) | 3 → 4 |
| Call | 302 → 332 (+9.9%) | 2132 → 2405 (+12.8%) | 2, 2 → 4, 4 |
| Data/global initialization | 361 → 396 (+9.7%) | 2225 → 2424 (+8.9%) | 1 → 4 |

All five cutover traces select only the Stack scheduling tier. Operand and edge load/store traffic are each zero; scalar spill, spanning spill and workspace frame contributions are zero. The frame increases are four-cell alignment padding, not spill growth. These cases do not establish performance under actual spilling.

Saved assembly identifies added work: the generated harness becomes a checked procedure invoked by the root; root cleanup explicitly prepares floor zeros; and fixed operand ordering adds swaps and rotations. Pressure emits fifteen zero/swap pairs before cleanup and fifteen swaps before commutative additions in its callee. Loop adds condition-order swaps. These observations explain particular additions, but the complete initial cycle delta was not apportioned instruction by instruction. Deleting cleanup requires proof of the physical floor contract, not merely an instruction-count argument.

Separate CLI traces provide the following sums over allocation units, including generated stages. Repeated local procedure names count as separate records.

| Workload | Allocation, ns | Verification, ns | Stack-tier records |
| --- | ---: | ---: | ---: |
| Pressure | 6,463,333 | 1,668,041 | 23 |
| Join | 2,480,833 | 1,415,375 | 17 |
| Loop | 3,478,125 | 3,093,666 | 34 |
| Call | 1,767,209 | 878,834 | 12 |
| Data/global initialization | 2,554,458 | 1,168,875 | 20 |

The legacy backend provided no corresponding stage or physical-traffic counters. Integration aggregate compilation observations, in microseconds, were pressure 125024 → 352666; join 124544/67738 → 311068/410118; loop 120547 → 342863; call 118999 → 303198; data/global 120226 → 312167. They include frontend, transforms, lowering and assembly, and were collected under different cache/concurrency conditions. They do not establish a comparable compiler slowdown or allocator-stage ratio.

## Optimization results and explanations

| Metric | Unified backend → final optimization, before later context-call marker repair |
| --- | ---: |
| RPO Falcon authentication cycles | 75,321 → 75,326 (+5) |
| SWAPP full-fill cycles | 13,814 → 13,571 |
| SWAPP partial-fill cycles | 19,373 → 18,966 |
| SWAPP reclaim cycles | 5,832 → 5,677 |
| SWAPP serialized MAST bytes | 46,906 → 47,426 (+520) |
| Batch largest successful scenario cycles | 348,074 → 342,905 |
| Batch serialized MAST bytes | 122,270 → 123,124 (+854) |

Authentication's five-cycle increase is accounted by executed assembly: initialization saves seven operations; the main path adds 32; five output bridges save three each; the hash bridge saves five. Main includes six new local stores and six new local loads, costing 66 expanded operations, offset by other savings. Its frame remains 20 cells. This is executed-path accounting, not proof that every local scheduling choice is optimal.

Investigation identified and repaired a real cost-model defect: local loads/stores were scored as three/four operations, while pinned assembler 0.35.0 expands them to five/six. Historical validation covered the correction with assembler conformance and 53 focused tests; the current [cost model](../../codegen/masm/src/opt/cost.rs) retains an assembler-backed oracle. A fresh explicitly rebuilt final CLI produced authentication assembly identical to the pre-correction optimization assembly. The repair therefore did not recover these five cycles; stale-binary comparisons were withdrawn. The historical numeric refresh subsequently updated expectations with later semantic assertions retained and final update-disabled checks passing.

Encoded MAST size is a distinct objective from primitive count or execution cycles. SWAPP has 179 fewer padded primitive operations but five more unique nodes, five more batches and 58 more immediate-bearing operations. Its complete +520-byte reconciliation is +200 node bytes, +285 operation bytes, +25 batch metadata and +10 block headers/counts. Post-storage-repair cache bytes match the analyzed cache exactly.

Batch independently has 155 fewer padded primitive operations but five more nodes, six more batches and 98 more immediate-bearing operations. Its final +854-byte reconciliation is +200 node bytes, +629 operation bytes, +30 batch metadata and −5 block headers. The earlier CI observation of 123193 bytes is superseded by final 123124 bytes. Executable reconstruction matches both measured sizes and the final forest's multiset of all 1547 node digests.

All 34 batch scenarios were compared through temporary execution histograms, retaining original inputs and semantic checks. All 68 baseline/final executions reproduce their cycle pins with zero counted-work residual. Fourteen rejection paths increase by 2–96 cycles: descending input/output +95/+78; duplicate input/output +78/+61; long/short header +2/+2; missing input/output +20/+36; oversized input/output +94/+89; phantom output +5; tampered header +37; unaligned input/output +96/+91. Early prefixes add routing, duplicate/address support and control work without extra MLoad/MStore. Later missing/phantom paths save routing but add memory/address, padding and control work. Assertion, hash, advice, pipe and integer/comparison opcode counts, apart from Add, do not increase on those paths. Deep-duplication and local-address expansions can introduce Add operations, but the historical assessment did not isolate the owner of every Add. Error outcomes and original success assertions are preserved. Histogram accounting does not prove opcode-sequence identity, identify a unique failing assertion when error codes are shared, or isolate the exact candidate decision responsible for every change.

These are explained local scheduling/encoding tradeoffs, with no unaccounted byte or counted-work residual in the analyzed cases. They remain opportunities for improvement rather than proof of global optimality. These historical results include the storage repair and final numeric refresh, with no temporary diagnostic edits retained.

## Later debugger frame markers

A separate correctness repair restored FrameStart/FrameEnd events around Call/SysCall/DynCall. Each EmitImm expands to Push(event_id), Emit, Drop in assembler 0.35.0. A pair adds six executable operations before batching/padding effects, even without debug metadata. Both branches independently reproduced all fifteen affected CI failures; temporarily omitting only these marker pairs restored all original expectations and semantic checks. The temporary edits were restored.

Representative October 24 changes are unoptimized unified-backend SWAPP full/partial/reclaim cycles 13814/19373/5832 → 13836/19403/5838 and bytes 46906 → 46965; optimization SWAPP cycles 13571/18966/5677 → 13590/18989/5684 and bytes 47426 → 47548. Marker costs therefore cannot be estimated by multiplying call counts by six without accounting for packing effects. Unified-backend refresh `5142a2a94d56` and optimization refresh `bf798c07104c` each pass all fifteen exact cases in normal and test-release profiles with updates disabled. These are local results; the historical closeout records new remote CI as pending. They do not replace the earlier optimization diagnosis.

## Repeating measurements

Use the same guest sources, inputs, dependency lockfile, sysroot, toolchain and compilation mode at both revisions. Record exact revision and dirty-source hashes, host details, profile, concurrency, cache state and binary hashes. Rebuild the CLI explicitly before inspecting emitted assembly; `cargo make test` does not necessarily rebuild `bin/midenc`. Keep expectation updating disabled and retain semantic assertions. Run repeated, sequential samples with an explicit warmup/cache policy before comparing wall times; do not compare a cold concurrent observation to a warm serial one.

The [production fixture helper](../../tests/integration/src/codegen/stack_allocator/mod.rs) supports these existing flags. Precreate an output directory, configure the repository's normal `MIDEN_SYSROOT`, then run:

```sh
mkdir -p /tmp/allocator-performance
RUST_MIN_STACK=16777216 MIDENC_BIN_DIR="$PWD/bin" \
MIDENC_STACK_ALLOCATOR_MASM_DIR=/tmp/allocator-performance \
MIDENC_STACK_ALLOCATOR_METRICS=1 MIDENC_TEST_TIMINGS=1 \
cargo make test-rust -p midenc-integration-tests \
  -E 'test(~codegen::stack_allocator::baseline_)' --success-output immediate
```

The helper saves WAT/MASM and reports cycles, stripped MAST bytes, aggregate compile microseconds, frame declarations and static textual local-load/store occurrences. The last two counts are not dynamic traffic. `MIDENC_TEST_TIMINGS=1` enables pipeline checkpoint observations; neither output should be silently relabeled allocator time. The filter selects the five baseline test functions, with join executing both inputs.

For each saved WAT, the historical CLI trace pattern was:

```sh
MIDENC_TRACE='codegen=trace' bin/cargo-miden miden build \
  /tmp/allocator-performance/allocator_pressure.wat \
  --test-harness --entrypoint allocator_pressure::entrypoint --emit=masm=-
```

Substitute the matching workload/module name for each fixture. Preserve stderr traces separately from assembly, and sum actual allocation/verification records without merging different units by local name. Use the [SWAPP](../../tests/integration-network/src/mockchain/swapp.rs),
[batch kernel](../../tests/integration/src/end_to_end/batch_kernel.rs), and
[authentication](../../tests/integration-network/src/mockchain/counter/rpo_auth.rs) fixture owners
for protocol comparisons: their execution modes and semantic checks are part of the measurement. No new benchmark command or universal performance threshold is implied here.

## Historical provenance and limits

The tables summarize archived capture logs, source/artifact hash manifests, emitted
MASM, package-cache decoding and temporary execution histograms. The initial
cutover was a working-source capture; the named later revisions identify committed
historical snapshots. Rebased stack heads need not retain those commit IDs. The
recorded local stage timings have no matched legacy baseline or repeated-sample
statistics, and the five small initial workloads exercised no spilling.

The SWAPP size comparison used cached packages with SHA256
`dfcdb0e4d25310689334f631645fe20d96a5f36feaa8b3d70c386eeca63edc5a`
(baseline) and
`0fadd430db447f259da57a2f3934d0511ba6f34b6b52982fca7be16ebf35e827`
(optimized). Final post-storage-repair bytes matched the analyzed optimized cache.
Batch's final executable reconstruction matched the measured size and all 1547
node digests, rather than substituting a library assembly of a multi-module dump.
Those artifact checks support the historical reconciliation; they do not establish
current-head equality or make the temporary diagnostic tools a supported interface.

The relevant decisions and limitations are stated in this report so that local,
ignored investigation ledgers are not required reading. To reproduce the numbers,
use the matching historical sources and measurement boundaries above, capture new
artifacts, and compare them explicitly. To assess the current compiler, make a new
measurement with current semantic checks and report it separately.
