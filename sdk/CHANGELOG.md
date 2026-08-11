# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- `println!` supports format arguments; formatted output requires `extern crate alloc` and a
  configured global allocator.
- `#[note]` impl blocks generate a `get_entrypoint_root()` associated method on the note type,
  returning the MAST root digest of the note script defined by the current crate (the
  `#[note_script]` entrypoint export). It is backed by a compiler intrinsic resolved at
  assembly time, so a note constructor can build a `Recipient` committing to its own note
  script root (e.g. via `note::build_recipient`). Compilation fails if the project defines no
  `#[note_script]` entrypoint #786
- `#[note_constructor]` marks a method of a `#[note]` impl block as an exported note
  constructor: it is exported through the note's WIT interface, so other Miden packages — e.g.
  transaction scripts — can declare the note package as a dependency and call the constructor
  to compute the note's recipient (committing to the note script root). The caller turns the
  recipient into an output note through an account procedure (e.g. the basic wallet's
  `create-note`), because `output_note::create` requires the account-component context.
  Constructors must be `pub`, must not take `self`, and their signatures are limited to SDK
  core types and primitives; unannotated methods stay plain Rust helpers #786
- The `#[note]` macro writes the note's WIT interface (entrypoint plus constructors) to
  `target/generated-wit/`, so dependent crates can reference it via
  `[package.metadata.component.target.dependencies]` #786
- `#[tx_script]` entrypoints now take typed script arguments: the entrypoint is a free function
  of any name whose by-value parameter can be any `ScriptArgs` type (every
  `FromFeltRepr + ToFeltRepr` type qualifies, e.g. `Felt`, `Word`, or a user struct deriving
  both) and is decoded automatically from the `TX_SCRIPT_ARGS` word — packed directly in the
  args word when the encoding statically fits its 4 felts, or fetched from the advice provider
  and hash-verified against the args word otherwise. The parameters may appear in any order
  (the account reference is optional); existing `fn run(arg: Word, ...)` signatures keep
  compiling and behave unchanged. On the host, `ScriptArgs::encode` on a struct with the
  identical layout yields the args word or the advice-map preimage (`EncodedScriptArgs`). The
  trait lives in the new `miden-tx-script-args` crate (re-exported from `miden`), which
  off-chain code can depend on without pulling in any on-chain SDK crates.
  `ScriptArgs::decode` returns `Result<_, ScriptArgsError>`: the generated entrypoint wrapper
  panics on decode errors (failing the transaction), while off-chain code handles them as
  values. On the basic-wallet example script, typed decoding costs about 790 VM cycles and
  4.4 KB of package size over the previous hand-rolled advice decode. See the
  [migration guide](./sdk/MIGRATION.md) for adopting typed arguments #1291
- `FromFeltRepr` gained a `FIXED_LEN: Option<usize>` associated constant — the statically known
  encoded length in felts, used by `ScriptArgs` to pick the transport mode at compile time. It
  defaults to `None` (variable length, the fail-safe commitment transport), so existing manual
  implementations keep compiling; `#[derive(FromFeltRepr)]` computes the exact value #1291
- `Tag`, `NoteType`, `Recipient`, and `Asset` now implement `FromFeltRepr`/`ToFeltRepr`, so
  they can be used as fields of derived script-argument structs. The `miden` crate additionally
  re-exports `miden_field_repr` under its own crate name, so the derives' generated code
  resolves in crates that only depend on `miden` and glob-import it #1291

### Migration and breaking changes

- `#[note]` reserves the inherent item name `get_entrypoint_root` for its generated note-script
  root accessor. Rename existing methods, note constructors, or associated constants with that
  name; see [MIGRATION.md](./sdk/MIGRATION.md).
- `#[note]` structs now also implement `felt_repr::ToFeltRepr` (mirroring the generated storage
  decoding), so note constructors can serialize the note inputs when computing the note
  recipient. Note input struct fields must now implement `ToFeltRepr` in addition to
  `FromFeltRepr`, and a manual `ToFeltRepr` impl on the note type now conflicts with the
  generated one; see [MIGRATION.md](./sdk/MIGRATION.md) for the required changes. `AccountId`
  implements `ToFeltRepr` #786


### Fixed

- `adv_load_preimage` no longer truncates huge word counts into an undersized buffer on wasm32
  (a potential guest heap overflow); it now traps for counts of `2^30` words or more, whose felt
  total cannot be represented in the 32-bit address space #1291

## [0.14.0-rc.1]

### Added

- `AssetAmount` and `AssetAmountError` provide validated fungible amounts bounded by
  `AssetAmount::MAX_U64` (`2^63 - 2^31`), with checked construction and conversions, integer
  ordering, and addition and subtraction that panic on overflow or underflow. `AssetAmount` can be
  used in exported component signatures and typed account storage. `Asset::amount()` returns a
  fungible asset's typed amount, while `Asset::is_fungible()` lets mixed-asset code check before
  calling it #999
- `FromFeltRepr` and `ToFeltRepr` are implemented for `Word`, encoding its four felt elements in
  order, so `Word` can be used directly in `#[note]` storage and other felt-representation derives
  #886
- `#[account(...)]` references accept `as Alias` to rename the generated component trait, for
  example `#[account(counter_contract::CounterContract as RemoteCounter)]`. Aliases must be
  UpperCamelCase and can resolve clashes with the wrapper name, another generated trait, or a
  sibling component trait #1208
- Account components may export a method named `new`. `Wallet::new(account_id)` remains the
  foreign-account constructor, while `wallet.new()` calls the component method #1208

### Fixed

- `#[account(...)]` now keeps canonical dependency WIT interfaces unchanged, fixing rare
  component-link failures when plain note, transaction-script, or sibling imports of a dependency
  are linked with generated FPI bindings. FPI bindings now also preserve anonymous compound types
  and allow repeated wrappers to coexist, including wrappers that select different dependency
  sets #1276

### Migration and breaking changes

- `#[account(...)]` now generates one trait per referenced interface, with the wrapper's
  visibility, instead of generating inherent methods on the wrapper. Import the generated trait at
  cross-module call sites. Rename a wrapper that has the same name as its generated trait, or use
  `as Alias`. Use distinct aliases for other generated-trait name clashes, and disambiguate
  overlapping methods with `<Wallet as Interface>::method(account, ...)`. To select an overlapping
  built-in method, import `miden::active_account::ActiveAccount` and use
  `<Wallet as ActiveAccount>::method(account, ...)`. Every referenced interface must export at
  least one callable function; empty references that were previously ignored are now rejected
  #1208
- `#[account(...)]` wrapper structs are supported only at module scope so their generated component
  metadata has a stable identity. Move wrappers declared inside a function or block to the
  enclosing module.
- Component methods must now be marked `#[account_procedure]` on the `#[component]` trait to remain
  callable from notes, transaction scripts, FPI, or sibling components. Unmarked methods remain
  exported but are not account procedures. Authentication components continue to use
  `#[auth_script]`; a component cannot combine the two markers.
- Kernel scalar APIs now use domain or integer types instead of raw `Felt`:
  - `tx::get_block_number()` returns `BlockNumber`; convert stored felts with
    `BlockNumber::try_from`, and use `as_felt()`, `as_u32()`, or `Into<Felt>` where a raw value is
    needed. `as_u32()` panics rather than truncating if a value constructed through component
    bindings exceeds the `u32` block-height limit.
  - `tx::get_block_timestamp()` returns `u32` seconds.
  - `tx::get_expiration_block_delta()` returns `u16`, with `0` meaning unset;
    `update_expiration_block_delta()` takes `u16`, and set deltas are restricted to
    `1..=u16::MAX`.
  - `active_account::get_nonce()` and `native_account::incr_nonce()` return `Nonce`; use
    `as_felt()`, `as_u64()`, or `Into<Felt>` where a raw value is needed.
  - `tx::get_num_input_notes()`, `tx::get_num_output_notes()`,
    `active_account::get_num_procedures()`, and the `num_assets` or `num_storage_items` fields of
    `OutputNoteAssetsInfo`, `InputNoteAssetsInfo`, and `InputNoteStorageInfo` return `u32`.
    `active_account::get_procedure_root()` now takes a `u32` index instead of `u8`.
  - Attachment lookup functions return `Option<u32>` instead of `AttachmentLocation`, and
    `write_attachment_to_memory()` and `write_indexed_attachment_to_memory()` bindings take `u32`
    indexes instead of `Felt`.
- SDK bindings now target VM v0.25 and the protocol 0.16 transaction-kernel API:
  - Rename `active_note::get_assets()` to `get_initial_assets()`, `input_note::get_assets()` to
    `get_initial_assets()`, and `input_note::get_assets_info()` to `get_initial_assets_info()`.
    These functions read the note's creation-time assets, unaffected by in-transaction removal.
  - Replace `active_account::has_non_fungible_asset(asset)` with `has_asset(asset_id)`, which takes
    a `Word` asset ID and tests membership for either fungible or non-fungible assets.
  - Move `get_initial_commitment()`, `get_initial_storage_commitment()`,
    `get_initial_vault_root()`, and `get_initial_asset()` from `active_account` and the
    `ActiveAccount` trait to `native_account` free functions. `storage::get_initial_item()` and
    `storage::get_initial_map_item()` keep their Rust API but now read the native account's initial
    state.
  - Remove `active_account::{get_balance, get_initial_balance}`,
    `faucet::{create_fungible_asset, create_non_fungible_asset, has_callbacks}`, and the `asset`
    module. Read asset values with `active_account::get_asset()` or
    `native_account::get_initial_asset()`; assets passed to `faucet::{mint, burn}` must be
    constructed outside the transaction.
  - `output_note::create()` is now runtime-restricted to account-component context. Note and
    transaction scripts must call an account-component wrapper to create notes.
- `midenc-frontend-wasm-metadata` now stores a list of metadata entries. Replace
  `FrontendMetadata::{to_bytes, from_bytes}` with `encode_section(&entries)` and
  `decode_section(bytes)`, which returns `Vec<FrontendMetadata>`. The serialized payload is now a
  JSON list, and exhaustive matches must handle the new `FrontendMetadata::{AccountProcedure,
  TxScript}` and `ProtocolExportKind::{AccountProcedure, TxScript}` variants.
