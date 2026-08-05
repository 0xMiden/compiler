---
name: rust-sdk-pitfalls
description: Critical pitfalls and safety rules for Miden Rust SDK development. Covers felt arithmetic security, comparison operators, argument limits, storage naming, no-std setup, asset layout, P2ID roots, NoteType construction, note-to-component call boundaries, and note input immutability. Use when reviewing, debugging, or writing Miden contract code.
---

# Miden SDK Pitfalls

## P1: Felt Arithmetic is Modular (SECURITY CRITICAL)

**Severity**: Critical — can cause loss of funds

Felt subtraction wraps around the prime field modulus (p = 2^64 - 2^32 + 1) instead of panicking. Subtracting more than available silently produces a huge positive number.

```rust
// DANGEROUS — no check before subtraction
let new_balance = current_balance - withdraw_amount;
// If withdraw_amount > current_balance, new_balance ≈ 2^64 (wraps!)

// SAFE — always validate first
assert!(
    current_balance.as_canonical_u64() >= withdraw_amount.as_canonical_u64(),
    "Insufficient balance"
);
let new_balance = current_balance - withdraw_amount;
```

**Rule**: ALWAYS check `.as_canonical_u64()` values before any Felt subtraction.

**Max Felt value**: The maximum valid Felt is `p - 1 = 18446744069414584320`, not `u64::MAX` (`18446744073709551615`). Using `u64::MAX` as a sentinel or boundary value causes silent wraparound.

## P2: Felt Comparison Operators Are Misleading for Quantity Logic

**Severity**: High — silently produces incorrect results

`<`, `>`, `<=`, `>=` on Felt values compare field elements, which differs from natural number ordering. In protocol-level code working with field elements, these comparisons may be intentional. For business logic (balances, amounts, counts), the results are misleading.

```rust
// MISLEADING for business logic — compares field elements
if balance > threshold { ... }

// CORRECT for business logic — compare as integers
if balance.as_canonical_u64() > threshold.as_canonical_u64() { ... }
```

**Rule**: For quantity/business logic, ALWAYS convert to `.as_canonical_u64()` before using comparison operators.

## P3: Function Argument Limit (4 Words / 16 Felts)

**Severity**: Medium — causes compilation errors

Functions can receive at most 4 Words (16 Felts) as arguments.

```rust
// PROBLEM — too many arguments
fn process(a: Word, b: Word, c: Word, d: Word, e: Word) { ... } // > 4 Words!

// SOLUTION — pass fat types by reference
fn process(a: &Word, b: &Word, c: &Word, d: &Word, e: &Word) { ... }
```

## P4: Storage API Is Typed

**Severity**: Medium — old examples no longer compile

The old `Value` / untyped `StorageMap` API is gone. Account storage is now:

- `StorageValue<T>` for a single typed slot
- `StorageMap<K, V>` for typed maps
- `get()` / `set()` methods instead of `.read()` / `.write()`
- `K: WordKey`, `T: WordValue`, `V: WordValue`

```rust
#[component]
struct CounterContract {
    #[storage(description = "single typed slot")]
    counter: StorageValue<Felt>,

    #[storage(description = "typed map")]
    balances: StorageMap<AccountId, Felt>,
}
```

If you need custom keys or values, implement `WordKey` / `WordValue` by converting to and from a single `Word`.

## P5: Storage Slot Naming Convention

**Severity**: Medium — causes silent default-value reads in tests

Storage slot names follow a strict pattern. Getting it wrong often returns the default value silently.

**Pattern**: `[component_package_or_name]::[snake_case(component_struct)]::[field_name]`

**Conversion rule**: Replace characters outside `[A-Za-z0-9_]` with `_` in the package or component name. The package comes from `[package.metadata.component] package = "..."`, with any `@version` suffix ignored.

| Package in Cargo.toml | Component Struct | Field | Storage Slot Name |
|----------------------|------------------|-------|-------------------|
| `miden:counter-account` | `CounterContract` | `count_map` | `miden_counter_account::counter_contract::count_map` |
| `miden:bank-account` | `BankAccount` | `balances` | `miden_bank_account::bank_account::balances` |
| `miden:bank-account` | `BankAccount` | `initialized` | `miden_bank_account::bank_account::initialized` |

## P6: No-std Environment

**Severity**: Medium -- causes compilation errors

All contract code must be `#![no_std]`. Forgetting this or using std types causes build failures.

**Required at the top of every contract file:** See any contract in [contracts/](../../../contracts/) for the correct pattern (`#![no_std]` + `#![feature(alloc_error_handler)]`).

**For heap allocation (Vec, String, Box):**
```rust
extern crate alloc;
use alloc::vec::Vec;
```

## P7: Asset ABI Is Two Words, Not One

**Severity**: Medium — old `asset.inner[...]` code is stale

`Asset` is now:

```rust
pub struct Asset {
    pub key: Word,
    pub value: Word,
}
```

```rust
// Reading the amount from a fungible asset
let amount = asset.value[0];

// Persisting or comparing the asset class
let asset_key = asset.key;
```

Do not assume the old single-word asset layout. Use `asset.key` and `asset.value`, or protocol helpers, instead of reconstructing from old `asset.inner[...]` offsets.

## P8: `Recipient::compute` Was Removed

**Severity**: Medium — causes compilation errors after upgrading

Building recipients now goes through the note binding:

```rust
extern crate alloc;
use alloc::vec;

let recipient = note::build_recipient(
    serial_num,
    script_root,
    vec![recipient_id.suffix, recipient_id.prefix],
);
```

## P9: P2ID Note Root Hardcoding

**Severity**: Low-Medium — breaks after miden-standards updates

Creating P2ID output notes requires the MAST root digest of the P2ID script. This is typically hardcoded as a constant.

For any note that is being created within the compiler code, the MAST root digest is needed. Below you find the example of a P2ID note

```rust
fn p2id_note_root() -> Digest {
    Digest::from_word(
        Word::try_from([
            13362761878458161062_u64,
            15090726097241769395_u64,
            444910447169617901_u64,
            3558201871398422326_u64,
        ])
        .unwrap(),
    )
}
```

**Risk**: If miden-standards updates the P2ID script, this digest becomes invalid and withdrawals silently fail.

**Mitigation**: Use `P2idNote::script_root()` from miden-standards if available, or verify the hardcoded root matches the current version after dependency updates.

**NoteType for P2ID**: P2ID output notes created in contract code should use the private note type value via `NoteType::from(felt!(2))` (see P10). Using the public note type triggers an opaque "missing details in advice provider" error at execution time. See [miden-bank withdraw](https://github.com/0xMiden/tutorials/blob/main/examples/miden-bank/contracts/bank-account/src/lib.rs) for the working pattern.

## P10: NoteType Variants Unavailable in Compiler SDK

**Severity**: Medium -- causes compilation errors

Named enum variants (`NoteType::Private`, `NoteType::Public`, `NoteType::Encrypted`) don't exist in contract code. Construct via `NoteType::from()`:

| NoteType | Value |
|----------|-------|
| Public | `NoteType::from(felt!(1))` |
| Private | `NoteType::from(felt!(2))` |
| Encrypted | `NoteType::from(felt!(3))` |

See [miden-bank bank-account](https://github.com/0xMiden/tutorials/blob/main/examples/miden-bank/contracts/bank-account/src/lib.rs) for `NoteType::from(note_type)` usage.

## P11: Note Scripts Cannot Call Native Account Functions

**Severity**: High -- causes runtime failures

Note scripts cannot call `native_account::add_asset()` or other `native_account::` functions directly. The kernel's `authenticate_account_origin` check rejects these calls from a note context. Instead, note scripts must call an account component method, which then calls `native_account::add_asset()` internally.

See [miden-bank deposit-note](https://github.com/0xMiden/tutorials/blob/main/examples/miden-bank/contracts/deposit-note/src/lib.rs) for the correct pattern: the note script calls `bank_account::deposit()`, which internally calls `native_account::add_asset()`.

## P12: Note Inputs Are Immutable After Creation

**Severity**: Low -- causes incorrect architecture

The Felt slice that the `#[note]` macro deserializes into `self` is baked at note creation time and cannot be modified after creation. Design the typed note struct's field set and field order carefully before deployment; any later change is a breaking change for existing notes.
