# Allocator snapshot attribution (C01)

Correcting the SDK allocator changes the generated code and execution costs of fixtures that allocate. Snapshot changes were checked by changing only `sdk/alloc/src/lib.rs` to audit baseline `52abe`, using the same current compiler/test binaries and fixture dependency resolutions. The corrected allocator was restored immediately after each comparison.

For the SWAPP group, all ten tests passed the original snapshots with the baseline allocator; with the corrected allocator, six passed and four stopped at size/cycle assertions. After updating those four expectations, all ten also passed normal-mode verification in the full nextest run. Details and exact values are recorded in [the SWAPP cache investigation](compiler-audit-swapp-cache.md).

The full run identified six additional tests with snapshot mismatches: four network note tests, the batch-kernel test, and the combined basic-wallet/P2ID package-size test. A direct controlled rerun reproduced all six failures with the corrected allocator. Replacing only the allocator source made all six pass, including assertions after the first snapshot and all four batch-kernel behavioral scenarios.

Both phases ran offline using `nightly-2026-09-01`, with `NEXTEST_RUN_ID` and `UPDATE_EXPECT` absent. Every existing and generated repository `Cargo.lock` was hashed before and after each phase; none changed and none were added. This matters because fixture dependencies had resolved newer transitive versions during broad testing: the comparison holds those versions fixed rather than attributing changes from historical totals alone. The binaries came from the current test inventory, and were not rebuilt between phases.

The controlled runner and logs are under `/private/tmp/audit-c01-controlled.py` and `/private/tmp/audit-c01-controlled/`. This is a causal comparison of one source change, not a compile-time performance benchmark. The allocator's alignment, overflow, and disjoint-allocation checks remain required; restoring the unsafe baseline is not a solution to the increased instruction counts.

With the corrected allocator restored, snapshot-update mode completed all six tests and all later behavioral assertions. It refreshed these 15 expectations:

| Fixture measurement | Baseline allocator | Corrected allocator |
|---|---:|---:|
| P2ID note cycles (both basic-wallet transfers and constructor consumption) | 4825 | 5030 |
| Basic-wallet transaction-script cycles | 6297 | 6409 |
| P2IDE recipient-claim cycles | 5232 | 5467 |
| P2IDE sender-reclaim cycles | 5807 | 6042 |
| Note-constructor transaction-script cycles | 8687 | 8932 |
| Batch-kernel stripped MAST size | 104333 | 115220 |
| Batch-kernel normal batch cycles | 31966 | 32500 |
| Batch-kernel note-erasure batch cycles | 28098 | 28618 |
| Batch-kernel tampered pre-image rejection cycle | 954 | 1084 |
| Batch-kernel consume-before-create rejection cycle | 15328 | 15756 |
| Basic-wallet transaction-script stripped MAST size | 13234 | 13784 |
| P2ID note stripped MAST size | 17488 | 21763 |
| P2IDE note stripped MAST size | 13566 | 16402 |

The basic-wallet account package's size remained 8505. The P2ID cycle expectation occurs in three places, accounting for the difference between 13 distinct table entries and 15 refreshed assertions. Snapshot-update mode again left all fixture lockfiles unchanged. After rebuilding, all six tests passed with `UPDATE_EXPECT` unset, as part of a 372-test verification run.
