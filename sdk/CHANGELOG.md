# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Added optional `codec-component` support to the new `miden-note-schema` host crate. It can
  load author-defined note codecs from a package without adding Wasmtime to the default feature
  set or the guest SDK dependency graph.
- Added typed host note-storage bindings through the new `miden-note-bindings` macros. Bindings
  can load a built note project or an exact `.masp`, generate native Rust storage types, and convert
  typed values to and from note storage. Its facade supplies all generated runtime dependencies,
  and generated string, validation, and display APIs keep stable standard-registry and
  caller-provided-registry forms as schemas gain nested types.
- The `FromFeltRepr`/`ToFeltRepr` derives accept an internal `#[felt_repr(crate_path = "...")]`
  attribute so macro-generated code can reference the runtime crate through a facade re-export.
- `#[note]` now embeds a WIT storage schema for named-field note structs in the
  `note_storage_schema` section of the compiled `.masp`. Schema records preserve Rust doc comments
  and can include nested types declared with `#[export_type]` before the note struct. Unit structs
  emit no schema.

### Migration and breaking changes

- `#[note]` storage types now require named-field or unit structs. Tuple structs no longer compile,
  and note storage fields no longer accept `Vec`. Follow the
  [migration guidance](./sdk/MIGRATION.md#rewrite-tuple-note-and-vec-storage-layouts) to preserve
  field order with named fields and replace dynamic vectors with a fixed schema.

## [0.14.0]

### Added

- `#[note]` types implement `active_note::ActiveNote`, so note scripts can read the executing
  note through methods such as `self.get_sender()`, `self.get_initial_assets()`, and
  `self.get_metadata()`, including attachment readers. Note storage remains available through
  the struct's fields #1010
- `#[note_constructor]` exports public associated functions from a `#[note]` impl, allowing other
  Miden packages to call constructors through a note package dependency. Constructors take no
  `self` parameter and support primitives and SDK core types #786
- `#[note]` impls generate `get_entrypoint_root() -> Word`, returning the note script's MAST root
  for constructing its recipient. Use it from constructors; calling it from code reachable from
  the note-script entrypoint creates a digest cycle and fails assembly. Running note scripts
  should use `active_note::get_script_root()` instead #786
- `#[tx_script]` accepts typed arguments implementing `FromFeltRepr` and `ToFeltRepr`. Statically
  sized encodings of at most four felts travel directly in `TX_SCRIPT_ARGS`; larger or variable
  encodings use an advice-map preimage whose Poseidon2 commitment is verified in the VM.
  Entrypoints may have any name, with the optional account reference before or after the argument.
  Ordinary `fn run(arg: Word, ...)` entrypoints keep their existing argument encoding #1291
- The new `miden-tx-script-args` crate provides `ScriptArgs`, `EncodedScriptArgs`, and
  `decode_preimage` for host-side encoding and decoding without guest bindings; these APIs are
  also re-exported by `miden`. For `EncodedScriptArgs::Preimage`, the host hashes the returned
  felts with `Poseidon2::hash_elements` and registers them in the advice map. Decoding returns
  `ScriptArgsError` on malformed input; a generated guest entrypoint fails the transaction on a
  decode error. Host and guest types must agree on the felt sequence and transport mode; see the
  [typed-argument guide](./sdk/MIGRATION.md#typed-transaction-script-arguments) #1291
- `FromFeltRepr::FIXED_LEN` exposes the statically known encoding length, and its derive computes
  that length for structs and enums. Manual implementations default to `None` and continue to
  compile; variable-length types use the commitment transport for typed script arguments #1291
- `AccountId` implements `ToFeltRepr`; `Tag`, `NoteType`, `Recipient`, and `Asset` implement both
  `FromFeltRepr` and `ToFeltRepr`. `miden` also re-exports `miden_field_repr`, allowing its derives
  to resolve with an SDK dependency and `use miden::*` #1291
- `println!` supports explicit format arguments, such as `println!("value={}", value)`, with
  `extern crate alloc` and a configured global allocator. Captured names in a lone string literal,
  such as `println!("value={value}")`, are still printed literally.
- The new `miden-sdk-build-script-support` crate makes dependency packages available during plain
  Cargo builds and IDE analysis through `prepare_package_cache()`. It stages dependencies without
  compiling the consuming crate twice #1298
- Package readers can obtain embedded component WIT with
  `midenc_frontend_wasm_metadata::package_wit()` and identify its section with
  `package_wit_section_id()`.

### Fixed

- Missing dependency diagnostics identify the configured package cache and expected files, or the
  dependency whose interface is missing, instead of referring to unused output directories or
  claiming that a declared dependency is undeclared #1302
- SDK crates avoid unnecessary rebuilds when alternating ordinary Cargo invocations with builds
  launched from a build script or test runner.

### Migration and breaking changes

- The SDK now targets Miden protocol and standards `0.16.0-rc.4`, VM `0.29`, and `miden-field 0.29`.
  Update matching host dependencies and use compiler `0.10.0`; rebuild `.masp` packages because
  the linked core library and transaction kernel change their commitments. SDK crates now declare
  Rust `1.99` instead of `1.97`; update toolchain pins to `nightly-2026-09-01` #1310
- Custom authentication components must construct six-word transaction summaries instead of four.
  The summary includes the account-delta, input-note, output-note, and reference-block commitments,
  followed by the expiration delta and seven user parameters; the standards convention places the
  final nonce in the first user parameter. The old layout still compiles but fails signing with
  `TransactionSummaryConstructionFailed`. Follow the
  [six-word migration](./sdk/MIGRATION.md#transaction-summaries-are-six-words-protocol-016) #1310
- Replace `output_note::set_word_attachment` with `output_note::add_word_attachment` and
  `output_note::set_array_attachment` with `output_note::add_attachment`. Both take a `Word`:
  the first takes the attachment value, the second its commitment with contents in the advice map.
  Attachments are append-only; use `add_attachment_from_memory` for a slice of words #1310
- `output_note::add_attachment_from_memory` now checks for 1–256 words before calling the kernel.
  Invalid lengths fail with an SDK assertion instead of the kernel assertion; update tests that
  depend on the previous failure message or location #1310
- `#[note]` now generates `ToFeltRepr` in field order. Custom field types must implement
  `ToFeltRepr` as well as `FromFeltRepr`; remove manual or separately derived `ToFeltRepr`
  implementations on the note struct itself to avoid conflicting implementations #786
- `#[note]` reserves the inherent name `get_entrypoint_root`. Rename existing methods or associated
  constants with this name, including those in separate impl blocks #786
- The automatically imported `ActiveNote` trait can make calls ambiguous when another trait defines
  the same method. Disambiguate with a qualified call such as
  `<MyNote as active_note::ActiveNote>::get_sender(&note)` #1010
- `#[note_script(...)]` arguments that were silently ignored are now rejected. Use the bare
  `#[note_script]` marker.
- `#[tx_script]` entrypoints must be synchronous, safe, nongeneric Rust functions returning `()`,
  without an explicit ABI or `where` clause. Move specialized code into helpers and handle results
  inside the entrypoint; the old wrapper silently discarded non-unit return values #1291
- Default `generate!` bindings now use `miden::Digest` for the SDK WIT `digest` type. Update
  guest-trait implementations and values using the separately generated record to use the SDK
  type, for example `Digest::from_word`.
- The WIT generated by `#[component]` is now embedded in the compiled package's `wit` section
  instead of being written to `target/generated-wit/`. Rebuild dependencies and remove their `wit`
  keys from `[package.metadata.miden.dependencies]` when the packages embed WIT; keeping both is
  an error. Read packaged interfaces from `.masp` files #1248
- WIT selection and embedding now recognize escaped world names, including `%interface`, and
  declarations separated by tabs, line breaks, or comments. Rebuild affected packages and
  regenerate bindings whose interfaces were previously missed.
- For packages without embedded WIT, a `wit` override must select one self-contained `.wit` file,
  or a directory containing exactly one top-level `.wit` file. It must export a named interface
  from a versioned package and resolve using bundled SDK WIT. Consolidate multi-file interfaces
  or external WIT dependencies. Bare `generate!()` can embed local `wit/` interfaces meeting these
  requirements. Packages without WIT are skipped unless a macro references their interface #1248
- SDK macros no longer search old `target/miden/<profile>` outputs or dependency source `wit/`
  directories for interfaces. Use `cargo miden build`, or add
  `miden-sdk-build-script-support = "0.14.0"` under `[build-dependencies]` and call
  `miden_sdk_build_script_support::prepare_package_cache()` from each contract's `build.rs` for
  plain Cargo and IDE workflows. Merge the call into an existing build script. The helper needs
  `cargo-miden` on `PATH`, or a binary selected by `CARGO_MIDEN` #1298
- A nonempty `MIDENC_PACKAGE_CACHE` bypasses build-script staging, so custom callers must populate
  and retain that directory themselves. An ordinary compiler build uses a temporary cache;
  building a dependency once no longer makes it available to a later plain Cargo check. The helper
  retains successful cache generations under `OUT_DIR` until cleanup and fails the outer build
  when dependency staging fails #1298
- Dependency macros now use the compiler's selected artifacts for workspace, path, git, and
  registry dependencies. A present artifact map is authoritative: malformed, incompatible, or
  incomplete maps fail expansion. Use compiler staging for workspace, git, and registry
  dependencies. Caches without a map support path dependencies, including direct `.masp`
  files #1328
- Generated bindings and FPI procedure roots refresh when dependency bytes, the artifact map,
  explicit WIT files, or `MIDENC_PACKAGE_CACHE` change. Rebuild consumers after interface changes
  and update snapshots that depended on stale bindings or procedure roots #1302
- `adv_load_preimage` now traps for `num_words >= 2^30`, preventing an overflowing felt count from
  producing an undersized guest buffer. Keep supplied word counts below this bound #1291
- `Vec<T>` felt decoding rejects impossible fixed-size element counts with `UnexpectedEof` before
  allocating; malformed lengths could previously cause an allocation abort. Variable-size elements
  are allocated as decoding proceeds. Update tests expecting the old failure #1291

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
