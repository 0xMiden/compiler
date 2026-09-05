# Compiler audit timing protocol

Record timings before changing build profiles or caching policy. A faster isolated slice is evidence
for that slice; it does not establish a whole-suite speedup. Never compare runs performed alongside
other builds. Record the exact revision, dirty state, command, target directory, toolchain, and test
concurrency. The command wrapper records these fields and wall time in append-only JSONL:

```sh
python3 tools/time-command.py --label warm-unchanged-1 \
  --output /tmp/compiler-timings.jsonl -- cargo nextest run --profile ci -p midenc-integration-tests
```

Run each warm command at least three times after an unmeasured warm-up and compare the median. For
an actual cold Cargo build, use a new, empty `CARGO_TARGET_DIR`; record whether the registry and
compiler caches were already populated. Do not delete the shared target directory or call a build
cold merely because it is the first measurement. Changing `CARGO_TARGET_DIR` can also change fixture
caches, so record that scope explicitly. Run the exact same command again for its warm comparison.
For an edit/rebuild comparison, record the exact source diff as well as the command; a timestamp-only
touch measures Cargo invalidation without representing a meaningful compiler edit.

Measure these separately:

- A tool build with an empty target directory and then unchanged with the same target directory.
- The unchanged suite with populated tool and fixture caches.
- The same suite after a recorded compiler source edit.
- A focused package after that edit, with the same profile and features.

## Test and compiler detail

CI selects nextest's `ci` profile and uploads `target/nextest/ci/junit.xml` as
`test-results-<job-id>`, even when tests fail. JUnit includes per-test durations and failed-test
output. A build failure before nextest starts may produce no report. Lit is a separate runner and is
not represented by this XML. Local runs can select `--profile ci` too. See the
[nextest JUnit documentation](https://www.nexte.st/docs/machine-readable/junit/).

Set `MIDENC_TEST_TIMINGS=1` and show captured output (`--nocapture` for libtest or
`--success-output immediate` for nextest) to collect integration-test compiler checkpoint timings.
The observer records timestamps without retaining artifacts or doing output I/O, then prints after
the pipeline returns. Records name the artifact and checkpoint, giving both the interval since the
previous root checkpoint and elapsed time since the request started. `returned` includes the final
interval and total. Cached `CompilerTest` accessors do not launch another pipeline or emit another
series.

These are **checkpoint intervals**, not exclusive CPU time for named compiler passes. They include
dependency work, requested artifact captures, scheduling, and other observers between root
checkpoints. Different frontends publish different checkpoints; compare matching routes. The
shared nested-Cargo boundary separately emits `cargo_ms`, including lock waits, compilation,
subprocess output parsing, and process completion. It does not claim to separate Cargo lock wait
from compilation. Cargo intervals are contained in pipeline intervals and must not be added to the
total. The existing session `Statistics` fields are not used: they record cumulative timestamps,
and current pipeline code does not populate them.

## Measurements

Measured 2026-09-05 UTC on macOS 26.4.1 ARM64, revision `51d5590cf` plus the
uncommitted timing instrumentation and other audit changes. Toolchain:
`nightly-2026-09-01` (`rustc 1.100.0-nightly (0dfb098f3 2026-08-31)`). All eight
executions passed. Builds were paused during these samples. These measurements cover a prebuilt
**debug integration-test binary**, not an outer Cargo build or the whole suite.

| Sample | Process wall (s) | Pipeline (s) | Nested Cargo (s) |
| --- | ---: | ---: | ---: |
| Warm prime 1 | 8.209 | 1.749 | 0.529 |
| Warm prime 2 | 3.729 | 1.453 | 0.249 |
| Warm prime 3 | 3.822 | 1.578 | 0.260 |
| Warm prime median | 3.822 | 1.578 | 0.260 |
| WAT package 1 | 0.315 | 0.285 | — |
| WAT package 2 | 0.324 | 0.299 | — |
| WAT package 3 | 0.299 | 0.277 | — |
| WAT package median | 0.315 | 0.285 | — |
| Isolated nested cache, cold prime | 17.217 | 15.039 | 13.817 |
| Same nested cache, warm prime | 3.814 | 1.601 | 0.298 |

The first warm prime process was an outlier: libtest reported 3.96 seconds while external process
wall time was 8.209 seconds. The discrepancy is outside the measured pipeline; it is retained in the
table rather than silently excluded. The warm series had populated Cargo caches, but no separate
unmeasured process warm-up. More repetitions are needed before drawing narrow timing conclusions.

Prime compiles once, retains HIR, and checks all 30 inputs `0..30` through HIR evaluation and MASM
execution. The WAT test compiles one constant-returning module and verifies cached-package reuse
and absence of intermediate captures. Its caught assertion panics are intentional assertions of
that API contract. Pipeline totals therefore exclude the prime execution/evaluation work and test
process overhead.

The isolated cold/warm pair used a newly created empty `CARGO_TARGET_DIR`; test support derives
its `miden_test_shared` final-artifact directory and `miden_build_cache` intermediates beneath that
root. The compiler binary, Rust toolchain, registry downloads, and system caches remained warm.
This is an actual **nested fixture-cache** cold/warm comparison, not a cold compiler-tool build.
The cold Cargo subprocess dominated its pipeline (13.817 of 15.039 seconds). A single pair does
not establish an overall suite speedup. Cold outer-tool builds and source-edit rebuild scenarios
remain unmeasured in this table; build-profile experiments are separate.

The exact commands and environment are preserved in [the JSONL records](compiler-audit-timings.jsonl).
Commands ran from `tests/integration` with `CARGO_MANIFEST_DIR` set to that absolute directory,
`RUSTUP_TOOLCHAIN=nightly-2026-09-01`, and `MIDENC_TEST_TIMINGS=1`. Each invocation selected one
exact test and used `--nocapture --test-threads=1`. Directly launching a Cargo-built test binary
requires those environment values; otherwise fixture toolchain and manifest discovery can differ
from a normal Cargo test run.

Warm prime 3 illustrates why root checkpoint names must not be read as exclusive phase names:


```text
midenc-timing cargo_ms=260.003 status=exit status: 0 directory=None
midenc-timing artifact="is-prime" checkpoint=dependencies.staged interval_ms=1154.326 elapsed_ms=1154.326
midenc-timing artifact="is-prime" checkpoint=wasm.parsed interval_ms=315.001 elapsed_ms=1469.327
midenc-timing artifact="is-prime" checkpoint=hir.initial interval_ms=3.601 elapsed_ms=1472.928
midenc-timing artifact="is-prime" checkpoint=hir.analyzed interval_ms=0.003 elapsed_ms=1472.931
midenc-timing artifact="is-prime" checkpoint=hir.transformed interval_ms=62.838 elapsed_ms=1535.769
midenc-timing artifact="is-prime" checkpoint=masm.lowered interval_ms=16.591 elapsed_ms=1552.360
midenc-timing artifact="is-prime" checkpoint=package.assembled interval_ms=25.359 elapsed_ms=1577.719
midenc-timing artifact="is-prime" checkpoint=returned interval_ms=0.097 elapsed_ms=1577.816
```
