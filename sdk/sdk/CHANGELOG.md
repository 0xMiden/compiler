# Changelog
All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.13.1] - 2026-07-09

### Fixed
- `*_note::get_metadata` now returns a single-`Word` `NoteMetadata { header: Word }` (the note
  metadata header) instead of two words; the note attachment is no longer part of the metadata.
  Retrieve attachments via `*_note::{get_attachments_commitment, find_attachment,
  write_attachment_commitments_to_memory, write_attachment_to_memory}` instead. See the
  [migration guide](./MIGRATION.md).
- The stack return areas of `stdlib::crypto::hashes::{hash_elements, hash_words, blake3_merge,
  sha256_merge}`, `intrinsics::crypto::merge`, `stdlib::mem::{pipe_words_to_memory,
  pipe_double_words_to_memory}`, and of all protocol bindings returning `Word`-based values
  (account/note/tx/storage/asset/faucet getters and `execute_foreign_procedure`) are now
  word-aligned. Previously they were under-aligned, so a call could trap at
  runtime with `assertion failed with error code: 0` when the compiler emitted a word-granular
  access to the returned value, depending on how the calling function's stack frame happened to
  be laid out.
- `active_note::get_storage` and the `get_assets` / `write_attachment_commitments_to_memory` /
  `write_attachment_to_memory` getters on `active_note`/`input_note`/`output_note` now match the
  transaction-kernel ABI, which returns a single count value. The compiler previously modeled a
  second (pointer) return value the kernel no longer provides, which could corrupt the VM operand
  stack at run time.

## [0.13.0] - 2026-06-29

### BREAKING
- `#[account(...)]` now generates the component methods as one trait per referenced interface
  (named after the interface, with the wrapper's visibility, implemented for the wrapper) instead
  of inherent methods on the wrapper struct. Two components that export the same method name can
  therefore coexist on one wrapper; a shared method name is no longer a compile error and is called
  with `<Wallet as Interface>::method(account, ..)`. Two consequences for existing code: the
  wrapper struct must be named differently from every generated trait (e.g.
  `#[account(counter_contract::CounterContract)] struct CounterContract;` no longer compiles —
  rename the struct, or use `as` (see Added) to rename the trait), and a component method that
  shares a name with an `ActiveAccount` built-in (e.g. `get_id`) no longer shadows it —
  disambiguate with `<Wallet as Interface>::get_id(account)` or
  `<Wallet as ActiveAccount>::get_id(account)`. Single-component accounts whose method names do not
  overlap keep calling `account.method(..)` unchanged #1208

### Added
- `#[account(...)]` references accept an `as Alias` to rename the generated trait, e.g.
  `#[account(counter_contract::CounterContract as RemoteCounter)]`. The path still selects the
  interface; only the generated trait is renamed. Use it when the interface name would clash with
  the wrapper struct, with another referenced interface, or with a sibling `#[component(...)]`
  trait of the same interface in the same crate #1208

## [0.13.0]

### BREAKING
- SDK bindings updated for VM v0.23 / protocol v0.15 (`miden-field` bumped to `^0.25`). `Felt::new`
  is now fallible: it returns `Result<Felt, _>` instead of `Felt`, so `Felt::new(x)` becomes
  `Felt::new(x).unwrap()` (or handle the error).
- `asset::{create_fungible_asset, create_non_fungible_asset}` now take an
  `enable_callbacks: bool` argument.
- `active_account::{get_balance, get_initial_balance}` and the corresponding
  `ActiveAccount` trait methods now take an asset key `Word` instead of a
  faucet `AccountId`.
- `faucet::{mint, burn}` no longer return an `Asset`, and the
  `faucet::{mint_value, burn_value}` helpers were removed to match the tx
  kernel API.
- `output_note::set_attachment` was removed. The attachment shape is now selected by function
  rather than a runtime `attachment_kind` argument: use
  `output_note::add_word_attachment(note_idx, attachment_scheme, attachment)` for a single word,
  `add_attachment` for a commitment, or `add_attachment_from_memory` for a multi-word attachment.
- The auto-generated `crate::bindings::Account` struct is removed. Declare the account
  explicitly with `#[account(...)]` and use that type as the note/tx-script entrypoint account
  parameter #1157
- `#[component]` no longer applies to structs or inherent impl blocks. An account component is
  now written as a `#[component_storage]` struct declaring the storage fields, a `#[component]`
  trait declaring the API, and a `#[component] impl Trait for Storage` block providing the
  behavior. The WIT interface name derives from the trait name, and `[lib].namespace` in
  `miden-project.toml` must equal the full `miden:<package>/<interface>@<version>` id (package
  from the kebab-cased `[package].name`, version from `miden-project.toml`) #697
- Storage slot names now derive from the `[lib].namespace` interface segment instead of the
  storage struct name. Slot names feed `StorageSlotId` derivation, so a component whose storage
  struct name does not match the interface segment gets different slot ids on recompile. Note
  that this also means renaming the component trait (and updating `[lib].namespace` to match)
  re-keys the storage slot ids of an already-deployed component #697
- `#[account(...)]` dependency references now require the dependency's exported WIT interface:
  write `#[account(counter_contract::CounterContract)]` instead of
  `#[account(counter_contract)]`. The interface segment is kebab-cased and validated against the
  interfaces the dependency's generated WIT exports #697

### Added
- `#[component(package::Interface, ...)]` on the component trait declares sibling component
  dependencies — other components deployed on the same account. Each reference generates a
  `pub trait` named after the interface whose default methods call the sibling component through
  the Wasm component-model boundary (an intra-account cross-context `call`, the same mechanism
  note scripts use to call the account). The generated traits attach to `#[component_storage]`
  structs automatically and may be declared as supertraits of the component trait. Each sibling
  package must be a declared dependency, and its generated WIT must be reachable through
  `[package.metadata.miden.dependencies].<name>.wit` in `miden-project.toml` (the same entry FPI
  dependencies use) #697
- `#[component]` traits may declare supertraits (e.g. `NativeAccount` and generated sibling
  traits) #697
- `#[account(...)]` on an empty struct generates a typed account wrapper exposing the methods
  of the account component packages listed in the attribute. The same type serves both as the
  transaction's native (active) account — when passed to a `#[note]`/`#[tx_script]` entrypoint —
  and as a foreign account caller created with `new(account_id)`, whose method calls are routed
  through `execute_foreign_procedure` (FPI) #1157
  For example:
  ```rust
  let counter = CounterContract::new(counter_account_id);
  let count = counter.get_count();
  ```
- Added tx-kernel SDK bindings for `native_account::get_id` and
  `tx::get_tx_script_root`.
- Added `AttachmentLocation` for note attachment lookup results.
- Added active-note bindings:
  `active_note::{is_public, is_private, get_attachments_commitment, write_attachment_commitments_to_memory, write_attachment_to_memory, find_attachment}`.
- Added note attachment and metadata bindings:
  `note::{compute_and_store_recipient, compute_storage_commitment, write_attachment_commitments_to_memory, write_attachment_to_memory, write_indexed_attachment_to_memory, compute_recipient, metadata_into_sender, metadata_into_attachment_schemes, metadata_into_note_type, metadata_into_tag, find_attachment_idx}`.
- Added input-note bindings:
  `input_note::{get_attachments_commitment, get_attachments_commitment_raw, write_attachment_commitments_to_memory, write_attachment_to_memory, find_attachment}`.
- Added output-note bindings:
  `output_note::{add_word_attachment, add_attachment, add_attachment_from_memory, get_attachments_commitment, find_attachment, write_attachment_commitments_to_memory, write_attachment_to_memory}`.
- Added `faucet::has_callbacks`.
- `println!` macro (and `debug::println`) for emitting a debug message during execution.

## [0.12.0] - 2026-04-16

### BREAKING
- `#[auth_script]` attribute macro is required to mark the authentication procedure in the authentication component #1051

## [0.11.0]

### BREAKING
- `Felt` and `Word` API changes (unified with the off-chain API).
- `Recipient::compute` removed in favor of `build_recipient` binding.
- Account storage `StorageMap` became `StorageMap<K,V>` and `Value` became `StorageValue<T>` where `K`, `V` and `T` have to be convertible to and from `Word` #987

### Fixed
- Fixed `pipe_words_to_memory` binding;


## [0.10.0]

### BREAKING
- Remove `miden::active_note::add_assets_to_account` #932
- `*_note::get_metadata` now returns `NoteMetadata` (2 `Word`s) #932

## [0.9.0]

### BREAKING
- Note scripts now use a struct-based API: replace `#[note_script] fn run(...)` with `#[note]` on a note input `struct` and `#[note]` on an inherent `impl` block containing exactly one `#[note_script]` entrypoint method #890. See an example: [before](https://github.com/0xMiden/project-template/blob/6cd50a3312dffba1826fd4f812bc431da7f51d5f/contracts/increment-note/src/lib.rs) and [after](https://github.com/0xMiden/project-template/blob/1dd023311021800002e3a9fb687e936991877e65/contracts/increment-note/src/lib.rs).
- Storage slot IDs are now derived from slot names; `#[storage(slot(...))]`/`slot(...)` is no longer supported, and slot name / id collisions are detected at compile time #907
- SDK bindings updated for VM v0.20 / protocol v0.13 (some bindings changed, e.g. `output_note::create(tag, note_type, recipient)`) #907
  - Previously auxiliary data could be passed into `output_note::create`. Now it can be attached to a note with `output_note::set_word_attachment`.
- Renamed `AccountId::from` to `AccountId::new` #808

### Added
- `ToFeltRepr` and `FromFeltRepr` traits with `derive` macros for felt-representation encoding/decoding #808
- `Word::from_u64_unchecked` constructor #894
- Assert `value <= Felt::M` in `Felt::from_u64_unchecked` #891

### Fixed
- Reverse the return values of `NativeAccount::add_asset` #862
- Correct operand order in `Felt` `le`/`lt` op bindings #882

## [0.8.0]

### BREAKING
- Require `&mut` in mutating methods of the account storage;

### Added

- Pass an account as a parameter to note and tx script #798
- `ActiveAccount` and `NativeAccount` traits to call tx kernel functions via `self.*` on an account #801
- Expose `miden::note::build_recipient_hash` tx kernel function Rust equivalent as `Recipient::compute` #823
- Assert range in `Felt` constructor, moving some range checks from runtime to compile time #891

## [0.7.1](https://github.com/0xMiden/compiler/compare/miden-v0.7.0...miden-v0.7.1) - 2025-11-13

### Other

- Updated the following local packages: miden-stdlib-sys, miden-base-sys, miden-base.

## [0.7.0]
### BREAKING
- WIT interface generation in `#[component]` macro on `impl <ACCOUNT_TYPE>`. The `#[export_type]` macro is required for any type in exported function signature.
- Generate global allocator and panic handler in `#[component]`, `#[note_script]` and `#[tx_script]` macros;

## [0.6.0]

### BREAKING
- Add `#[note_script]` and `#[tx_script]` attribute macros;
- Generate Rust bindings in the attributes macros instead of in the `src/bindings.rs` file;
- Remove explicit `miden::base`(`miden.wit` file) dependency in Cargo.toml and generate it in the macros;

## [0.5.0]

### BREAKING
- Remove low-level WIT interfaces for Miden standard library and transaction protocol library and link directly using stub library and transforming the stub functions into calls to MASM procedures.

## [0.0.8](https://github.com/0xMiden/compiler/compare/miden-v0.0.7...miden-v0.0.8) - 2025-04-24

### Added
- *(sdk)* introduce miden-sdk-alloc
- introduce TransformStrategy and add the "return-via-pointer"
- lay out the Rust Miden SDK structure, the first integration test

### Fixed
- fix value type in store op in `return_via_pointer` transformation,

### Other
- treat warnings as compiler errors,
- [**breaking**] revamp Miden SDK API and expose some modules;
- [**breaking**] rename `miden-sdk` crate to `miden` [#338](https://github.com/0xMiden/compiler/pull/338)
- release-plz update (bumped to v0.0.7)
- 0.0.6
- switch all crates to a single workspace version (0.0.5)
- bump all crate versions to 0.0.5
- bump all crate versions to 0.0.4 [#296](https://github.com/0xMiden/compiler/pull/296)
- `release-plz update` (bump versions, changelogs)
- `release-plz update` to update crate versions and changelogs
- set `miden-sdk-alloc` version to `0.0.0` to be in sync with
- delete `miden-tx-kernel-sys` crate and move the code to `miden-base-sys`
- `release-plz update` in `sdk` folder (SDK crates)
- fix typos ([#243](https://github.com/0xMiden/compiler/pull/243))
- set crates versions to 0.0.0, and `publish = false` for tests
- rename `miden-sdk-tx-kernel` to `miden-tx-kernel-sys`
- rename `miden-prelude` to `miden-stdlib-sys` in SDK
- start guides for developing in rust in the book,
- introduce `miden-prelude` crate for intrinsics and stdlib
- remove `dylib` from `crate-type` in Miden SDK crates
- optimize rust Miden SDK for size
- a few minor improvements
- set up mdbook deploy
- add guides for compiling rust->masm
- add mdbook skeleton
- provide some initial usage instructions
- Initial commit

## [0.0.6](https://github.com/0xpolygonmiden/compiler/compare/miden-sdk-v0.0.5...miden-sdk-v0.0.6) - 2024-09-06

### Other
- switch all crates to a single workspace version (0.0.5)

## [0.0.2](https://github.com/0xPolygonMiden/compiler/compare/miden-sdk-v0.0.1...miden-sdk-v0.0.2) - 2024-08-30

### Other
- updated the following local packages: miden-base-sys, miden-stdlib-sys, miden-sdk-alloc

## [0.0.1](https://github.com/0xPolygonMiden/compiler/compare/miden-sdk-v0.0.0...miden-sdk-v0.0.1) - 2024-07-18

### Added
- introduce TransformStrategy and add the "return-via-pointer"
- lay out the Rust Miden SDK structure, the first integration test

### Fixed
- fix value type in store op in `return_via_pointer` transformation,

### Other
- set crates versions to 0.0.0, and `publish = false` for tests
- rename `miden-sdk-tx-kernel` to `miden-tx-kernel-sys`
- rename `miden-prelude` to `miden-stdlib-sys` in SDK
- start guides for developing in rust in the book,
- introduce `miden-prelude` crate for intrinsics and stdlib
- remove `dylib` from `crate-type` in Miden SDK crates
- optimize rust Miden SDK for size
