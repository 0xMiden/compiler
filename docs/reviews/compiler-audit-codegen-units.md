# Development codegen-unit experiment

## Decision and scope

The global development setting remains `codegen-units = 1` pending repeatable performance evidence
and equivalent Linux registration checks.
The macOS repetitions provide insufficient repeatable evidence to change the global default.
Both settings passed the selected registration-dependent tests on this host.

Compare **explicit package overrides for one versus 16 codegen units for all 41 workspace
packages**, using `/private/tmp/audit-cgu1.toml` and `/private/tmp/audit-cgu16.toml`. Keep third-party
settings unchanged between arms. Both explicit configurations are required: comparing the candidate
against the bare shipping profile introduces a debug-information confound. Do not replace the global
setting for this experiment. Keep the toolchain, checkout, features, target directory, linker,
and `--build-jobs 8` fixed.

The 1,045-unit graphs were compared by recursive normalized unit identity, including dependency-edge
attributes and referenced unit identities, with duplicate-unit multiplicities and root identities
preserved. Numeric indices and unit ordering were not used to pair graphs. Between the explicit
arms, only `codegen_units` differs: 95 workspace units change from 1 to 16. Third-party profiles,
features, targets, dependency edges, and all other profile fields match.

The explicit one-unit baseline is **not identical to the shipping profile**. Relative to shipping,
17 workspace build units change `codegen_units` from Cargo's unspecified/default value to 1, and four
build-dependency library units change debug information from 0 to 2: `miden-field-repr`,
`midenc-frontend-wasm-metadata`, `midenc-log`, and `midenc-session`. The explicit 16-unit arm has the
same debug-information changes, so those differences are controlled within the experiment.
Consequently, this comparison isolates the explicit workspace CGU choice; it does not directly
measure a speedup over the shipping configuration.

This measures a workspace rebuild with third-party artifacts cached. It is neither an empty-target
build nor a typical one-file edit. It also does not measure suite execution speed.

## Commands for the coordinated experiment

Run from the workspace root only after the current suite and nested fixture builds have exited.
Capture workspace package IDs once; IDs avoid ambiguous package names. Check that the override
files cover exactly these workspace packages before timing either arm. Generate both from the same
metadata rather than maintaining hand-written package lists:

```sh
cargo metadata --format-version 1 --no-deps > /private/tmp/audit-cgu-metadata.json
python3 - <<'PY'
import json, tomllib
from pathlib import Path
metadata = json.loads(Path('/private/tmp/audit-cgu-metadata.json').read_text())
members = set(metadata['workspace_members'])
names = {p['name'] for p in metadata['packages'] if p['id'] in members}
for units in (1, 16):
    path = Path(f'/private/tmp/audit-cgu{units}.toml')
    path.write_text(''.join(
        f'[profile.dev.package.{json.dumps(name)}]\ncodegen-units = {units}\n\n'
        for name in sorted(names)
    ))
    config = tomllib.loads(path.read_text())
    overrides = config['profile']['dev']['package']
    assert set(overrides) == names, (names - set(overrides), set(overrides) - names)
    assert all(value == {'codegen-units': units} for value in overrides.values())
PY
```

Before **each** measured rebuild, clean the same complete set of workspace package artifacts.
Retain third-party and nested fixture caches. This command is deliberately not `cargo clean`
without package selections:

```sh
python3 - <<'PY'
import json, subprocess
from pathlib import Path
metadata = json.loads(Path('/private/tmp/audit-cgu-metadata.json').read_text())
arguments = ['cargo', 'clean']
for package_id in sorted(metadata['workspace_members']):
    arguments.extend(['-p', package_id])
subprocess.run(arguments, check=True)
PY
```

Baseline build and inventory:

```sh
python3 tools/time-command.py --label workspace-rebuild-cgu1-1 \
  --output /private/tmp/audit-cgu-timings.jsonl -- \
  cargo nextest list --config /private/tmp/audit-cgu1.toml --workspace --locked --offline --build-jobs 8 --message-format json
cargo nextest list --config /private/tmp/audit-cgu1.toml --workspace --locked --offline --build-jobs 8 --message-format json > /private/tmp/audit-cgu1-inventory.json
```

After repeating the package cleanup, candidate build and inventory:

```sh
python3 tools/time-command.py --label workspace-rebuild-cgu16-1 \
  --output /private/tmp/audit-cgu-timings.jsonl -- \
  cargo nextest list --config /private/tmp/audit-cgu16.toml --workspace --locked --offline --build-jobs 8 --message-format json
cargo nextest list --config /private/tmp/audit-cgu16.toml --workspace --locked --offline --build-jobs 8 \
  --message-format json > /private/tmp/audit-cgu16-inventory.json
```

Keep both override files and all three unit graphs (shipping, explicit 1, explicit 16) with the results. Repeat in alternating order (for example 1,16,16,1)
when affordable, cleaning workspace artifacts before every timed run. Record the order; a single
pair is vulnerable to filesystem-cache and thermal effects. Monitor build output for unexpected
third-party recompilation. If one arm rebuilds dependencies that the other reuses, report that
as a confound rather than attributing the difference entirely to workspace codegen units.

Compare `(binary ID, test name, ignored flag)` sets, not binary paths or hashes, which change with
codegen configuration. Require exact equality between the two fixed-revision inventories.
Listing tests alone does not exercise dialect and pass registration.

After each arm, run registration-dependent checks while preserving the workspace build graph:

```sh
cargo nextest run --config /private/tmp/audit-cgu1.toml --workspace --locked --offline --build-jobs 8 -E \
  'package(midenc-hir) or package(midenc-hir-opt) or package(midenc-compile) or (package(midenc-integration-tests) and test(harness::))'
```

For the candidate, replace the configuration path with `/private/tmp/audit-cgu16.toml`.
The installed nextest calls Cargo build concurrency `--build-jobs`; its test execution concurrency
is a separate option. Pass Cargo configuration through nextest's `--config` option explicitly.
Using a workspace build plus a nextest test filter avoids changing feature unification merely to
select tests. Record actual selected test names and pass/fail results. Include equivalent Linux
execution before relaxing a platform-wide registration safeguard.

## Results

Host: Apple M4 (`Mac16,12`), 10 logical CPUs, 32 GiB memory, macOS 26.4.1 ARM64.
Revision `e27b923d506efe8cfdc88b43222676044c41a266`, plus the opcode-compatibility and allocator-snapshot
corrections in this change set and documentation work in progress. Production source and test
expectations stayed fixed across all four samples.
Rust `1.100.0-nightly (0dfb098f3 2026-08-31)`, toolchain `nightly-2026-09-01`.
The experiment uses ABBA order: explicit 1, explicit 16, explicit 16, explicit 1.
The host was not reserved. A process snapshot during the second candidate build showed only this
checkout's rustc processes; no continuous host-contention monitoring was performed.

| Configuration / order | Cargo build duration (s) | Complete process wall (s) | Test executable bytes | Inventory |
| --- | ---: | ---: | ---: | ---: |
| Explicit 1 / A1 | 38.22 | 70.621 | 2,454,616,056 | 2,519 |
| Explicit 16 / B1 | 46.95 | 77.675 | 2,472,123,784 | 2,519 |
| Explicit 16 / B2 | 119 (reported as 1m 59s) | 167.222 | 2,472,123,784 | 2,519 |
| Explicit 1 / A2 | 103 (reported as 1m 43s) | 173.003 | 2,454,616,056 | 2,519 |

Cargo's reported build duration and external process wall time are separate measurements. The
complete `cargo nextest list --message-format json` process also launches and lists 45 test binaries.
Its extra wall time must not be described as compiler or linker duration. Executable bytes sum the
listed test binaries, not all build artifacts. No peak-memory measurement was collected. Each of
the four build logs contains 39 workspace `Compiling` lines and no third-party compilation lines, consistent with the intended
dependency-cache control.

All four inventories contain identical `(binary ID, test name, ignored flag)` entries, not
merely equal test counts. Both configurations also passed **372 actual tests** covering registration,
frontend/macro/HIR behavior, six snapshot controls, and the fixed-WAT wide-product regression.
Those checks exercise discovery and execution on this macOS host. They do not establish Linux
registration behavior, and their execution durations are not a controlled suite-runtime comparison.

Both second repetitions were substantially slower than their first repetitions despite identical
configurations, inventories, and per-configuration executable sizes. Retain all four observations:
the host was not reserved and the cause of this variation was not established. With only two runs
per setting and this variance, a precise percentage-performance claim is not justified. The larger
configuration has a longer Cargo build duration in each pair, while the complete-process ordering
reverses in the second pair. It consistently produces slightly larger test binaries; these
measurements do not establish a build-latency improvement.

[Raw command records with host and Cargo-duration annotations](compiler-audit-codegen-units.jsonl)
preserve all four samples. The original command, timestamp, revision, wall time, and result fields
are retained. Cargo durations reported in minutes are rounded as shown in the build log; the JSON
includes the original duration text rather than implying subsecond precision.

This controlled comparison is against an explicit one-unit workspace configuration, not the bare
shipping profile, as described above. It does not establish a cold-build, source-edit-rebuild, or
full-suite speedup. Keep the global development setting unchanged unless subsequent supported-platform
measurements and registration checks justify a different policy.
