# Miden Compiler

> [!IMPORTANT]
> This project is a work-in-progress, so if you encounter bugs or other
> things which are not covered in the issue tracker, there is a good chance we know
> about them, but please do report them anyway so we can ensure they are tracked
> publically as well.

This repository contains the Miden compiler, which can be used both as a compiler backend
for existing languages that wish to target Miden Assembly using a standard SSA-based IR;
or as means of compiling WebAssembly (Wasm) produced by another compiler to Miden Assembly.

This repo is broken into the following high-level components:

- Miden HIR (high-level intermediate representation) and it's supporting crates;
  providing everything needed to build and compile IR for a program you want to
  emit Miden Assembly for.
- The Wasm frontend; a library which can be used to convert a program compiled to `.wasm` to HIR
- The `midenc` executable, which provides a command-line tool that provides a convenient way
  to compile Wasm or HIR modules/programs to Miden Assembly. The separate `miden-debug`
  tool executes and debugs compiled packages.

> [!TIP]
> We've published initial [documentation](https://0xMiden.github.io/compiler)
> in mdBook format for easier reading, also accessible in the `docs` directory. This documentation
> covers how to get started with the compiler, provides a couple guides for currently supported
> use cases, and contains appendices that go into detail about various design aspects of the
> toolchain.

## Building

You'll need to have Rust installed. This repository pins the toolchain in `rust-toolchain.toml` at the repo root (currently a nightly channel); use `rustup` to install that exact channel after cloning so local builds match CI.

Additionally, you'll want to have [`cargo-make`](https://github.com/sagiegurari/cargo-make) installed:

    $ cargo install cargo-make

From there, build the compiler with:

    $ cargo make

To build just the compiler:

    $ cargo make midenc

## Testing

To run the compiler test suite:

    $ cargo make test

This runs the Rust tests, including integration and template tests. To include the `lit`/FileCheck
suite, run `cargo make test-all`.

## Debugging

Use `miden-debug program.masp` to open a compiled program in the debugger. See the
[debugger guide](docs/external/src/guides/debugger.md) for installation, inputs, and batch execution.

### Emitting internal sources/artifacts

- `MIDENC_EMIT`: Environment-variable equivalent of `--emit`. Accepts the same `KIND[=PATH]` syntax
  (comma-delimited), where `PATH` is treated either as folder e.g. `MIDENC_EMIT=ir=target/emit` or file `MIDENC_EMIT=hir=my_name.hir`.
- `MIDENC_EMIT_MACRO_EXPAND[=<dir>]`: When set, integration tests dump `cargo expand`
  output for Rust fixtures to `<fixture>.expanded.rs` files in `<dir>` (or the CWD if empty/`1`).
- `MIDENC_EMIT_WIT[=<dir>]`: When set, integration tests emit the public component WIT embedded
  in each compiled package as `<fixture>.wit` and resolved macro-generated inline worlds as
  `<package>.<world>.inline.wit` in `<dir>` (or the CWD if empty/`1`). Resolved FPI worlds include
  their injected synthetic packages and `fpi-*` functions. Generated SDK integration fixtures
  enable the internal WIT-printer feature in their Cargo manifests.

## Docs

The documentation in the `docs/external` folder is built using Docusaurus and is automatically absorbed into the main [miden-docs](https://github.com/0xMiden/miden-docs) repository for the main documentation website. Changes to the `next` branch trigger an automated deployment workflow. The docs folder requires npm packages to be installed before building.

Run `cargo make docs` to install the public documentation dependencies and start the local Docusaurus server. For the same production build checked by CI, run `npm ci` and `npm run build:dev` from `docs/external`.

Internal compiler notes start at [docs/internal/src/index.md](docs/internal/src/index.md). These Markdown files are maintained separately from the public Docusaurus site; the public documentation build does not publish them.

## Packaging

TBD
