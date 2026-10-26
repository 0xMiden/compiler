# Unified stack allocator performance

This report separates current CI comparisons, local scheduling measurements, and historical cutover evidence. VM cycles, serialized MAST bytes and compiler wall time answer different questions; none substitutes for the others.

## Current CI comparison

[PR 1484 benchmark run 40246156884](https://github.com/0xMiden/compiler/actions/runs/40246156884/job/121328054304), completed October 26, 2026 UTC, reports candidate `37aa93e240af` against `next` `51c31fdbf447`. The run belongs to PR head `6361b76b6`; the candidate is the workflow checkout revision, not a claim that those two hashes identify the same commit. These measurements precede the dead-prefix cleanup below. Both sides use the workflow's own builds and sysroots; the run retains compiled packages, replay snapshots and VM cycle flamegraphs.

| Workload | VM cycles (change from next) | MAST bytes (change from next) |
| --- | ---: | ---: |
| No-auth component | 8,482 (+2.08%) | 7,107 (+5.30%) |
| RPO Falcon authentication | 81,577 (+0.52%) | 15,299 (+14.55%) |
| Basic wallet | 82,879 (+0.23%) | 9,360 (+5.83%) |
| Basic-wallet transaction | 84,588 (+0.36%) | 16,300 (+6.44%) |
| Collatz | 5,277 (+0.29%) | 2,497 (+5.63%) |
| Counter contract | 86,794 (+0.60%) | 15,208 (+13.03%) |
| Counter note | 86,794 (+0.60%) | 4,091 (+11.32%) |
| DEX note | 83,630 (+0.26%) | 17,550 (+10.20%) |
| Fibonacci | 883 (+1.73%) | 3,315 (+4.18%) |
| Is-prime | 2,309,570 (~0%) | 6,072 (+4.28%) |
| P2ID note | 82,879 (+0.23%) | 21,347 (+4.69%) |
| P2ID transaction | 87,257 (+0.64%) | 16,425 (+8.34%) |
| P2IDE note | 83,341 (+0.29%) | 17,482 (+7.07%) |
| Storage example | Not measured | 16,547 (+6.16%) |

The cumulative stack still costs more than `next` on these workloads, including correctness repairs and debugger markers as well as allocator choices. These totals do not assign the increase to any one mechanism. The workflow's flamegraphs describe guest execution, not compiler CPU time.

## Bounded dead-prefix cleanup

The scheduler compares the existing preparation with a candidate that first drops consecutive unneeded top occurrences. The candidate must have no greater complete estimated preparation cost, including its drops; ties prefer cleanup. It does not assume that a later return will repay those drops, and leaves zero-cost preparations and alias publication alone. The focused VM regression reduces `swap; drop; swap; drop; drop` to three drops while preserving the complete caller suffix. This is a local cost guarantee, not a guarantee about total execution cycles or MAST encoding.

A paired local measurement compares optimization snapshot `82a82764` with that same source plus the bounded cleanup. Both binaries use the locked dependencies, the dev compiler profile, Rust `1.100.0-nightly (0dfb098f3 2026-08-31)`, Darwin arm64, and sysroot publication `0.17.0-aac05c66ae5bf43f`. Each workload has one discarded warmup and five sequential measured runs per binary; baseline/candidate order alternates each round. Another compiler process was active on the host, so these elapsed times are descriptive samples, not isolated CPU measurements or a speedup claim.

| Workload / compiler | Full CLI median, ms (range) | Allocation median, ms | Verification median, ms | Median combined stage share |
| --- | ---: | ---: | ---: | ---: |
| Counter / original | 760.21 (740.78–762.66) | 74.74 | 51.97 | 16.62% |
| Counter / cleanup | 748.23 (739.39–761.18) | 68.97 | 49.51 | 15.95% |
| Spilling initializers / original | 490.49 (481.64–491.63) | 154.79 | 40.94 | 39.95% |
| Spilling initializers / cleanup | 492.75 (480.05–494.81) | 152.76 | 41.43 | 39.66% |

The denominator is the entire CLI build, including parsing, selection, frame assignment, expansion, assembly and I/O. Stage times sum the existing allocation and verification elapsed-time records across thirteen procedures/stages for the counter and four for the initializer workload. Allocation encloses `allocate`; verification follows expansion and peepholes. Tracing uses `MIDENC_TRACE='codegen:^$=trace'` and `MIDENC_TRACE_STYLE=never`, excluding solver-detail logging. The percentages are medians of each sample's combined-stage ratio, not ratios of independently chosen medians.

The counter uses the existing compiled counter-contract guest, linked with the same sysroot's `miden-protocol.masp`. The pressure fixture has two global initializers, each keeping twenty advice values live, checking each value and summing them, followed by a sixteen-input entrypoint. It exercises real scalar spilling: the generated initialization procedure uses a twenty-cell scalar spill frame, reused across the two initializers. Each initializer records forty operand loads and twenty operand stores, for eighty loads and forty stores overall. Neither variant changes those counts or the pressure assembly. Counter assembly removes dead-lane swaps and shortens some later movements. These are static plan/assembly observations. Dynamic cycle comparisons require executing the integration tests through their semantic assertions; static counts alone do not establish those results.

For capture identity, the input SHA256 hashes are `b7e8391a83da771c5954b39fdcd99a850e33dff1196469400fd434c6a6d5e87a` (counter) and `cb29c327a9b6d8a6016d17480898de2bb51d852f482ed496b0150359df24d3fc` (pressure). Binary hashes are `749cbd2d699a0f83ce27a6fcc7a11d73f4b6792c153bc642922256624d871a97` (original) and `484be8443ab55272d995fa91f5de4c91d8d538a6920019c32806390a32eb4652` (cleanup). Counter MASM hashes differ (`8e31533efa5bbb07093aeebbb700056943b9d93f21aadb33dedf74b8174b8afb` → `1cb4a1b78708e8629df4408e0205fc31e58f52cfe1f2359279e5407ccbff7f25`); pressure MASM is identical (`32def9d3895cc791129a399ff280e65f37eaf4eab3088fb6d3eec5497f13defc`). Hashes identify these captures; they do not make a rebased revision equivalent to either binary.

The targeted integration capture also measures the following dynamic/encoded changes on unchanged inputs. These fixtures use their own execution modes and are distinct from the CI benchmark rows above. All later semantic assertions are retained.

| Integration metric | Original → cleanup |
| --- | ---: |
| No-auth counter note cycles | 9,375 → 9,266 |
| RPO authentication cycles | 75,318 → 75,143 |
| SWAPP full-fill cycles | 13,579 → 13,423 |
| SWAPP partial-fill cycles | 18,978 → 18,731 |
| SWAPP reclaim cycles | 5,673 → 5,618 |
| SWAPP MAST bytes | 47,540 → 45,658 |
| Batch largest successful scenario cycles | 342,905 → 326,274 |
| Batch MAST bytes | 123,124 → 119,181 |

Across the twelve affected expectation files, all fifty-eight changed numeric literals decrease, including later assertions previously hidden by a failing cost pin. That is evidence for these workloads, not a universal consequence of choosing a cheaper preparation.

### Remaining burial cost

Dropping dead top lanes cannot help when every owned lane remains live. A separate raw allocator-state probe starts with sixteen distinct live inputs and copies one for an operation. The next operation needs the original deepest value:

| First operation | Prepared → completed depth | Next preparation | Estimated next cost | Loads / stores |
| --- | --- | --- | ---: | ---: |
| No result | 17 → 16 | Stack | 6 | 0 / 0 |
| One result, no backing for deepest value | 17 → 17 | Normalization | 107 | 1 / 17 |
| One result, deepest value already backed | 17 → 17 | Loads | 5 | 1 / 0 |

All first preparations cost one estimated operation. These probes establish a scheduling opportunity, not a correctness failure or an end-to-end VM result. The already-backed case excludes the cost of obtaining that backing. Temporary burial disappears after consumption; a published result can make it persist. Changing survivor accounting to ignore values below depth sixteen would be incorrect. A future policy should compare bounded recovery when the buried value is actually requested with advance backing based on completed-state demand; advance writes can be wasted on paths that never request the value. Neither policy is part of this cleanup.

## Historical measurements

The following sections retain the original cutover and optimization evidence. Their snapshot hashes and metrics predate the current CI table; rebases do not turn them into measurements of the current compiler.

### Measurement boundaries

The initial comparison uses archived pre-cutover production output and the cutover capture. Adjacent historical investigation records associate the baseline with `f3d47420`; the measurement report itself identifies capture logs and hashes, so that revision attribution is corroborated rather than directly established. The cutover capture was a working-source snapshot preserved by source/artifact hash manifests, not a separately identified final commit. It predates the completed unified-backend snapshot `c46ec2e3`. Names, inputs and expected results were retained. Original baseline WAT was not captured, so original-WAT hash equality is not established. The data/global fixture has no Wasm start function; a distinct raw-start experiment is excluded.

The optimization comparison uses accepted unified-backend output (`c46ec2e3`) against final optimization/storage-repair evidence (`f89b6b0e`, followed by metric refresh `5160d439`). All revision IDs here identify historical snapshots; rebasing the review stack may give current changes different IDs. These snapshots precede the later debugger context-call marker repair and its October 24 metric refresh. Do not combine their numbers with post-marker expectations as though they were the same compiler. No fresh measurement was made for these historical tables.

### Initial cutover

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

### Optimization results and explanations

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

### Later debugger frame markers

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
