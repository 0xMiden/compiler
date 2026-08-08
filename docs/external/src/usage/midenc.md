---
title: As an Executable
sidebar_position: 2
---

# Getting started with `midenc`

The `midenc` executable is the command-line interface for the compiler driver, as well as other helpful tools, such as the interactive debugger.

While it is a lower-level tool compared to `cargo-miden`, just like the difference between `rustc` and `cargo`, it provides a lot of functionality for emitting diagnostic information, controlling the output of the compiler, and configuring the compilation pipeline. Most users will want to use `cargo-miden`, but understanding `midenc` is helpful for those times where you need to get your hands dirty.

## Installation

To install `midenc`, you have two choices:

1. Install via [`midenup`](https://github.com/0xMiden/midenup), which also handles other toolchain components that you'll likely want.
2. Install from source

We'll cover installation from source here - see the `midenup` README for details on how to install Miden components that way.


First, clone the compiler repo:

```bash
git clone https://github.com/0xMiden/compiler
```

Then, run the following in your shell in the cloned repo folder:

```bash
cargo install --path midenc --locked
```

## Usage

Once installed, you should be able to invoke the compiler, you should see output similar to this:

```text
midenc --help
Usage: midenc [OPTIONS] [FILE]

Arguments:
  [INPUTS]...
          Path(s) to the source file(s) to compile.

          You may also use `-` as a file name to read a file from stdin.

Options:
  -p, --package <SPEC>
          Package(s) to build

      --manifest-path <PATH>
          Path to the package/project manifest

          If unspecified, the compiler will create a virtual manifest for the given input

  -h, --help
          Print help (see a summary with '-h')

  -V, --version
          Print version

..snip..
```

The actual help output covers quite a bit more than that - see the actual command output for the full picture.

## Compilation

See the help output for `midenc` for detailed information on its options and their behavior. However, the following is an example of how one might use `midenc` in practice:

```bash
midenc --entrypoint 'foo::main' \
    -lextra \
    -L ./masm \
    --emit=hir=- \
    -o out.masp \
    target/wasm32-wasip1/release/foo.wasm
```

In this scenario, we are in the root of a Rust crate, named `foo`, which we have compiled for the `wasm32-wasip1` target, which placed the resulting WebAssembly module in the `target/wasm32-wasip1/release` directory. This crate exports a function named `main`, which we want to use as the entrypoint of the program.

Additionally, our Rust code links against some hand-written Miden Assembly code, namespaced under `extra`, which can be found in `./masm/extra`. We are telling `midenc` to link the `extra` library, and to add the `./masm` directory to the library search path.

Lastly, we're configuring the output:

- We're using `--emit` to request `midenc` to dump Miden IR (`hir`) to stdout (specified via the `-` shorthand), in addition to the Miden package artifact (produced by default).
- We're telling `midenc` to write the compiled output to `out.masp` in the current directory, rather than the default path that would have been used (`target/miden/foo.masp`).

### Stopping early

`--stop-after=CHECKPOINT` ends compilation once the named phase has run, rather than building a package. It is most useful together with `--emit`, to look at an intermediate form without paying for the rest of the build:

```bash
midenc --stop-after=transform --emit=hir=- foo.wasm
```

`CHECKPOINT` is either an alias — `parse`, `analyze`, `transform`, `lower`, `assemble` — or a fully-qualified checkpoint id such as `hir.initial`. Which names are valid depends on the input: each frontend declares its own route, and a name that route does not reach is reported along with the names it does accept. A Miden Assembly input, for example, has no `transform` phase.

## Debugging

See [Debugging Programs](../guides/debugger.md) for details on how to debug Miden programs using `miden-debug`.

## Next steps

We have put together two useful guides to walk through more detail on compiling Rust to WebAssembly:

1. To learn how to compile Rust to WebAssembly so that you can invoke `midenc compile` on the
   resulting Wasm module, see [this guide](../guides/rust_to_wasm.md).
2. If you already have a WebAssembly module, or know how to produce one, and want to learn how to
   compile it to Miden Assembly, see [this guide](../guides/wasm_to_masm.md).

To start from a working project rather than an empty one, use `cargo miden new`:

```bash
cargo miden new my-project            # a full project scaffold
cargo miden new my-account --account  # a single account component
```

The available templates are `--account`, `--note`, `--tx-script`,
`--auth-component`, and `--program`. Their sources live in this repository under
[`extra/templates`](https://github.com/0xMiden/compiler/tree/main/extra/templates)
and are released independently of the compiler, so `cargo miden new` picks up
template updates without you reinstalling `cargo-miden`.
