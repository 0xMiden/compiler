# Miden Compiler

> [!IMPORTANT]
> This project is rapidly evolving, so if you encounter bugs or other
> things which are not covered in the issue tracker, there is a good chance we know
> about them, but please do report them anyway so we can ensure they are tracked
> publically as well.

This repository contains the Miden compiler, which can be used both as a compiler backend
for existing languages that wish to target Miden Assembly, using a standard SSA-based IR;
or as means of compiling WebAssembly (Wasm) produced by another compiler to Miden Assembly.

This repo is broken into the following high-level components:

- Miden HIR (high-level intermediate representation) and it's supporting crates;
  providing everything needed to build and compile IR for a program you want to
  emit Miden Assembly for.
- The Wasm frontend; a library which can be used to convert a program compiled to `.wasm` to HIR
- The `midenc` executable, which provides a command-line tool that provides a convenient way to compile Wasm or HIR modules/programs to Miden Assembly.
- The `cargo-miden` executable, which provides a Cargo-native way of building Miden-based Rust projects.
- The `hir-opt` executable, used in compiler testing and troubleshooting.
- The `miden-objtool` executable, used for examining/analyzing compiled artifacts.

## Installing

You should install the compiler using [`midenup`](https://github.com/0xMiden/midenup), which is part of the default set of toolchain components it installs. You can invoke the compiler using `miden build`.

Alternatively, you can download prebuilt binaries from the [latest GitHub release](https://github.com/0xMiden/compiler/releases/latest), and install them somewhere in your shell's `PATH`.

> [!TIP]
> Our prebuilt binaries have attached GitHub build attestations, which can be verified using the `gh` CLI, like so:
>
> ```
> gh attestation verify midenc-aarch64-apple-darwin.tar.gz \
>    --repo 0xMiden/compiler \
>    --signer-workflow 0xMiden/compiler/.github/workflows/release.yml
> ```

## Building from source

You'll need to have Rust installed. This repository pins the toolchain in `rust-toolchain.toml` at the repo root (currently a nightly channel); use `rustup` to install that exact channel after cloning so local builds match CI.

Additionally, you'll need to have [`cargo-make`](https://github.com/sagiegurari/cargo-make) installed:

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

The debugger is called [`miden-debug`](https://github.com/0xMiden/miden-debug), see more information on how to debug compiled programs there.

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

## License

This project is dual-licensed under MIT and Apache 2.0, see [LICENSE](LICENSE.md) for details.
