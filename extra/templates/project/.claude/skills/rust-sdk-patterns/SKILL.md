---
name: rust-sdk-patterns
description: Complete guide to writing Miden smart contracts with the Rust SDK. Covers #[component], #[note], #[tx_script] macros, storage patterns, native functions, asset handling, cross-component calls, P2ID note creation, and asset receiving via component methods. Use when writing, editing, or reviewing Miden Rust contract code.
---

# Miden Rust SDK Patterns

## Three Contract Types

### Account Component (`#[component]`)
Defines reusable logic and storage for accounts. Accounts are composed of one or more components.

See [counter-account/src/lib.rs](../../../contracts/counter-account/src/lib.rs) for a working example demonstrating `#[component]`, typed `StorageMap<Word, Felt>`, `get()`/`set()`, and felt arithmetic.

**Cargo.toml for accounts:** See [counter-account/Cargo.toml](../../../contracts/counter-account/Cargo.toml) for the required `crate-type`, `miden` dependency, `component` metadata, and `project-kind`.

### Note Script (`#[note]`)
Executes when a note is consumed by an account. Can call component methods on the consuming account.

See [increment-note/src/lib.rs](../../../contracts/increment-note/src/lib.rs) for a working example demonstrating `#[note]`, `#[note_script]`, and cross-component calls.

**Cargo.toml for notes:** See [increment-note/Cargo.toml](../../../contracts/increment-note/Cargo.toml) for the required `miden` deps, cross-component dependencies, wit deps, and `project-kind = "note-script"`.

### Transaction Script (`#[tx_script]`)
One-off logic executed in the context of an account. Used for initialization, admin operations, etc.

```rust
#![no_std]
#![feature(alloc_error_handler)]
use miden::*;
use crate::bindings::Account;

#[tx_script]
fn run(_arg: Word, account: &mut Account) {
    account.initialize();
}
```

**Cargo.toml:** Same as account but with `project-kind = "tx-script"`.

## Storage Types

| Type | Usage | Read | Write |
|------|-------|------|-------|
| `StorageValue<T>` | Single typed slot (flags, counters, IDs) | `.get() -> T` | `.set(T) -> T` |
| `StorageMap<K, V>` | Typed key-value mapping (balances, records) | `.get(K) -> V` | `.set(K, V) -> V` |

## Native Function Modules

| Module | Key Functions | Purpose |
|--------|--------------|---------|
| `native_account::` | `add_asset(Asset)`, `remove_asset(Asset)`, `incr_nonce()` | Modify account vault/nonce |
| `active_account::` | `get_id() -> AccountId`, `get_balance(AccountId) -> Felt` | Query current account |
| `active_note::` | `get_assets() -> Vec<Asset>`, `get_sender() -> AccountId` | Query note being consumed (typed note storage arrives as `self` in the `#[note_script]` method; see "Cross-Component Note Pattern" below) |
| `note::` | `build_recipient(Word, Word, Vec<Felt>) -> Recipient` | Build note recipients from serial number, script root, and note storage |
| `output_note::` | `create(Tag, NoteType, Recipient) -> NoteIdx`, `add_asset(Asset, NoteIdx)` | Create output notes |
| `faucet::` | `create_fungible_asset(Felt) -> Asset`, `mint(Asset)`, `burn(Asset)` | Asset minting |
| `tx::` | `get_block_number() -> Felt`, `get_block_timestamp() -> Felt` | Transaction context |
| Intrinsics | `assert(bool)`, `assertz(Felt)`, `assert_eq(Felt, Felt)` | Validation |

## Asset Handling

`Asset` is now a two-word value:

**Constructor**: `Asset::new(word)` creates an Asset from a Word.

See [miden-bank bank-account](https://github.com/0xMiden/tutorials/blob/main/examples/miden-bank/contracts/bank-account/src/lib.rs) for complete asset handling patterns including deposit, withdrawal, and balance tracking.

```rust
pub struct Asset {
    pub key: Word,
    pub value: Word,
}
```

For fungible assets, the amount lives in `asset.value[0]`. The asset class / vault identity lives in `asset.key`.

```rust
// Access fungible amount
let amount = asset.value[0];

// Keep the asset key if you need to persist or compare the asset class
let asset_key = asset.key;

// Add asset to account vault (only from component methods, not note scripts; see pitfall P11)
native_account::add_asset(asset);

// Remove asset from account vault
native_account::remove_asset(asset.clone());
```

## P2ID Output Note Creation

To send assets to another account, create a P2ID (Pay-to-ID) output note. See [miden-bank bank-account](https://github.com/0xMiden/tutorials/blob/main/examples/miden-bank/contracts/bank-account/src/lib.rs) `create_p2id_note()` for a complete working implementation.

## Cross-Component Dependencies

To call another component's methods from a note or tx script, two Cargo.toml sections are needed. See [increment-note/Cargo.toml](../../../contracts/increment-note/Cargo.toml) for a working example showing both `[package.metadata.miden.dependencies]` and `[package.metadata.component.target.dependencies]`.

Then import the bindings in your Rust code. See [increment-note/src/lib.rs](../../../contracts/increment-note/src/lib.rs) line 13 for the import pattern: `use crate::bindings::miden::target_component::target_component;`

## Common Type Conversions

```rust
// Felt from integer
let f = felt!(42);                     // preferred for literals in contract code
let f = Felt::new(42);                 // construct a Felt from a u64
let f = Felt::from_u32(42);
let f = Felt::from_canonical_checked(42).unwrap();

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

## No-std Requirements

Every contract file must start with `#![no_std]` and `#![feature(alloc_error_handler)]`. See any contract in [contracts/](../../../contracts/) for the pattern.

If you need heap allocation (Vec, String, etc.):
```rust
extern crate alloc;
use alloc::vec::Vec;
```

## Cross-Component Note Pattern

A note script reads from `active_note::*` and forwards work to a public account-component method via generated bindings. This is the canonical pattern for any note that updates account state, because note scripts cannot call `native_account::*` directly (see `rust-sdk-pitfalls` skill, P11).

The `#[note]` macro generates `TryFrom<&[Felt]>` for the note struct, so the note's serialized storage is deserialized into typed fields before the script runs. The `#[note_script]` method receives the deserialized note as `self` (by value) and never indexes a raw Felt slice manually. Alongside the required `Word` arg, the method may optionally accept a `&Account` or `&mut Account` parameter. See [compiler/sdk/base-macros/src/lib.rs](https://github.com/0xMiden/compiler/blob/main/sdk/base-macros/src/lib.rs) for the macro contract and [compiler/sdk/base-macros/src/note.rs](https://github.com/0xMiden/compiler/blob/main/sdk/base-macros/src/note.rs) for the generated deserialization (each named field is read via `<T as miden::felt_repr::FromFeltRepr>::from_felt_repr(...)` and EOF is asserted at the end).

Supported field types include `Felt`, the unsigned integer scalars (`u64`, `u32`, `u8`), `bool`, `Option<T>`, and `Vec<T>` via the `FromFeltRepr` trait (`compiler/sdk/field-repr/repr/src/lib.rs`), plus any user type that opts in with `#[derive(FromFeltRepr)]` (this is how `AccountId` supports the macro - see `compiler/sdk/base-sys/src/bindings/types.rs`). Do **not** use `Asset` or `Word` directly as note struct fields; those types do not currently derive `FromFeltRepr`. If you need asset-shaped data inside the note, flatten it into supported scalar fields and reconstruct inside the script, or keep it on the side as a separate `active_note::get_assets()` read.

For Cargo.toml wiring (cross-component dependencies + bindings import), see "Cross-Component Dependencies" above. See [increment-note/src/lib.rs](../../../contracts/increment-note/src/lib.rs) for the project-template's local example of the `#[note] struct + #[note] impl` macro form.

**Storage-free case** (sender + assets, single component call per asset): declare a unit struct (`#[note] struct DepositNote;`). The script reads `active_note::get_sender()` and iterates `active_note::get_assets()`, calling the component method per asset. The macro still generates the deserialization wrapper; for a unit struct it only asserts the storage Felt slice is empty.

**Typed-storage case** (note carries scripted data): declare named fields on the note struct. The macro deserializes them in declaration order, and the script accesses them via `self.<field>`. Illustrative shape:

```rust
#[note]
struct DepositNote {
    depositor: AccountId,
}

#[note]
impl DepositNote {
    #[note_script]
    pub fn run(self, _arg: Word) {
        let assets = active_note::get_assets();
        for asset in assets {
            bank_account::deposit(self.depositor, asset);
        }
    }
}
```

(`use` statements and crate attributes elided; see [increment-note/src/lib.rs](../../../contracts/increment-note/src/lib.rs) for a complete file.) For a verified working example with an `&mut Account` parameter, see [compiler/examples/p2id-note/src/lib.rs](https://github.com/0xMiden/compiler/blob/main/examples/p2id-note/src/lib.rs) (`#[note] struct P2idNote { target_account_id: AccountId }`, where the script asserts `account.get_id() == self.target_account_id` and calls `account.receive_asset(asset)` for each attached asset).

**Component side that absorbs the call**: see [miden-bank bank-account](https://github.com/0xMiden/tutorials/blob/main/examples/miden-bank/contracts/bank-account/src/lib.rs) for `deposit(...)` and `withdraw(...)` in a fuller example. The component method validates (felt-arithmetic safety, see `rust-sdk-pitfalls` P1), updates storage, and (for `withdraw`) creates a P2ID output note via the existing P2ID pattern. Note: miden-bank currently demonstrates an older raw-indexing variant for its withdraw-request note; treat the typed pattern shown above as the preferred shape for new note scripts.

**Test wiring**: tests pass the serialized Felt representation of the note struct's fields through `NoteCreationConfig.storage`, in declaration order. See `rust-sdk-testing-patterns` skill, "Note Construction" section, for the helper that builds a note from a compiled `.masp` package and a populated `NoteCreationConfig`.

## Asset Receiving via Component Methods

Note scripts cannot call `native_account::add_asset()` directly (see pitfall P11). The canonical pattern is for an account component to expose a public method that wraps `native_account::add_asset()`, and note scripts call that method via cross-component bindings.

See [miden-bank bank-account deposit()](https://github.com/0xMiden/tutorials/blob/main/examples/miden-bank/contracts/bank-account/src/lib.rs) for the component side: the `deposit()` method validates the deposit, updates storage, and calls `native_account::add_asset()`.

See [miden-bank deposit-note](https://github.com/0xMiden/tutorials/blob/main/examples/miden-bank/contracts/deposit-note/src/lib.rs) for the note side: the note script calls `bank_account::deposit()` via generated bindings.

## Validation Checklist

- [ ] `#![no_std]` and `#![feature(alloc_error_handler)]` at top of every contract
- [ ] `crate-type = ["cdylib"]` in Cargo.toml
- [ ] Correct `project-kind` in `[package.metadata.miden]`
- [ ] Typed storage uses `StorageValue<T>` / `StorageMap<K, V>` with `get()` / `set()`
- [ ] Cross-component deps in both `[package.metadata.miden.dependencies]` and `[package.metadata.component.target.dependencies]`
- [ ] Felt arithmetic validated before subtraction (see rust-sdk-pitfalls skill)
- [ ] Felt comparisons use `.as_canonical_u64()` (see rust-sdk-pitfalls skill)
