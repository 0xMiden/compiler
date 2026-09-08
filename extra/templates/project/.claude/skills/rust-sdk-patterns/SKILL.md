---
name: rust-sdk-patterns
description: Complete guide to writing Miden smart contracts with the Rust SDK. Covers the three-part #[component_storage]/#[component] account-component pattern, the #[account_procedure] interface marker, #[note]/#[note_script] notes, #[tx_script] scripts, the #[account(...)] wrapper and its generated traits, storage patterns, native functions, asset handling, cross-component calls, P2ID note creation, and asset receiving via component methods. Use when writing, editing, or reviewing Miden Rust contract code.
---

# Miden Rust SDK Patterns

## Three Contract Types

### Account Component (three-part pattern)
Defines reusable logic and storage for accounts. Accounts are composed of one or more components.

An account component is written as **three parts** — the storage struct is annotated `#[component_storage]`, and `#[component]` applies to the API trait and the impl block:

1. `#[component_storage]` on the **storage struct** — declares typed `#[storage(...)]` fields and derives slot names.
2. `#[component]` on a **trait** — the component's exported API (this is the source of the generated WIT interface).
3. `#[component]` on the **`impl Trait for Storage`** block — the behavior, wired to the guest bindings.

```rust
#![no_std]
#![feature(alloc_error_handler)]
#[macro_use]
extern crate alloc;
use miden::*;

#[component_storage]
struct BankStorage {
    #[storage(description = "initialized")]
    initialized: StorageValue<Word>,
    #[storage(description = "balances")]
    balances: StorageMap<Word, Felt>,
}

#[component]
trait Bank {
    #[account_procedure]
    fn initialize(&mut self);
    #[account_procedure]
    fn deposit(&mut self, depositor: AccountId, deposit_asset: Asset);
}

#[component]
impl Bank for BankStorage {
    fn initialize(&mut self) { /* read/write self.initialized, etc. */ }
    fn deposit(&mut self, depositor: AccountId, deposit_asset: Asset) { /* ... */ }
}
```

Only the trait's methods are exported to WIT. Inherent (`impl BankStorage`) methods stay private to the contract — use them for helpers like key derivation.

Reference: `examples/basic-wallet/src/lib.rs` and `examples/counter-contract/src/lib.rs` in the compiler repo.

### `#[account_procedure]`: being exported is not the same as being callable

A component's methods are **not** implicitly part of the account interface. Mark every method that must be reachable from a transaction script, a note script, foreign procedure invocation (FPI), or a sibling component with `#[account_procedure]`.

Three rules that catch people out:

- **It goes on the `#[component]` trait declaration, not on the impl block.** Putting it on the impl does nothing.
- **An unmarked method still compiles and is still exported by the package** — it simply is not an account procedure. There is no error at build time; the call site fails later.
- **`#[account_procedure]` and `#[auth_script]` cannot be combined in one component.** They belong to different component kinds: an ordinary account component versus an authentication component. An authentication component uses `#[auth_script]` alone, and its single method is the interface implicitly.

`#[account_procedure]` needs no import — the enclosing `#[component]` macro recognises it, exactly as it does `#[auth_script]`.

Any number of methods may be marked.

**Project metadata for accounts:** `[lib]` needs `kind`, `namespace`, and an explicit `path`; `[dependencies]` needs `miden-core` and `miden-protocol`:

```toml
# miden-project.toml, matching contracts/counter-account/miden-project.toml
[package]
name = "counter-account"
version = "0.1.0"

[lib]
path = "src/lib.rs"
kind = "account-component"
namespace = "miden:counter-account/counter-contract@0.1.0"

[dependencies]
miden-core = "*"
miden-protocol = "*"

[package.metadata.miden]
supported-types = ["RegularAccountImmutableCode"]
```

`supported-types` also accepts `"RegularAccountUpdatableCode"` and the faucet kinds `["FungibleFaucet", "NonFungibleFaucet"]`.

The project-template contract `Cargo.toml` files currently use `edition = "2021"`, `crate-type = ["cdylib"]`, and published `miden` / `miden-sdk-build-script-support` `0.14` dependencies. Copy the local manifests unless intentionally changing the template line:

```toml
[package]
name = "counter-account"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["cdylib"]

[dependencies]
miden = { version = "0.14" }

[build-dependencies]
miden-sdk-build-script-support = { version = "0.14" }
```

Contracts build on `nightly-2026-09-01` with target `wasm32-wasip2`. Use the midenup / cargo-miden command documented in this repository's `README.md` and `CLAUDE.md`; the local `build.rs` calls `miden_sdk_build_script_support::prepare_package_cache()` so source dependencies are available to the SDK macros during Cargo checks and IDE analysis.

### Note Script (`#[note]` / `#[note_script]`)
Executes when a note is consumed by an account. Can call component methods on the consuming account.

A note is two parts: a `#[note]` struct (the note inputs type) and a `#[note]` `impl` block containing exactly one `#[note_script]` entrypoint. The entrypoint takes `self` **by value**, exactly one `Word` argument, and optionally a single reference to an `#[account(...)]` wrapper (`&MyAccount` or `&mut MyAccount`). The consuming account is declared separately with `#[account(...)]`.

```rust
#![no_std]
#![feature(alloc_error_handler)]

use miden::*;

#[account(counter_account::CounterContract)]
pub struct Wallet;

#[note]
struct IncrementNote;

#[note]
impl IncrementNote {
    #[note_script]
    fn run(self, _arg: Word, account: &mut Wallet) {
        let initial_value = account.get_count();
        account.increment_count();
        let expected_value = initial_value + Felt::from_u32(1);
        let final_value = account.get_count();
        assert_eq(final_value, expected_value);
    }
}
```

A `#[note]` struct with fields is auto-decoded from `active_note::get_storage()`. The decoder is strict: it calls `ensure_eof()`, so surplus felts in the note's storage fail with `FeltReprError::TrailingData`. A zero-sized note type skips `get_storage()` entirely.

Reference: `examples/p2id-note/src/lib.rs`, `examples/p2ide-note/src/lib.rs`, `examples/counter-note/src/lib.rs`.

**Project metadata for notes:** `[lib] kind = "note"`, plus `namespace` and `path`. Conventional namespace shape is `miden:<pkg>/miden-<pkg>@0.1.0`.

### Transaction Script (`#[tx_script]`)
One-off logic executed in the context of an account. Used for initialization, admin operations, etc.

`#[tx_script]` annotates a free `fn run`. Its signature is `fn run(arg: Word)` or `fn run(arg: Word, account: &mut MyAccount)` where `MyAccount` is an `#[account(...)]` wrapper. There is no `crate::bindings::Account` — you declare the account wrapper yourself and the macro instantiates it as the active account.

```rust
#![no_std]
#![feature(alloc_error_handler)]
use miden::*;

#[account(bank_account::Bank)]
pub struct Wallet;

#[tx_script]
fn run(_arg: Word, account: &mut Wallet) {
    account.initialize();
}
```

Reference: `examples/basic-wallet-tx-script/src/lib.rs`.

**Project metadata for tx scripts:** like a note, but `[lib] kind = "tx-script"` and `namespace = "miden:base/transaction-script@1.0.0"`, plus `path = "src/lib.rs"`.

## `#[account(...)]` generates one trait per interface

`#[account(pkg::Interface)]` does **not** generate inherent methods on the wrapper struct. It generates **one trait per referenced interface**, named after the interface and carrying the wrapper's visibility, and implements it for the wrapper. Single-component accounts still call `account.method(..)` unchanged — as long as the generated trait is in scope.

The consequences worth knowing before you hit them:

- **The wrapper struct must not share a name with any generated trait.** `#[account(counter_contract::CounterContract)] struct CounterContract;` is a hard error. Rename the struct (e.g. `Counter`).
- **A call site in a different module than the wrapper must `use` the generated trait.** A same-module `#[note]` / `#[tx_script]` entrypoint sees it automatically.
- **A referenced interface must export at least one method**, or `#[account(...)]` errors.
- **Wrappers are module-scope only.**
- **Clashing method names are disambiguated with UFCS**: `<Wallet as BasicWallet>::deposit(account, asset)`. Generated traits are same-module so they need no import — but disambiguating against an `ActiveAccount` built-in does: `use miden::active_account::ActiveAccount;`.
- **Clashing trait names are renamed with `as`**: `#[account(counter_contract::CounterContract as RemoteCounter)]`. The path still selects the interface.
- **A component method named `new` is now legal.** `Wallet::new(id)` resolves to the inherent constructor, `wallet.new()` to the component method.

## Storage Slot Naming

Storage slot names are part of the on-chain storage ABI and are derived as:

```
<package_name_snake>::<interface_segment_snake>::<field_name>
```

The first segment is `[package] name` from **`miden-project.toml`** (character-sanitised, not re-snake-cased). The **middle segment is the interface segment of the `[lib].namespace`** (the part between the last `/` and `@`), snake-cased — **not** the snake-cased struct name. This deliberately decouples slot names from private Rust renames. The version suffix (`@0.1.0`) is ignored so the slot name stays stable, and there is no `slot(...)` attribute.

Live values from the pinned examples: `counter_contract::counter_contract::count_map`, `auth_component_rpo_falcon512::auth_component::owner_public_key`.

See the rust-sdk-pitfalls skill (P5) for more on slot naming.

## Storage Types

| Type | Usage | Read | Write |
|------|-------|------|-------|
| `StorageValue<T>` | Single typed slot (flags, counters, IDs) | `.get() -> T` | `.set(T) -> T` |
| `StorageMap<K, V>` | Typed key-value mapping (balances, records) | `.get(K) -> V` | `.set(K, V) -> V` |

`K: WordKey`, and `T`/`V`: `WordValue`. `WordValue` is implemented for `Word`, `Felt`, `AssetAmount`, `Digest`, `AccountId`, `Recipient`, `Tag`, `NoteIdx`, `NoteType`; `WordKey` for the same set minus `Digest` and `Recipient`.

## Native Function Modules

| Module | Key Functions | Purpose |
|--------|--------------|---------|
| `native_account::` | `add_asset(Asset) -> Word`, `remove_asset(Asset) -> Word`, `incr_nonce() -> Nonce`, `get_id() -> AccountId`, `get_initial_asset(AssetId) -> Word`, `get_initial_commitment() -> Word`, `compute_commitment() -> Word`, `has_state_changed() -> bool`, `was_procedure_called(Word) -> bool`, `compute_delta_commitment() -> Word` | Modify / read the native account |
| `active_account::` | `get_id() -> AccountId`, `get_nonce() -> Nonce`, `get_asset(asset_id: AssetId) -> Word`, `has_asset(asset_id: AssetId) -> bool`, `get_vault_root() -> Word`, `get_num_procedures() -> u32`, `get_procedure_root(u32) -> Word`, `has_procedure(Word) -> bool` | Query the active account |
| `active_note::` | `get_storage() -> Vec<Felt>`, `get_initial_assets() -> Vec<Asset>`, `get_sender() -> AccountId`, `get_recipient() -> Recipient`, `get_metadata() -> NoteMetadata`, `find_attachment(Felt) -> Option<u32>`, `write_attachment_to_memory(u32) -> Vec<Word>` | Query the note being consumed |
| `note::` | `build_recipient(Word, Word, Vec<Felt>) -> Recipient` | Build note recipients from serial number, script root, and note storage |
| `output_note::` | `create(Tag, NoteType, Recipient) -> NoteIdx`, `add_asset(Asset, NoteIdx)`, the `*_attachment` family | Create output notes |
| `faucet::` | `mint(Asset)`, `burn(Asset)` | Move assets in and out of existence |
| `tx::` | `get_reference_block_number() -> BlockNumber`, `get_block_timestamp() -> u32`, `get_num_input_notes() -> u32`, `get_num_output_notes() -> u32`, `get_expiration_block_delta() -> u16`, `update_expiration_block_delta(u16)`, `execute_foreign_procedure(..)` | Transaction context and FPI |
| Intrinsics | `assert(Felt)`, `assertz(Felt)`, `assert_eq(Felt, Felt)` | Validation (`assert` fails unless the felt equals 1; `assertz` fails unless it equals 0) |

`add_asset`, `remove_asset` and the `active_account` queries are also trait methods auto-implemented on the `#[component_storage]` struct, so the idiomatic body is `self.add_asset(asset)` rather than the free function.

### Three context restrictions the compiler will not catch

- **`native_account::incr_nonce()` may only be called from the account's authentication procedure.** The kernel asserts the caller's origin; calling it from an ordinary component method panics at runtime.
- **`output_note::create` is account-component context only.** A transaction or note script must go through a component method that wraps it — see `create_note` in `examples/basic-wallet/src/lib.rs`.
- **`native_account::add_asset` / `remove_asset` are likewise account-context only** (see pitfall P11).

### Balances and asset construction

There is no `active_account::get_balance`. Read the asset value word with `active_account::get_asset(asset_id)` (or `native_account::get_initial_asset(asset_id)` for the pre-transaction value) and take the fungible amount from it; test membership with `active_account::has_asset(asset_id)`.

There is also no in-transaction asset construction: `faucet::create_fungible_asset`, `create_non_fungible_asset` and `has_callbacks` are gone, and the `asset` module keeps only the asset-id readers behind `AssetId::faucet_id()`, `asset_class()` and `composition()`. `faucet::mint` and `faucet::burn` take an already-built `Asset`.

## Asset Handling

`Asset` is a two-word value, an asset id and a value word:

```rust
pub struct Asset {
    pub id: AssetId,   // #[repr(transparent)] over the asset-id Word (`id.inner`)
    pub value: Word,
}
```

**Constructor**: `Asset::new(id, value)` builds an Asset from its asset id and value word (`id: impl Into<AssetId>`, `value: impl Into<Word>`; a raw `Word` converts into `AssetId`).

`asset.id` is the protocol's **asset ID** — the vault's unique identifier for the asset, encoding the issuing faucet, the asset class and the composition rule. Read those through `asset.id().faucet_id()`, `asset.id().asset_class()` and `asset.id().composition()` rather than decoding the limbs.

For fungible assets the amount lives in `asset.value[0]`. Prefer the typed accessors over raw felt maths:

```rust
// Typed amount: panics if the asset is non-fungible, the amount is out of range, or the
// composition bits of the asset id are unrecognized. Fungibility is read through the protocol
// library's `asset::id_into_composition` procedure, not decoded in the SDK.
let amount: AssetAmount = asset.amount();
let fungible: bool = asset.is_fungible();

// Raw form, if you need the felt
let amount_felt = asset.value[0];

// Keep the asset id if you need to persist or compare the asset
let asset_id: AssetId = asset.id;
let asset_id_word: Word = asset.id.inner;
let held: bool = self.has_asset(asset_id);

// Vault operations (component methods only — see pitfall P11)
self.add_asset(asset);
self.remove_asset(asset);     // Asset is Copy, no clone needed
```

`AssetAmount` is a validated newtype (`MAX_U64 = 2^63 - 2^31`) with integer ordering and add/sub that panic on over/underflow. It is usable in exported signatures and as a storage value type.

## P2ID Output Note Creation

To send assets to another account, create a P2ID output note **from an account-component method** — both `output_note::create` and `native_account::remove_asset` are account-context only, so a note or tx script cannot do this inline.

The sequence is `note::build_recipient` → `output_note::create` → `remove_asset` + `output_note::add_asset`. `examples/basic-wallet/src/lib.rs` is the reference: `create_note` wraps `output_note::create`, and `move_asset_to_note` wraps the remove-then-add pair.

`note::build_recipient` panics if the note storage exceeds `MAX_NOTE_STORAGE_ITEMS` (1024 felts). A note may carry at most `MAX_ASSETS_PER_NOTE` = **16** assets.

## Cross-Component Dependencies

To call another component's methods from a note or tx script, declare the dependency in your **`miden-project.toml`** — never in `Cargo.toml`, which the macros read only for `[package] name` and `description`:

```toml
[dependencies]
miden-core = "*"
miden-protocol = "*"
basic-wallet = { path = "../basic-wallet" }
```

The `[dependencies]` entry is required. A component's WIT is embedded in its compiled package and is authoritative. Do not add a `[package.metadata.miden.dependencies].<name>.wit` override when the package already embeds WIT; current SDK macros reject that combination. An explicit WIT path is only a fallback for packages without embedded WIT.

`cargo miden build` prepares source dependencies for you. For plain Cargo checks and IDE analysis, put the build helper in the **consuming crate**: the note, transaction script, or component whose macros reference the dependency. Keep the template's matching build dependency and `build.rs` in that crate:

```toml
[build-dependencies]
miden-sdk-build-script-support = { version = "0.14" }
```

```rust
// build.rs in the consuming crate
fn main() {
    miden_sdk_build_script_support::prepare_package_cache();
}
```

This prepares `MIDENC_PACKAGE_CACHE` for that crate's macro expansion. A hook in the dependency crate does not configure the consumer. Direct `.masp` dependencies and an already valid package cache do not need this preparation.

Then expose the dependency's methods on the consuming account by declaring an `#[account(package::Interface)]` wrapper (e.g. `#[account(basic_wallet::BasicWallet)] pub struct Wallet;`) and calling methods on the injected `account` parameter. The package name is the dependency's Rust-style name (`-` replaced with `_`) and `Interface` is its exported WIT interface in UpperCamelCase.

A component can also declare siblings it calls with `#[component(pkg::Interface)]` on its own trait. The generated traits attach through a blanket impl bound on `NativeAccount`, so only the native account can make intra-account sibling calls.

There is a second, wrapper-free form: the generated bindings expose free functions, which `examples/counter-note/src/lib.rs` uses directly (`use crate::bindings::miden::counter_contract::counter_contract; counter_contract::get_count();`).

## Common Type Conversions

```rust
// Felt from integer
let f = felt!(42);                     // preferred for literals in contract code
let f = Felt::new(42).unwrap();        // fallible: Felt::new returns Result<Felt, FeltFromIntError>
let f = Felt::new_unchecked(42);       // infallible, non-reducing form
let f = Felt::from_u32(42);            // infallible (u32 always fits)
let f = Felt::from_canonical_checked(42).unwrap(); // returns Option<Felt>

// Word from Felts
let w = Word::from([f0, f1, f2, f3]);
let w = Word::new([f0, f1, f2, f3]);
let w = Word::from([0_u32, 0, 0, 1]);
let w = Word::try_from([0_u64, 0, 0, 1]).unwrap();

// Inspect a Word
let limbs: [Felt; 4] = w.into_elements();
let bytes: [u8; 32] = w.as_bytes();
let hex = w.to_hex();

// Felt to u64 (for comparisons and arithmetic safety)
let n: u64 = f.as_canonical_u64();
```

### Kernel scalars are typed, not felts

Counts are `u32` (`tx::get_num_input_notes`, `tx::get_num_output_notes`, `active_account::get_num_procedures`, and the `num_assets` / `num_storage_items` fields of the note-info structs), so count-driven loops index directly. Block heights are `BlockNumber` (comparable as integers; `BlockNumber::try_from(felt)` validates a height read out of note storage). Block timestamps are `u32` seconds and expiration deltas are `u16`. Account nonces are `Nonce` — use `as_felt()` / `as_u64()` or `Felt::from(nonce)` where a raw value is needed, e.g. when packing a nonce into a `Word`. Attachment lookups return `Option<u32>`.

## Debug Printing

```rust
miden::println!("checkpoint");        // string literal / &str only — format args are a compile error
miden::debug::println(some_str);
miden::intrinsics::debug::breakpoint();
```

## No-std Requirements

Every contract file must start with `#![no_std]` and `#![feature(alloc_error_handler)]`.

If you need heap allocation (Vec, String, etc.):
```rust
extern crate alloc;
use alloc::vec::Vec;
```

Use `#[macro_use] extern crate alloc;` when you want the `vec!` macro available (e.g. for building note-recipient inputs).

## Cross-Component Note Pattern

A note script reads from `active_note::*` or from typed `#[note]` fields and forwards work to a public account-component method via generated bindings. This is the canonical pattern for any note that updates account state, because note scripts cannot call `native_account::*` directly (see `rust-sdk-pitfalls` skill, P11).

The `#[note]` macro generates `TryFrom<&[Felt]>` for the note struct, so the note's serialized inputs are deserialized into typed fields before the script runs. The `#[note_script]` method receives the deserialized note as `self` (by value) and never has to index a raw Felt slice manually for new typed note scripts. Alongside the required `Word` arg, the method may optionally accept a `&Account` or `&mut Account` parameter. See [compiler/sdk/base-macros/src/lib.rs](https://github.com/0xMiden/compiler/blob/main/sdk/base-macros/src/lib.rs) for the macro contract and [compiler/sdk/base-macros/src/note.rs](https://github.com/0xMiden/compiler/blob/main/sdk/base-macros/src/note.rs) for the generated deserialization.

Supported field types include `Felt`, the unsigned integer scalars (`u64`, `u32`, `u8`), `bool`, and `Option<T>` over a supported type via the `FromFeltRepr` trait (`compiler/sdk/field-repr/repr/src/lib.rs`), plus any user type that opts in with `#[derive(FromFeltRepr)]` (this is how `AccountId` supports the macro - see `compiler/sdk/base-sys/src/bindings/types.rs`). `Vec<T>` is not supported: the note storage schema needs a stable, named position for each stored value, so a dynamic vector is a hard error. Two more rules follow from the schema: a `#[note]` struct must have named fields or be a unit struct (tuple structs are rejected), and a crate may contain only one `#[note]` struct (move each extra note struct into its own crate). Do **not** use `Asset` or `Word` directly as note struct fields; those types do not currently derive `FromFeltRepr`. If you need asset-shaped data inside the note, flatten it into supported scalar fields and reconstruct inside the script, or keep it on the side as a separate `active_note::get_assets()` read.

For dependency wiring, see "Cross-Component Dependencies" above. See [increment-note/src/lib.rs](../../../contracts/increment-note/src/lib.rs) for the project-template's local example of the `#[note] struct + #[note] impl` macro form.

**Storage-free case** (sender + assets, single component call per asset): declare a unit struct. The script reads the sender and attached assets from `active_note`, then calls the component method per asset. A unit note struct skips automatic storage decoding; the script can still read and validate storage explicitly when needed. The miden-bank [deposit-note](https://github.com/0xMiden/tutorials/blob/main/examples/miden-bank/contracts/deposit-note/src/lib.rs) shows this flow.

**Input-bearing case** (note carries scripted data): declare named fields on the note struct when possible. The macro deserializes them in declaration order, and the script accesses them via `self.<field>`. The miden-bank [withdraw-request-note](https://github.com/0xMiden/tutorials/blob/965739b626acf3c63a0e547d313859079ee03c18/examples/miden-bank/contracts/withdraw-request-note/src/lib.rs) shows manual parsing of 14 storage Felts: four for the asset encoding, four for the serial number, one each for the tag and note type, and four for the P2ID script root. It calls `account.withdraw(asset, serial_num, tag, note_type)` through its account wrapper; the bank reads the script root directly from the active note's storage. Keep the host serialization and the consuming script's layout in agreement.

**Component side that absorbs the call**: see [miden-bank bank-account](https://github.com/0xMiden/tutorials/blob/main/examples/miden-bank/contracts/bank-account/src/lib.rs) for `deposit(...)` and `withdraw(...)` in a fuller example. The component method validates balances, updates storage, and, for `withdraw`, creates a P2ID output note via the existing P2ID pattern.

**Test wiring**: tests pass the serialized Felt representation of the note fields through `NoteCreationConfig.storage`, in declaration order. See `rust-sdk-testing-patterns` skill, "Note Construction", for the helper that builds a note from a compiled `.masp` package and a populated `NoteCreationConfig`.

## Asset Receiving via Component Methods

Note scripts cannot call `native_account::add_asset()` directly (see pitfall P11). The canonical pattern is for an account component to expose a trait method — marked `#[account_procedure]` — that wraps `add_asset`, and for the note script to call that method through the `#[account(...)]` wrapper.

Component side (`examples/basic-wallet/src/lib.rs`):

```rust
#[component]
trait BasicWallet {
    #[account_procedure]
    fn receive_asset(&mut self, asset: Asset);
}

#[component]
impl BasicWallet for BasicWalletStorage {
    fn receive_asset(&mut self, asset: Asset) {
        self.add_asset(asset);
    }
}
```

Note side (`examples/p2id-note/src/lib.rs`): the note declares `#[account(basic_wallet::BasicWallet)] pub struct Wallet;` and, inside `#[note_script] fn script(self, _arg: Word, account: &mut Wallet)`, calls `account.receive_asset(asset)` on that wrapper. It is **not** a free `basic_wallet::receive_asset()` call.

## Validation Checklist

- [ ] `#![no_std]` and `#![feature(alloc_error_handler)]` at top of every contract
- [ ] Account components use the three-part pattern: `#[component_storage]` struct + `#[component]` trait + `#[component]` impl (never `#[component]` on a struct)
- [ ] Every externally-callable trait method carries `#[account_procedure]`, on the **trait**, not the impl
- [ ] `#[account_procedure]` and `#[auth_script]` are not combined in one component
- [ ] The `#[account(...)]` wrapper struct name differs from every generated trait name
- [ ] Contract `Cargo.toml` matches the local template shape: `edition = "2021"`, `crate-type = ["cdylib"]`, `miden = { version = "0.14" }`, and matching `miden-sdk-build-script-support = { version = "0.14" }`
- [ ] `[lib]` in `miden-project.toml` has `kind` (`account-component` / `note` / `tx-script`), `namespace`, **and `path`**
- [ ] `[dependencies]` in `miden-project.toml` carries `miden-core = "*"` and `miden-protocol = "*"`
- [ ] Typed storage uses `StorageValue<T>` / `StorageMap<K, V>` with `get()` / `set()`; slot names derive from `<package>::<namespace-interface>::<field>`
- [ ] Notes/tx-scripts that call a component declare an `#[account(package::Interface)]` wrapper and call methods on the injected `account`
- [ ] Cross-component deps declared in `miden-project.toml` (never `Cargo.toml`) under `[dependencies]`; rely on embedded WIT and the package cache unless source inspection proves an override is required
- [ ] `incr_nonce()` is called only from an authentication procedure; `output_note::create` and the vault operations only from account-component context
- [ ] Felt arithmetic validated before subtraction (see rust-sdk-pitfalls skill)
- [ ] Felt comparisons use `.as_canonical_u64()` (see rust-sdk-pitfalls skill)
