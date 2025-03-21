# Getting started with Cargo

As part of the Miden compiler toolchain, we provide a Cargo extension, `cargo-miden`, which provides
a template to spin up a new Miden project in Rust, and takes care of orchestrating `rustc` and
`midenc` to compile the Rust crate to a Miden package.

## Installation

> [!WARNING]
> Currently, `midenc` (and as a result, `cargo-miden`), requires the nightly Rust toolchain, so
> make sure you have it installed first:
>
> ```bash
> rustup toolchain install nightly-2025-01-16
> ```
>
> NOTE: You can also use the latest nightly, but the specific nightly shown here is known to
> work.

To install the extension, clone the compiler repo first:

```bash
git clone https://github.com/0xpolygonmiden/compiler
```

Then, run the following in your shell in the cloned repo folder:

```bash
cargo install --path tools/cargo-miden --locked
```

This will take a minute to compile, but once complete, you can run `cargo help miden` or just
`cargo miden` to see the set of available commands and options.

To get help for a specific command, use `cargo miden help <command>` or `cargo miden <command> --help`.

## Creating a new project

Your first step will be to create a new Rust project set up for compiling to Miden:

```bash
cargo miden new foo
```

In this above example, this will create a new directory `foo`, containing a Cargo project for a
crate named `foo`, generated from our Miden project template.

The template we use sets things up so that you can pretty much just build and run. Since the
toolchain depends on Rust's native WebAssembly target, it is set up just like a minimal WebAssembly
crate, with some additional tweaks for Miden specifically.

Out of the box, you will get a Rust crate that depends on the Miden SDK, and sets the global
allocator to a simple bump allocator we provide as part of the SDK, and is well suited for most
Miden use cases, avoiding the overhead of more complex allocators.

As there is no panic infrastructure, `panic = "abort"` is set, and the panic handler is configured
to use the native WebAssembly `unreachable` intrinsic, so the compiler will strip out all of the
usual panic formatting code.

## Compiling to Miden package

Now that you've created your project, compiling it to Miden package is as easy as running the
following command from the root of the project directory:

```bash
cargo miden build --release
```

This will emit the compiled artifacts to `target/miden/release/foo.masp`.


## Running a compiled Miden VM program


> [!WARNING]
> To run the compiled Miden VM program you need to have `midenc` installed. See [`midenc` docs](./midenc.md) for the installation instructions.


The compiled Miden VM program can be run from the Miden package with the following:

```bash
midenc run target/miden/release/foo.masp --inputs some_inputs.toml
```

See `midenc run --help` for the inputs file format.



## Examples

Check out the [examples](https://github.com/0xPolygonMiden/compiler/tree/next/examples) for some `cargo-miden` project examples.
