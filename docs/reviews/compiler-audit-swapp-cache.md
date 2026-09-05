# SWAPP fixture compilation cache (T03)

The ten SWAPP tests all call `compile_swapp_packages`. Previously that helper used two `OnceLock`s. They shared compilation in a single `cargo test` process, but nextest starts a separate process for each test, so the full SWAPP selection could compile the wallet and note ten times each.

The helper now keeps one in-process `OnceLock` for the pair and, when `NEXTEST_RUN_ID` is present, shares a serialized pair across processes. The cache path uses the full nextest run ID under the workspace `target/nextest/<run-id>/swapp` directory; run IDs containing path components are rejected. This deliberately permits reuse only within a single invocation; changes between runs cannot reuse an earlier compiled fixture. Cache directories are retained across runs for inspection and removed by cleaning that directory (for example, `cargo clean --target-dir target` from the workspace root). The cache uses the workspace `target` directory even if compilation selects a custom `CARGO_TARGET_DIR`; cleaning only that custom directory does not remove these caches. They are never read by a different run.

An exclusive `std::fs::File` lock covers lookup, compilation, and publication. The wallet compiles first so its package artifacts exist when the dependent note compiles. Both complete packages are serialized together; a pending file is renamed into place only when complete. Readers use trusted package deserialization to preserve compiler debug sections. A panic releases the file lock through RAII, and a producer killed before publication leaves no visible partial payload. Direct `cargo test` without a nextest run ID uses only the in-process cache.

The standalone regression compiles the actual standard-library-only cache implementation with `rustc --edition 2024 --test tests/integration-network/src/mockchain/support/run_cache.rs`. Its ten simultaneous child processes share one cache directory, each supplies a producer recording wallet and note compilation, and the resulting cross-process counter contains exactly two records. Removing the lock from a temporary copy makes this regression fail. Additional tests cover full run identity preservation and rejection of path traversal and a panicking producer followed by a successful retry. All four tests passed.

These counters measure the cache's producer boundary with lightweight stand-ins, not elapsed compiler time or real package compilation. They establish one pair producer across ten processes; the actual SWAPP integration tests must also validate package serialization and execution. All ten existing SWAPP test functions remain selected independently, so sharing fixture compilation does not reduce behavioral coverage. Run them with `cargo nextest run -p midenc-integration-network-tests -E 'test(mockchain::swapp::)'` and the cache regressions with `cargo test -p midenc-integration-network-tests run_cache`.

## Actual fixture validation and snapshot attribution

The focused nextest run exercised the actual fixture cache and logged exactly two root package compilations: the basic wallet and SWAPP note. Four SWAPP tests stopped at pre-existing size/cycle snapshots. Running the same network test binary directly, with `NEXTEST_RUN_ID` absent, reproduced the same four mismatches (six tests passed). This rules out the shared cache and package roundtrip as their cause.

A controlled comparison then changed only `sdk/alloc/src/lib.rs` back to audit baseline `52abe`, leaving the current Miden compiler and test binary in place. All ten direct SWAPP tests passed their original expectations. The corrected allocator was immediately restored and its working-tree contents verified. Thus the observed changes are attributable to the C01 allocator correction:

| Snapshot | Before C01 | With C01 |
|---|---:|---:|
| Stripped note package size | 38844 | 42586 |
| Full fill cycles | 12575 | 12839 |
| Partial fill cycles | 17645 | 17931 |
| Creator reclaim cycles | 5336 | 5573 |

The comparison logs were `/private/tmp/audit-t03-direct.log` and `/private/tmp/audit-t03-old-allocator.log`; direct runs took 5.99 and 6.11 seconds respectively, with warm build artifacts. These are attribution checks, not performance benchmarks. Some cycle assertions precede later behavioral assertions, so the tests must be rerun after refreshing the attributed snapshots before claiming all current-allocator behavior passes.

With the corrected allocator restored, a direct run in snapshot-update mode reached all remaining assertions: ten tests passed in 6.02 seconds, and only the four attributed expectations above changed. This is not yet a normal-mode snapshot verification; the rebuilt binary must be rerun without `UPDATE_EXPECT` to verify the newly embedded expectations.

The real ten-test nextest selection was then executed. Timing records showed exactly one root compilation each for `basic-wallet` and `swapp-note` across all ten processes. Four original size/cycle snapshots changed, and a controlled uncached run reproduced the same changes. Restoring only the original allocator made all ten original expectations pass, identifying C01's corrected allocation arithmetic as the cause. After updating those four expectations, all ten SWAPP tests passed in normal nextest mode, including the assertions after the cycle checks. Package round-trip and behavioral validation therefore cover the real serialized fixtures as well as the lightweight cache producer test.
