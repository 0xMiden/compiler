---
title: As a Cargo extension
sidebar_position: 3
---

# Getting started with Cargo

As part of the Miden compiler toolchain, we provide a Cargo extension, `cargo-miden`, which provides
a template to spin up a new Miden project in Rust, and takes care of orchestrating `rustc` and
`midenc` to compile the Rust crate to a Miden package.

## Installation

:::warning

Currently, `midenc` (and as a result, `cargo-miden`), requires the nightly Rust toolchain, so
make sure you have it installed first:

```bash
rustup toolchain install nightly-2026-09-01
```

NOTE: You can also use the latest nightly, but the specific nightly shown here is known to
work.

:::

To install the extension:

```bash
cargo +nightly-2026-09-01 install cargo-miden --locked

```

This will take a minute to compile, but once complete, you can run `cargo help miden` or just
`cargo miden` to see the set of available commands and options.

To get help for a specific command, use `cargo miden help <command>` or `cargo miden <command> --help`.
For `build`, use `cargo miden build --help`: every argument of `cargo miden build` is forwarded to
`midenc`, so that is `midenc`'s own help, and it is the one that lists them.

## Creating a new project

Your first step will be to create a new Rust project set up for compiling to Miden:

```bash
cargo miden new foo
```

In this above example, this will create a new directory `foo`, containing a Cargo project for a
crate named `foo`, generated from our Miden project template.

Templates are released independently of the compiler, so `cargo miden new`
fetches the newest compatible template bundle at run time and falls back to a
copy embedded in `cargo-miden` — you get template fixes without reinstalling,
and project creation still works with no network access.

Pass `--force-download` to require the released bundle and fail rather than
silently falling back, which is useful for confirming what a released template
actually produces:

```bash
cargo miden new foo --force-download
```

Pass `--template-path <dir>` to generate from templates on disk instead.

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

This will emit the compiled artifacts to `target/miden/release/foo.masp`, and print the path of
the compiled Miden package on success.

`cargo miden build` is a thin wrapper: every argument after `build` is passed to
[`midenc`](midenc.md) unchanged, so `cargo miden build <args>` compiles what `midenc <args>` would
in the same directory. Run `cargo miden build --help` to see the options — that help is `midenc`'s.

The project is located the same way too: the `miden-project.toml` in the current directory, or the
`Cargo.toml` there when no Miden manifest exists beside it, or the one `--manifest-path` names.
Logging is controlled by `MIDENC_TRACE`, the same variable `midenc` reads.

## Naming

Account component, note and transaction script projects are named by the `[lib].namespace` key of
their `miden-project.toml`. It is a Miden path of exactly three segments, `ns::pkg::iface`, where
each segment is a snake_case identifier: lowercase ASCII letters and digits in words joined by
single `_`, starting with a letter (`[a-z][a-z0-9]*(_[a-z0-9]+)*`, e.g. `counter_contract` or
`wallet2`), and not a WIT keyword such as `list` or `type`. The first two segments form the WIT
package id, so the `pkg` segment identifies the crate and must not be shared by two crates a
consumer links. Projects created with `cargo miden new` default to `miden::<package>::<package>`
(package name in snake_case):

```toml
[package]
name = "counter-contract"
version = "0.1.0"

[lib]
kind = "account-component"
namespace = "miden::counter_contract::counter_contract"
path = "src/lib.rs"
```

The SDK macros derive every other name from the namespace:

| Item | Rule | Example |
|------|------|---------|
| WIT interface id | `ns:pkg/iface@<[package].version>`, `_` replaced by `-` | `miden:counter-contract/counter-contract@0.1.0` |
| Exported procedure | `<namespace>::<Rust fn name>` | `miden::counter_contract::counter_contract::get_count` |
| Storage slot | `<namespace>::<field name>` | `miden::counter_contract::counter_contract::count_map` |
| Transaction script entrypoint | `<namespace>::run` | `miden::basic_wallet_tx_script::basic_wallet_tx_script::run` |

Paths never contain a version. Changing the namespace changes procedure paths and storage slot
names, so treat it as part of the component's on-chain interface.

All exports of a package sit directly under its namespace. Calls into a dependency use the
dependency's paths as they are, e.g. a note calling the basic wallet calls
`miden::basic_wallet::basic_wallet::receive_asset`.

If you write WIT by hand instead of generating it with the SDK macros, every function must carry its
full Miden path in an `@external-id` attribute, otherwise compilation fails:

```wit
interface counter-contract {
    @external-id("miden::counter_contract::counter_contract::get_count")
    get-count: func() -> felt;
}
```

When compiling a bare `.wasm` component without a manifest or an explicit namespace, the namespace is
taken from the common parent path of the component's exports. If a manifest namespace is given and
does not match the exports, compilation fails.

## Running a compiled Miden VM program

Use `miden-debug` to execute the compiled package. See [Debugging programs](../guides/debugger.md)
for installation and the input-file format.

```bash
miden-debug target/miden/release/foo.masp --inputs some_inputs.toml
```

This opens the interactive debugger. For terminal commands, add `--repl`; for a non-interactive
run, use `--commands` with a debugger command file, as described in the guide. Run
`miden-debug --help` for the available options.

## Examples

Check out the [examples](https://github.com/0xMiden/compiler/tree/next/examples) for some `cargo-miden` project examples.
