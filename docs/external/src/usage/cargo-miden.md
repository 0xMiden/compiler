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

The template's panic handler traps through the native WebAssembly `unreachable` intrinsic. Cargo-based
Miden builds default to `panic=immediate-abort`, which bypasses the handler and avoids retaining panic
formatting code. To invoke the handler before trapping, opt in to `abort`:

```bash
cargo miden build --release -C panic=abort
```

`abort` can introduce call graph cycles that Miden does not yet support. Projects that avoid these
cycles can already use it to print static panic messages. See
[Rust panic strategy](./midenc.md#rust-panic-strategy) for limitations and flag precedence.

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
`wallet2`), and not a WIT or Rust 2024 keyword such as `list`, `type`, `match` or `gen`. The first
two segments form the WIT package id, so they must be unique among all crates a consumer links,
whatever their versions; for the same reason the last segment must not be `core_types`, which
names the SDK's own WIT interface. The namespace must neither be nor lie under a library that
components link against: `miden::base` (the SDK's own WIT package), `miden::protocol`,
`miden::core`, `intrinsics` and `std`. Projects created with `cargo miden new` default to
`miden::<package>::<package>` (package name in snake_case):

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
names, so treat it as part of the component's on-chain interface. Exported WIT function names must
not start with `fpi-` or `dyncall-` (`fpi_`/`dyncall_` in Rust): the compiler reserves both
prefixes for the imports the SDK macros generate. A storage field's name is used as written in
its slot name and, for a `StoredProcedure` slot, in its `<namespace>::dyncall::<field>` import
path: a raw identifier loses its `r#` (`r#type` gives `<namespace>::type`), and a field name
starting with `_` or containing a non-ASCII character is rejected, since a slot name segment
cannot start with an underscore and allows only ASCII letters, digits and `_`.

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

The parent of an imported function's path (the path without the function name) names a
dependency component, so the parents of a component's imports must not nest in the component's
own namespace: a component in the namespace `acme::app` importing `acme::app::util::add` is
rejected. The parents of the imports may nest in one another: importing both `acme::math::add`
and `acme::math::u64::add` is accepted, except that the dependency whose namespace is one segment
shorter may not declare a procedure named like that segment (`acme::math` may not declare `u64`
beside `acme::math::u64`). Foreign procedure invocation (FPI) imports generated by
`#[account(...)]` and the stored-procedure imports generated by `#[component_storage]` for
`StoredProcedure` slots are exempt: their paths (`<namespace>::fpi::<dependency path>::<function>` and
`<namespace>::dyncall::<field>`) nest in the component's own namespace by design, but they call a
procedure by its root and declare no dependency component. The other paths generated by the SDK
macros satisfy the rule. A WIT import names a procedure of another component. In a library
package (the core library or a Miden Assembly library dependency), procedures tagged with a
protocol role attribute (`@account_procedure`, `@note_script`, `@auth_script`, `@tx_script`), such
as the standard account components of the `miden-standards` library, stay importable through WIT,
and every other procedure of a library package is rejected: bind a plain library procedure
natively with an `extern "C"` function carrying its path in `#[link_name]`. The procedures of
component packages stay importable.

The core Wasm module of a component is named after the crate, so a crate named like the Miden path
leaf of one of its exports (a crate `swap` exporting `miden::swap::swap::swap`) or named `init` is
rejected: rename the crate (or, for an export clash, the function). An executable without a target
namespace is rooted at its crate name, which must not be a reserved library root (`miden`,
`intrinsics`, `std`); rename the crate.

When compiling a bare `.wasm` or `.wat` component without a manifest or an explicit namespace, the
namespace is inferred only when every export has the same parent path, which becomes the
namespace; exports under different parents are an error, and a component that exports nothing is
an error unless a namespace is given. If a namespace is given (by the manifest or `--name`) and
does not match the exports, compilation fails.

## Depending on MASM account components

An account component written in Miden Assembly, such as the standard basic wallet, needs no WIT
to be used from Rust, however it is declared (a registry version, a path to a `.masp`, ...). The
SDK macros derive its interface from the package manifest:

- the WIT package is `<head>:<name>@<version>`, where `<head>` is the first segment of the
  component's namespace and `<name>` the package name without the leading `<head>-`, e.g.
  `miden:standards-wallets-basic-wallet@0.17.2` for a package named
  `miden-standards-wallets-basic-wallet` in the `miden::…` namespace. A package name that does
  not start with `<head>-` keeps it whole (`other-foo` → `miden:other-foo`), and so does one whose
  rest would be a keyword, no WIT name, or the SDK's own `base` (`miden-list` →
  `miden:miden-list`, `miden-base` → `miden:miden-base`);
- the interface is the last segment of the component's namespace (the module of its first
  offered account procedure, in path order: one not also tagged `@auth_script`) in kebab case, so
  `miden-standards-wallets-basic-wallet` provides `basic-wallet`, used as
  `#[account(miden_standards_wallets_basic_wallet::BasicWallet)]`. The interface ident in
  `#[account(pkg::Iface)]` is kebab-cased with heck, so a namespace leaf with digits like
  `ownable2step` is referenced as `Ownable2step` (`Ownable2Step` would be `ownable2-step`);
- each procedure of the component's interface (procedures tagged `@account_procedure`; an
  `@auth_script` procedure is invoked by the kernel only and left out) becomes one function with
  the parameter types of the manifest. Type aliases are not recoverable: a
  `NoteTag` or `Tag` is a `u32`, an `AssetAmount` a `felt`. Parameters are named after their
  type, with a counter on repeats (`asset`, `asset2`), and `argN` (`N` being the parameter's
  position) when the type has no name or its name has no WIT or Rust form;
- a manifest type matching a `core-types` item exactly (same name and field layout) uses that
  item; any other named type becomes a type local to the dependency's interface (the standards'
  `AccountId` matches the core `account-id`, so such procedures take and return
  `miden::AccountId`). A Rust-built component's manifest names the SDK types it uses by
  their `core-types` id (`miden:base/core-types@2.0.0/asset`), and those map to the `core-types`
  items when their fields are the item's, in its order; one with other fields (an `account-id`
  built by an SDK that put the prefix first) becomes a local type of that name instead. A type
  named by another WIT id is not supported;
- auth procedures (also when tagged `@account_procedure`), procedures without a typed
  signature, outside the interface module, with parameters or results beyond 16 stack elements
  (the pointer through which a result flattening to more than one value comes back is passed
  after the parameters but stays in the caller's context, so it does not count), with a 64-bit integer
  (`u64`/`s64`) parameter or result or a record containing a 64-bit integer field (not callable
  yet), with types WIT cannot express, with local types named `Option`, `Result`, `String` or
  `Vec` (they would clash with the Rust prelude in the bindings), whose names have no WIT form, clash,
  or would be a Rust keyword wit-bindgen does not escape (`gen`), with an export path that is not
  a Miden procedure path, with a name whose kebab form starts with `fpi-` or `dyncall-` (reserved
  for the SDK's foreign-procedure and stored-procedure imports) are left out. The reasons are
  listed in the interface's doc comment, which the generated Rust bindings carry. A procedure
  with several results returns them as one WIT `tuple`, in manifest order (e.g.
  `get-conversion-rate: func(..) -> tuple<bool, conversion-rate>`). A derived procedure can also
  be called through FPI, which puts a six-element prefix (the account id prefix and suffix and
  the procedure root) before the parameters: a parameter list that does not fit beside the
  prefix (more than 16 elements with it) travels through memory, and otherwise the prefix and
  the parameter elements must fit 16 elements. The pointer through which a result that flattens
  to more than one value is returned stays in the caller's context and does not count, so a
  procedure with exactly ten parameter elements and such a result is callable both natively and
  through FPI; among the standard components, `check_policy` of
  `miden-standards-faucets-policies-mint-allow-all` and
  `miden-standards-faucets-policies-mint-owner-controlled-owner-only` is such a procedure.

A package gets no interface at all when it has no `@account_procedure` or `@auth_script`
procedure, when its first offered account procedure (one not also tagged `@auth_script`) is not
inside a module, when its package name or namespace yields a keyword, an invalid WIT name or the
reserved `core_types` interface leaf, when its WIT package id is the SDK's own `miden:base` (even
with the package name kept whole) or the depending crate's own package id, when every one of its
procedures is left out, or when its synthesized WIT fails to parse. Such a dependency is skipped,
and the reasons are reported where a macro references it. Today three standard components are in
that situation: `miden-standards-auth-no-auth`, `miden-standards-auth-singlesig` and
`miden-standards-auth-tx-fee-collector`, whose only procedure is their auth procedure.

Like a dependency with embedded WIT, a MASM account-component dependency declared in
`miden-project.toml` has bindings generated for it in every SDK macro of the crate, whether or not
a macro references it.

```toml
# miden-project.toml
[dependencies]
miden-standards-wallets-basic-wallet = "0.17.2"
```

```rust
#[account(miden_standards_wallets_basic_wallet::BasicWallet)]
pub struct Wallet;

// in a note script taking `account: &mut Wallet`
account.receive_asset(asset);
```

The basic wallet's derived interface:

```wit
package miden:standards-wallets-basic-wallet@0.17.2;

/// Left out: none
interface basic-wallet {
    use miden:base/core-types@2.0.0.{asset, note-type, word};

    @external-id("miden::standards::components::wallets::basic_wallet::create_note")
    create-note: func(arg0: u32, note-type: note-type, arg2: word) -> u16;

    @external-id("miden::standards::components::wallets::basic_wallet::move_asset_to_note")
    move-asset-to-note: func(asset: asset, arg1: u16);

    @external-id("miden::standards::components::wallets::basic_wallet::receive_asset")
    receive-asset: func(asset: asset);
}
```

A WIT file set through `package.metadata.miden.dependencies.<name>.wit` in `miden-project.toml`
still takes precedence over the derived interface.

## Writing an account component in Miden Assembly

An account component can be written in Miden Assembly and used from Rust like any other
dependency. The project is a `miden-project.toml` next to the `.masm` sources, built with
`miden build` (`midenc`). The library kind makes it an account component, the namespace is the
module its procedures live in, and the protocol libraries are dynamic dependencies resolved from
the toolchain:

```toml
[package]
name = "counter"
version = "0.1.0"

[lib]
kind = "account-component"
namespace = "acme::counter::counter"
path = "lib/mod.masm"

[dependencies]
miden-core = { linkage = "dynamic", version = "0.36" }
miden-protocol = { linkage = "dynamic", version = "0.17.2" }
```

Each procedure of the component's interface is a `pub proc` with a typed signature and a role
attribute: `@account_procedure` for the procedures notes, scripts and other components call, and
`@auth_script` for the account's authentication procedure, which only the transaction kernel
calls. Record types are declared with `pub type ... = struct`. An abridged version of the counter
in `tests/fixtures/components/acme-counter-masm`, which keeps its count in one value slot:

```masm
use miden::protocol::active_account
use miden::protocol::native_account

#! An affine update of the count: scale it by `mul`, then add `add`.
pub type Delta = struct { add: felt, mul: felt }

const COUNT_SLOT = word("acme::counter::count")

#! Returns the count.
#!
#! Inputs:  [pad(16)]
#! Outputs: [count, pad(15)]
#!
#! Invocation: call
@account_procedure
pub proc get_count() -> felt
    exec.load_count
    swap drop
end

#! Scales the count by `delta.mul`, then adds `delta.add`.
#!
#! Inputs:  [add, mul, pad(14)]
#! Outputs: [pad(16)]
#!
#! Invocation: call
@account_procedure
pub proc add_to_count(delta: Delta)
    exec.load_count
    movup.2 mul
    add
    exec.save_count
end

#! Authenticates a transaction against the account by incrementing its nonce.
#!
#! Inputs:  [AUTH_ARGS, pad(12)]
#! Outputs: [pad(16)]
#!
#! Invocation: call
@auth_script
pub proc auth_incr_nonce(auth_args: word)
    dropw
    exec.native_account::incr_nonce drop
end

# `increment`, `get_count_word`, `wide`, `split`, `four_words`, `fifteen_to_word` and the
# `load_count`/`save_count` helpers, which read the slot with `active_account::get_item` and
# write it with `native_account::set_item`, are omitted.
```

The interface derived from the compiled package, following the rules of the previous section,
offers the account procedures and leaves the auth procedure out:

```wit
package acme:counter@0.1.0;

/// Left out: `acme::counter::counter::auth_incr_nonce`: an auth procedure is invoked by the transaction kernel in the epilogue, not by notes or scripts
interface counter {
    use miden:base/core-types@2.0.0.{felt, word};

    record delta {
        add: felt,
        mul: felt,
    }

    @external-id("acme::counter::counter::add_to_count")
    add-to-count: func(delta: delta);

    @external-id("acme::counter::counter::fifteen_to_word")
    fifteen-to-word: func(arg0: word, arg1: word, arg2: word, arg3: felt, arg4: felt, arg5: felt) -> word;

    @external-id("acme::counter::counter::four_words")
    four-words: func() -> tuple<word, word, word, word>;

    @external-id("acme::counter::counter::get_count")
    get-count: func() -> felt;

    @external-id("acme::counter::counter::get_count_word")
    get-count-word: func() -> word;

    @external-id("acme::counter::counter::increment")
    increment: func();

    @external-id("acme::counter::counter::split")
    split: func(arg0: felt) -> tuple<felt, felt>;

    @external-id("acme::counter::counter::wide")
    wide: func(arg0: word, arg1: word, arg2: word, arg3: word) -> felt;
}

world counter-world {
    export counter;
}
```

A Rust note, transaction script or account component depends on the project by path, with no
WIT of its own, and references the interface by the dependency name and the namespace leaf:

```toml
# miden-project.toml of the Rust project
[dependencies]
counter = { path = "../counter" }
```

```rust
use crate::bindings::acme::counter::counter::Delta;

#[account(counter::Counter)]
struct CounterAccount;

// in a note script taking `account: &mut CounterAccount`
account.increment();
account.add_to_count(Delta { add: felt!(5), mul: felt!(2) });
```

An account component of the same account calls it as a sibling:

```rust
#[component(counter::Counter)]
trait CallerAccount: NativeAccount + Counter {
    #[account_procedure]
    fn bump_twice(&mut self) -> Felt;
}
```

A Miden Assembly package carries no storage metadata, so `AccountComponent::from_package` cannot
instantiate it. The host builds the metadata in Rust and passes it with the initial storage slots
to `AccountComponent::new(code, slots, metadata)`, as the `miden-standards` crate does for its own
components:

```rust
let count_slot = StorageSlotName::new("acme::counter::count")?;
let count = WordSchema::new_value([
    FeltSchema::new_typed(SchemaType::native_felt(), "count"),
    FeltSchema::new_void(),
    FeltSchema::new_void(),
    FeltSchema::new_void(),
]);
let schema =
    StorageSchema::new(vec![(count_slot.clone(), StorageSlotSchema::value("the count", count))])?;
let metadata = AccountComponentMetadata::new("acme::counter").with_storage_schema(schema);
let initial = Word::from([Felt::new_unchecked(10), Felt::ZERO, Felt::ZERO, Felt::ZERO]);
let slots = vec![StorageSlot::with_value(count_slot, initial)];
let component = AccountComponent::new(AccountComponentCode::from(package), slots, metadata)?;
```

`tests/integration-network/src/mockchain/notes/user_masm_component.rs` is the worked example: it
builds the counter, instantiates it this way, and drives it from a Rust note, a transaction
script, a sibling component and through foreign procedure invocation.

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
