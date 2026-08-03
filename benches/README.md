# Miden Compiler Benchmarks

This suite builds every project under `examples/` with maximum optimization, then records the
serialized MAST forest size. Executable examples (`collatz`, `fibonacci`, and
`is-prime`) are also executed with their checked-in `inputs.toml`; their exact VM cycle count is
recorded and a debug build is used to generate an SVG flamegraph. The reported metrics always come
from the optimized package's executable MAST; package metadata and the instrumented flamegraph
build are not measured.

## Running locally

Build the compiler's Cargo frontend, then run the suite:

```bash
cargo build -p cargo-miden
cargo make bench -- --cargo-miden target/debug/cargo-miden
```

Results are written to `target/example-benchmarks/`:

- `results.json` contains machine-readable MAST sizes and VM cycles.
- `packages/` contains the optimized packages whose MAST forests were measured.
- `flamegraphs/` contains cycle-weighted SVG flamegraphs for executable examples.

Intermediate Cargo and compiler artifacts are isolated in `target/example-benchmark-build/`. Use
`--output-dir`, `--build-dir`, `--workspace-root`, or `--commit` to override the corresponding
defaults. When `--cargo-miden` is omitted, the runner invokes `cargo miden` from `PATH`.

## Pull requests

The `Examples benchmark` workflow builds `cargo-miden` from both the pull request and the current
`origin/next` HEAD. The same examples and benchmark harness are run against both compilers, and a
sticky PR comment reports cycle and MAST-size changes. Lower values are marked as improvements;
the job is informational and does not reject regressions automatically. Fork pull requests receive
the same report in the job summary because their workflow token cannot write comments. Both result
sets, compiled packages, and flamegraphs are retained as workflow artifacts.
