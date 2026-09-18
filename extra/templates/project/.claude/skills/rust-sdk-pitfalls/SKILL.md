---
name: rust-sdk-pitfalls
description: Critical pitfalls and safety rules for Miden Rust SDK development. Covers felt arithmetic security, comparison operators, argument limits, storage naming, no-std setup, asset layout, P2ID roots, NoteType construction, note-to-component call boundaries, and note input immutability. Use when reviewing, debugging, or writing Miden contract code.
---

# Miden SDK Pitfalls

Verified against project-template's current v16 line: contract SDK `miden` and
`miden-sdk-build-script-support` `0.14` (published `0.14.0`) on `nightly-2026-09-01`,
`cargo-miden` `0.10.0` through midenup / cargo-miden, `Cargo.lock` resolving
protocol/standards/testing `0.16.0-rc.6`, `miden-client` `0.16.0-rc.2`, and VM/package crates
`0.29.4`.

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

**Prefer `AssetAmount` for token quantities.** `miden::AssetAmount` is a validated newtype over
`Felt` (`AssetAmount::MAX_U64 = (1 << 63) - (1 << 31)`) whose `Add` / `Sub` impls **panic** on
overflow / underflow rather than wrapping, and whose ordering is canonical-integer ordering.
Constructors and accessors: `AssetAmount::new(u64) -> Result<AssetAmount, AssetAmountError>`,
`AssetAmount::max()`, `AssetAmount::ZERO`, `as_u64()`, `as_felt()`. It is a valid `WordKey` and
`WordValue`, so it can be stored directly in `StorageValue` / `StorageMap` and used in exported
signatures.

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

**Exception — typed scalars already compare as integers.** `BlockNumber`, `Nonce` and
`AssetAmount` derive integer ordering, so no conversion is needed. The `p2ide-note` example relies
on this directly:

```rust
let block_number = tx::get_block_number();          // BlockNumber
let timelock_height = BlockNumber::try_from(inputs[3]).unwrap();
assert!(block_number >= timelock_height);           // integer comparison, correct as written
```

## P3: The 16-Felt Cross-Context Boundary — Two Limits, and Exports Differ From FPI Imports

**Severity**: High — the wrong mental model makes you refactor a signature that would have compiled,
and miss the one that will not

A cross-context call passes its parameters on the MASM operand stack, whose addressable window is 16
felts (4 Words, counting the canonical-ABI result pointer when present). Two *different* limits
govern that boundary, and conflating them is the actual pitfall:

- **`MAX_FLAT_PARAMS = 16`** — a **count** of canonical-ABI flat values.
- **`MAX_DIRECT_STACK_FELTS = 16`** — a **felt budget**, measured after `u64` values expand to two
  felts each and any result pointer is added.

Both live in `compiler:sdk/v0.14.0:frontend/wasm/src/component/types/mod.rs:44-62`, whose own
doc comment spells out the distinction: the felt budget "is a Miden VM constraint, distinct from the
spec's count-based `MAX_FLAT_PARAMS`: a signature can stay within 16 flat values while 64-bit values
expand it past 16 stack felts."

Exceeding **either** limit makes canonical-ABI flattening replace the whole parameter list with a
single pointer to a tuple in linear memory — `flat_params_need_tuple` is an **OR**
(`compiler:sdk/v0.14.0:frontend/wasm/src/component/flat.rs:212-217,263-270`):

```rust
flat_params.len() > MAX_FLAT_PARAMS
    || flat_params.iter().map(|param| param.ty.size_in_felts()).sum::<usize>()
        > MAX_DIRECT_STACK_FELTS
```

What happens to that pointer is where the two sides of the boundary part ways.

### FPI imports: the count decides the path, the felt budget can still reject it

The critical subtlety: `plan_fpi_call` does **not** reuse the OR above. It re-derives the call shape
from the flat-value **count alone**
(`compiler:sdk/v0.14.0:frontend/wasm/src/component/lower_imports.rs:330-351`):

```rust
let has_arg_ptr = flattened_params.len() > MAX_FLAT_PARAMS;
```

So the two disagree in exactly one band — **count ≤ 16 but felts > 16**. Flattening *has* tupled
such a signature, but `plan_fpi_call` still believes it is a direct call, so it takes the
`!has_arg_ptr` path, sums the operand felts, and rejects:

```
FPI import `{path}` lowers to {n} operand stack felts after expanding 64-bit
values and result pointers, but direct FPI calls support at most 16
```

The source says this check is deliberately ordered first, because "over-budget direct shapes are
tupled by canonical ABI flattening, which would otherwise surface as a confusing shape mismatch."

```rust
// REJECTED — 13 flat values, so the count-based `has_arg_ptr` is false, but six
// `u64`s expand to two felts each: 6 prefix felts + 12 + 1 + 1 result pointer
// = 20 operand felts.
struct SixU64Record { a: u64, b: u64, c: u64, d: u64, e: u64, f: u64, tag: Felt }
fn echo_six_u64_record(&self, input: SixU64Record) -> SixU64Record;
```

That is the compiler's own negative test,
`compiler:sdk/v0.14.0:tests/integration-network/src/mockchain/fpi/note/six_u64_struct.rs:5-13`
(`#[should_panic(expected = "direct FPI calls support at most 16")]`).

**Above 16 flat values the indirect path is supported** — `has_arg_ptr` is true, the felt-budget
check is skipped entirely, and the wrapper reloads the tuple so the backend still sees a direct,
felt-only call
(`compiler:sdk/v0.14.0:frontend/wasm/src/component/lower_imports.rs:439-508`). A
22-flat-parameter FPI import is a **passing** test
(`compiler:sdk/v0.14.0:tests/integration-network/src/mockchain/fpi/note/sixteen_flattened_params_struct.rs`).

**Indirect is not unbounded, though.** The FPI executor imposes its own caps, checked after the
shape is settled (`compiler:sdk/v0.14.0:frontend/wasm/src/component/lower_imports.rs:381-399`),
with values from `ExecFpi` (`compiler:sdk/v0.14.0:dialects/hir/src/ops/invoke.rs:160-169`):

| bound | value | diagnostic |
| --- | --- | --- |
| `PREFIX_FELTS` — account id + procedure root, subtracted before the input check | 6 | `must pass account id and procedure root` |
| `MAX_INPUT_FELTS` — flattened *procedure input* felts, after the prefix | 16 | ``passes {n} flattened procedure input felts, but `execute_foreign_procedure` supports at most 16`` |
| `EXECUTOR_RESULT_FELTS` — result felts | 16 | ``returns {n} result felts, but `execute_foreign_procedure` supports at most 16`` |

So the tuple pointer buys you past the *stack window*, not past the *protocol*: the payload the
foreign procedure actually receives is still capped at 16 felts.

**Do not "fix" the felt-budget rejection by padding the signature until the count exceeds 16** just
to trigger the indirect path. It does flip `has_arg_ptr` and skip the stack-window check, but the
executor's 16-felt input cap then rejects the same payload — you have only moved which diagnostic
fires. Reduce what crosses the boundary instead: split the call, or hand over an identifier (a
storage key, a note index, a commitment) and let the callee load the rest itself.

### Component exports: indirect parameters are not implemented yet

On the export side the tuple pointer is produced the same way but then refused, so **either** an
over-16 flat-value count **or** an over-16 felt budget fails
(`compiler:sdk/v0.14.0:frontend/wasm/src/component/lift_exports.rs:68-74`):

```
component export lifting for '{path}' is not yet implemented for passing the
parameters using the advice provider in the cross-context `call`;
```

```rust
// REJECTED as a component export — flattens to 20 felt params.
fn process(a: Word, b: Word, c: Word, d: Word, e: Word) { ... }

// STILL REJECTED — a wrapper struct compresses nothing. Flattening recurses
// into struct fields and concatenates them, so a WordBatch holding those same
// five Words is still 20 felts.
fn process(batch: WordBatch) { ... }

// OK — reduce what actually crosses the boundary.
fn process(batch_commitment: Word) { ... }
```

Export **return** values are capped separately, at 16 loaded *values* (a count, with no felt-budget
check at all — a record of nine `u64` fields is 9 values but 18 felts and is not caught):
`compiler:sdk/v0.14.0:frontend/wasm/src/component/lift_exports.rs:281-286`.

### Unrelated, but adjacent

`&T` parameters are refused before any of this, by the `#[component]` macro rather than the
compiler frontend: `references are not supported in component interfaces or exported types`
(`compiler:sdk/v0.14.0:sdk/base-macros/src/types.rs:102-106`). It applies to exported method
parameters, return types, and exported struct/enum fields alike — `&self` receivers are fine.

## P4: Storage API Is Typed, and a Component Is Three Parts

**Severity**: Medium — the wrong component shape does not compile

Account storage uses typed slots:

- `StorageValue<T>` for a single typed slot
- `StorageMap<K, V>` for typed maps
- `get()` / `set()` methods instead of `.read()` / `.write()`
- `K: WordKey`, `T: WordValue`, `V: WordValue`

`WordValue` is implemented for `Word`, `Felt`, `AssetAmount`, `Digest`, `AccountId`, `Recipient`,
`Tag`, `NoteIdx`, `NoteType`. `WordKey` is implemented for `Word`, `Felt`, `AssetAmount`,
`AccountId`, `Tag`, `NoteIdx`, `NoteType`.

An account component is written in **three parts**: annotate the storage struct with `#[component_storage]`, the API `trait` with `#[component]`, and the `impl Trait for Storage` block with `#[component]`. Every trait method that must be callable from outside the component also needs `#[account_procedure]` (see P13):

```rust
// 1. Storage struct — annotated #[component_storage], NOT #[component].
//    Applying #[component] to a struct is a hard compile error.
#[component_storage]
struct CounterContractStorage {
    #[storage(description = "counter contract storage map")]
    count_map: StorageMap<Word, Felt>,
}

// 2. API trait — defines the exported interface.
#[component]
trait CounterContract {
    #[account_procedure]
    fn get_count(&self) -> Felt;
    #[account_procedure]
    fn increment_count(&mut self) -> Felt;
}

// 3. Implementation — the behavior, wired to the storage struct.
//    #[account_procedure] goes on the trait declaration only, never here.
#[component]
impl CounterContract for CounterContractStorage {
    fn get_count(&self) -> Felt {
        let key = Word::new([felt!(0), felt!(0), felt!(0), felt!(1)]);
        self.count_map.get(key)
    }

    fn increment_count(&mut self) -> Felt {
        let key = Word::new([felt!(0), felt!(0), felt!(0), felt!(1)]);
        let current_value: Felt = self.count_map.get(key);
        let new_value = current_value + felt!(1);
        self.count_map.set(key, new_value);
        new_value
    }
}
```

`#[component_storage]` accepts only unit structs and structs with named fields — a tuple struct
fails with `` `#[component_storage]` only supports unit structs or structs with named fields. ``
Generic storage structs are rejected.

If you need custom keys or values, implement `WordKey` / `WordValue` by converting to and from a single `Word`.

## P5: Storage Slot Naming Convention

**Severity**: Medium — causes silent default-value reads in tests

Storage slot names follow a strict pattern. Getting it wrong often returns the default value silently.

**Pattern**: `[package_name]::[namespace_interface_segment]::[field_name]`

**Where the segments come from**: The `#[component_storage]` macro (NOT `#[component]`) processes the `#[storage]` fields and derives slot names. It loads `miden-project.toml` (next to your `Cargo.toml`, NOT `Cargo.toml` itself):

- **First segment** = `[package] name` from `miden-project.toml`.
- **Middle segment** = the *interface segment* of the `[lib] namespace` value. The namespace is a fully-qualified component id `namespace:package/interface@version`; the interface segment sits between the last `/` and the `@`. This is deliberately decoupled from the Rust storage-struct name, so renaming the private struct cannot change deployed slot names. The struct name (`CounterContractStorage`, `AuthComponentStorage`, …) does NOT appear in the slot name.
- **Last segment** = the `#[storage]` field name.

**Conversion rule**: All three segments are character-sanitized — any `@version` suffix is stripped, characters outside `[A-Za-z0-9_]` are replaced with `_`, and an empty or leading-`_` segment is prefixed with `x`. Only the **middle** segment additionally goes through `to_snake_case()`; the package name is *not* snake-cased, it is only character-sanitized (which is why the conventional kebab-case `counter-contract` still lands as `counter_contract`).

| `[package] name` | `[lib] namespace` | Field | Storage Slot Name |
|------------------|-------------------|-------|-------------------|
| `counter-contract` | `miden:counter-contract/counter-contract@0.1.0` | `count_map` | `counter_contract::counter_contract::count_map` |
| `auth-component-rpo-falcon512` | `miden:auth-component-rpo-falcon512/auth-component@0.1.0` | `owner_public_key` | `auth_component_rpo_falcon512::auth_component::owner_public_key` |
| `storage-example` | `miden:storage-example/foo@1.0.0` | `asset_qty_map` | `storage_example::foo::asset_qty_map` |

The first two rows are the exact strings the compiler's own MockChain tests assert against, so use
them as the ground truth for the algorithm.

Omitting the manifest is an error, not a fallback: a `#[component_storage]` struct with `#[storage]`
fields and no `miden-project.toml` fails with `` `#[component_storage]` with `#[storage]` fields
requires a `miden-project.toml` next to the crate's `Cargo.toml`: storage slot names derive from the
`[lib].namespace` interface segment. ``

**Caveat (toolchain-version dependent)**: this naming is a property of the Rust SDK contract macros
in the `miden-base-macros` crate, which ships at published `0.14.0` alongside `miden`, `miden-base`,
`miden-base-sys`, `miden-stdlib-sys` and `miden-sdk-alloc`. The separate compiler / `midenc` /
`cargo-miden` workspace is `0.10.0`. Neither is the protocol/network version: protocol,
`miden-standards` and `miden-testing` resolve to `0.16.0-rc.6`, `miden-client` resolves to
`0.16.0-rc.2`, and the VM/package crates resolve to `0.29.4`. See P20 for the full version matrix.
Verify slot names against your installed toolchain rather than assuming a protocol version implies a
macro behavior.

## P6: No-std Environment

**Severity**: Medium -- causes compilation errors

All contract code must be `#![no_std]`. Forgetting this or using std types causes build failures.

**Required at the top of every contract file:**

```rust
#![no_std]
#![feature(alloc_error_handler)]
```

Both lines appear before any code in the SDK examples — see
`compiler:sdk/v0.14.0:examples/counter-contract/src/lib.rs`,
`compiler:sdk/v0.14.0:examples/basic-wallet/src/lib.rs` and
`compiler:sdk/v0.14.0:examples/p2id-note/src/lib.rs`. Most of them lead with an explanatory
`// Do not link against libstd ...` comment first, so match the two attributes, not the first line.

**For heap allocation (Vec, String, Box):**
```rust
extern crate alloc;
use alloc::vec::Vec;
```

**Toolchain**: current project-template contract crates use `nightly-2026-09-01` with target
`wasm32-wasip2`; the published SDK and `cargo-miden` line require Rust 1.99. Local contract
`Cargo.toml` files use `edition = "2021"`, `crate-type = ["cdylib"]`, `miden = { version = "0.14" }`,
and matching `miden-sdk-build-script-support = { version = "0.14" }`.

## P7: Rust SDK `Asset` Is Two Words (ID + Value)

**Severity**: Medium — reconstructing an asset from raw `asset.inner[...]` offsets is wrong

In the Rust SDK (`miden::Asset` / `miden_base_sys::bindings::Asset`), an `Asset` is encoded as two words:

```rust
pub struct Asset {
    pub key: Word,
    pub value: Word,
}
```

The field is literally named `key`, but the word it holds is the **asset-ID word** at the protocol
layer (see P17). Construct with `Asset::new(key: impl Into<Word>, value: impl Into<Word>)`.

```rust
// Preferred accessors — validated, and integer-ordered
let amount: AssetAmount = asset.amount();   // panics if non-fungible or out of range
let fungible: bool = asset.is_fungible();

// Raw access when you need the words themselves
let raw_amount: Felt = asset.value[0];      // fungible amount lives here
let asset_id_word: Word = asset.key;        // persist or compare the asset class
```

Use `asset.key` / `asset.value` (or the accessors above) rather than reconstructing an asset from raw `asset.inner[...]` offsets.

**SDK vs protocol `Asset`**: the two-word `{key, value}` form is the Rust SDK ABI type. At the
protocol layer, `Asset` is an enum `{ Fungible(FungibleAsset), NonFungible(NonFungibleAsset) }` and
the vault words come from `Asset::to_id_word()` and `Asset::to_value_word()`. There is no
`to_key_word()` — that name does not exist anywhere in the protocol source. Related protocol
accessors: `Asset::id() -> AssetId`, `Asset::from_id_and_value(AssetId, Word)`,
`Asset::from_id_and_value_words(Word, Word)`, `Asset::as_elements() -> [Felt; 8]`.
`FungibleAsset::amount()` returns `AssetAmount`, not `u64`.

## P8: Build Recipients with `note::build_recipient` (no `Recipient::compute`)

**Severity**: Medium — calling a nonexistent `Recipient::compute` fails to compile

Build recipients through the note binding:

```rust
extern crate alloc;
use alloc::vec;

let recipient = note::build_recipient(
    serial_num,
    script_root,
    vec![recipient_id.suffix, recipient_id.prefix],
);
```

`note::build_recipient(serial_num: Word, script_root: Word, storage: Vec<Felt>) -> Recipient` is the
Rust SDK alias for the host function `miden::protocol::note::compute_and_store_recipient`, which
computes and stores the recipient in one step. You can call either name.

**Storage cap**: note storage is limited to 1024 felts (`MAX_NOTE_STORAGE_ITEMS`). Both
`build_recipient` / `compute_and_store_recipient` and `note::compute_storage_commitment` assert on
it and panic with `note storage cannot contain more than 1024 items`.

## P9: P2ID Note Root — Prefer `script_root()`, Do Not Hardcode

**Severity**: Low-Medium — breaks after miden-standards updates

Creating P2ID output notes requires the MAST root of the P2ID script. The root changes whenever the P2ID script or the assembler/hashing changes, so a hardcoded literal is fragile and unverifiable.

**Source of truth**: Use the associated function `P2idNote::script_root() -> NoteScriptRoot` from
`miden-standards` (`NoteScriptRoot` is a `Word` newtype). The same associated function exists on
`P2ideNote`, `SwapNote`, `PswapNote`, `MintNote` and `BurnNote`. From the client, both types are
re-exported as `miden_client::note::P2idNote` and `miden_client::note::NoteScriptRoot`. Derive the
root from the dependency rather than embedding a literal, and re-derive after any dependency bump.

```rust
use miden_standards::note::P2idNote;

// script_root() returns a NoteScriptRoot (a Word newtype); convert to Word when needed.
let p2id_root: Word = P2idNote::script_root().into();
```

**If you must embed a constant** (e.g., inside compiler/contract code that cannot call into miden-standards), regenerate it from the current `miden-standards` version and verify it after every update. The four-limb literal below is ILLUSTRATIVE only — it will not match your build and must not be copied as-is:

```rust
// ILLUSTRATIVE ONLY — will not match your build. Regenerate from
// P2idNote::script_root() for your pinned miden-standards version.
fn p2id_note_root() -> Word {
    Word::try_from([
        13362761878458161062_u64,
        15090726097241769395_u64,
        444910447169617901_u64,
        3558201871398422326_u64,
    ])
    .unwrap()
}
```

**Risk**: If miden-standards updates the P2ID script, any hardcoded digest becomes invalid and withdrawals silently fail.

**NoteType for P2ID**: P2ID output notes created in contract code are constructed with `NoteType::from(felt!(...))` — `felt!(0)` for private, `felt!(1)` for public (see P10). The kernel rejects any other note type with `ERR_NOTE_INVALID_TYPE` ("invalid note type").

## P10: NoteType Variants Unavailable in Compiler SDK

**Severity**: Critical -- wrong values panic at runtime, named variants cause compilation errors

Named enum variants (`NoteType::Private`, `NoteType::Public`) don't exist in contract code — the guest-side SDK `NoteType` is an unvalidated `#[repr(transparent)]` wrapper around `Felt` with `From<Felt>`, `From<NoteType> for Word` and `TryFrom<Word>`, and no named variants. Construct via `NoteType::from()`:

| NoteType | Value |
|----------|-------|
| Private (default) | `NoteType::from(felt!(0))` |
| Public | `NoteType::from(felt!(1))` |

**Note-type encoding**: the note type is 1-bit — `Private = 0` (the protocol default) and `Public = 1`. Only these two values exist; there is no `Encrypted` type. The SDK wrapper does no validation, so an out-of-range value (e.g. `felt!(2)` or `felt!(3)`) is not caught at compile time — the kernel rejects it at execution time with `ERR_NOTE_INVALID_TYPE` (the output-note builder does `u32assert.err=ERR_NOTE_INVALID_TYPE u32lte.NOTE_TYPE_PUBLIC`).

For a working conversion site, see `compiler:sdk/v0.14.0:examples/basic-wallet-tx-script/src/lib.rs`,
which turns a raw input felt into a note type with `note_type.into()` before calling the wallet's
`create_note`.

## P11: Note Scripts Cannot Call Native Account Functions

**Severity**: High -- causes runtime failures

Note scripts cannot call `native_account::add_asset()` or other `native_account::` functions
directly. The kernel's `authenticate_account_origin` check rejects these calls from a note context
(`pub proc account_add_asset` runs `exec.memory::assert_native_account` then
`exec.authenticate_account_origin`; `account_remove_asset` does the same). Instead, note scripts
must call an account component method, which then calls `native_account::add_asset()` internally.

The pattern, split across two pinned examples:

```rust
// Note side — compiler:sdk/v0.14.0:examples/p2id-note/src/lib.rs
#[account(basic_wallet::BasicWallet)]
pub struct Wallet;

#[note]
impl P2idNote {
    #[note_script]
    pub fn script(self, _arg: Word, account: &mut Wallet) {
        for asset in active_note::get_initial_assets() {
            account.receive_asset(asset);   // component method, not a free function
        }
    }
}

// Component side — compiler:sdk/v0.14.0:examples/basic-wallet/src/lib.rs
#[component]
trait BasicWallet {
    #[account_procedure]
    fn receive_asset(&mut self, asset: Asset);
}

#[component]
impl BasicWallet for BasicWalletStorage {
    fn receive_asset(&mut self, asset: Asset) {
        self.add_asset(asset);   // NativeAccount trait method, the idiomatic form
    }
}
```

`self.add_asset(asset)` / `self.remove_asset(asset)` are `NativeAccount` trait methods
auto-implemented on the `#[component_storage]` struct; the free functions
`native_account::add_asset(asset)` / `native_account::remove_asset(asset)` are equivalent.

The alternative to an `#[account(..)]` wrapper is the generated-bindings free-function form, used by
`compiler:sdk/v0.14.0:examples/counter-note/src/lib.rs`:

```rust
use crate::bindings::miden::counter_contract::counter_contract;
counter_contract::increment_count();
```

## P12: Note Inputs Are Immutable After Creation

**Severity**: Low -- causes incorrect architecture

Note inputs (`active_note::get_storage()`) are baked at note creation time and cannot be modified after creation. Design note input layouts carefully before deployment.

A `#[note]` struct **with fields** is auto-decoded from that storage: the macro generates a
`TryFrom<&[Felt]>` that decodes each field via `FromFeltRepr` and then calls `ensure_eof()`, so
extra trailing felts are a decode failure (`FeltReprError::TrailingData`), not ignored padding. A
zero-sized `#[note]` struct skips `get_storage()` entirely. Manual slicing is still available —
`compiler:sdk/v0.14.0:examples/p2ide-note/src/lib.rs` reads `active_note::get_storage()`
directly and asserts `inputs.len() == 4` — but the typed form in
`compiler:sdk/v0.14.0:examples/p2id-note/src/lib.rs` (`#[note] struct P2idNote {
target_account_id: AccountId }`) is the shape to prefer.

## P13: Externally-Callable Methods Must Be Marked `#[account_procedure]`

**Severity**: Critical — omitting it compiles clean and fails only when something calls the method

A `#[component]` trait method is part of the account's **interface** only if it carries
`#[account_procedure]`. Without it the method still compiles and is still exported to WIT, but it is
not an account procedure, so any note script, transaction script, FPI call or sibling-component call
that targets it fails. There is no compile-time warning.

```rust
#[component]
trait Bank {
    #[account_procedure]
    fn initialize(&mut self);
    #[account_procedure]
    fn deposit(&mut self, depositor: AccountId, deposit_asset: Asset);
}
```

Rules:

- Placement is on the **trait declaration**, never on the `impl` block. The `impl` methods stay bare.
- No import is needed — the enclosing `#[component]` macro recognises the attribute. Applying it
  outside a `#[component]` trait errors with `` `#[account_procedure]` must be applied to a method
  inside a `#[component]` `trait` ``, and it takes no arguments.
- `#[account_procedure]` and `#[auth_script]` are **mutually exclusive within one component**:
  `a component cannot combine #[auth_script] and #[account_procedure]`.
- Inherent (`impl BankStorage`) methods are not exported at all, and "exported to WIT" is not the
  same thing as "is an account procedure".
- The `cargo miden new` scaffolding under `compiler:sdk/v0.14.0:extra/templates/` omits
  `#[account_procedure]`, so freshly-generated code is wrong out of the box. Use
  `compiler:sdk/v0.14.0:examples/counter-contract/src/lib.rs` and
  `compiler:sdk/v0.14.0:examples/basic-wallet/src/lib.rs` as the reference instead.

**MASM equivalent**: a hand-written or standards MASM component marks its exports with the
`@account_procedure` / `@auth_script` attributes — "a procedure is part of the component interface
if it has either the `@account_procedure` or `@auth_script` attributes". See
`protocol:v0.16.0-rc.6:crates/miden-standards/asm/standards/wallets/basic.masm`.

## P14: Some Kernel Calls Are Legal Only in a Specific Runtime Context

**Severity**: Critical — these compile anywhere and panic at execution time

Three restrictions enforced by the kernel, not the type system:

| Call | Allowed only from | Kernel enforcement |
|---|---|---|
| `output_note::create(tag, note_type, recipient)` | an account-component procedure | `exec.authenticate_account_origin` + `exec.memory::assert_native_account` |
| `native_account::{add_asset, remove_asset}` | an account-component procedure | `exec.memory::assert_native_account` + `exec.authenticate_account_origin` |
| `native_account::incr_nonce()` / `self.incr_nonce()` | the account's `#[auth_script]` authentication procedure | `exec.memory::assert_native_account` + `exec.assert_auth_procedure_origin` |

All three are in `protocol:v0.16.0-rc.6:crates/miden-protocol/asm/kernels/transaction/lib/api.masm`
(`pub proc output_note_create`, `pub proc account_add_asset`, `pub proc account_incr_nonce`).

Consequences:

- A transaction script or note script that calls `output_note::create` directly compiles and then
  fails at execution. Route it through an account-component method — `basic-wallet` exposes
  `#[account_procedure] fn create_note(&mut self, tag: Tag, note_type: NoteType, recipient:
  Recipient) -> NoteIdx` for exactly this reason.
- Calling `incr_nonce()` from an ordinary component method panics. Only the authentication
  component's single `#[auth_script]` method may do it — see
  `compiler:sdk/v0.14.0:examples/auth-component-no-auth/src/lib.rs`.

**Auth components**: exactly one `#[auth_script]` method per `#[component]` trait, and a crate whose
`miden-project.toml` sets `[package.metadata.miden] project-kind = "authentication-component"` must
have exactly one (`authentication components require exactly one #[auth_script] method`);
`#[auth_script]` on a non-account-component target is rejected outright.

## P15: Bindings That No Longer Exist

**Severity**: High — a contract carried forward from an earlier SDK will not compile, or will compile against the wrong name

| Gone | Use instead |
|---|---|
| `active_note::get_assets()` | `active_note::get_initial_assets() -> Vec<Asset>` |
| `input_note::get_assets(idx)` | `input_note::get_initial_assets(idx)` |
| `input_note::get_assets_info(idx)` | `input_note::get_initial_assets_info(idx)` |
| `active_account::get_balance` / `get_initial_balance` | `active_account::get_asset(asset_key: Word) -> Word` (or `native_account::get_initial_asset(asset_key: Word) -> Word`), then read the amount out of the value word |
| `active_account::has_non_fungible_asset(asset)` | `active_account::has_asset(asset_id: Word) -> bool` |
| `faucet::create_fungible_asset` / `create_non_fungible_asset` / `has_callbacks`, and the whole `asset` module | build the `Asset` outside the transaction; only `faucet::mint(Asset)` and `faucet::burn(Asset)` remain |
| `AttachmentLocation` | `Option<u32>` from `find_attachment` |
| `output_note::set_attachment` | append with `output_note::add_word_attachment`, `output_note::add_attachment`, or `output_note::add_attachment_from_memory` |

The output-note attachment APIs append attachment entries; they do not replace an existing attachment in place.

The current `active_account` surface is `get_id() -> AccountId`, `get_nonce() -> Nonce`,
`compute_commitment() -> Word`, `get_code_commitment() -> Word`, `compute_storage_commitment() ->
Word`, `get_asset(Word) -> Word`, `has_asset(Word) -> bool`, `get_vault_root() -> Word`,
`get_num_procedures() -> u32`, `get_procedure_root(u32) -> Word`, `has_procedure(Word) -> bool` —
all also available on the `ActiveAccount` trait.

Initial-state getters live on `native_account` as free functions: `get_initial_commitment()`,
`get_initial_storage_commitment()`, `get_initial_vault_root()`, `get_initial_asset(Word) -> Word`,
plus `compute_delta_commitment()` and `was_procedure_called(Word) -> bool`.

## P16: Kernel Scalars Are Typed, Not `Felt`

**Severity**: Medium — arithmetic and `Word` packing that assumed `Felt` no longer type-checks

| Binding | Return type |
|---|---|
| `tx::get_block_number()` | `BlockNumber` |
| `tx::get_block_timestamp()` | `u32` (seconds) |
| `tx::get_num_input_notes()` / `get_num_output_notes()` | `u32` |
| `tx::get_expiration_block_delta()` | `u16` (and `update_expiration_block_delta(delta: u16)`) |
| `active_account::get_num_procedures()` | `u32` (and `get_procedure_root(index: u32)`) |
| `active_account::get_nonce()`, `native_account::incr_nonce()` | `Nonce` |
| `active_note::find_attachment(..)`, `output_note::find_attachment(..)` | `Option<u32>` |

`BlockNumber` offers `try_from(Felt)`, `as_u32()`, `as_felt()`, `From<u32>` and integer comparison;
`as_u32()` **panics** rather than truncating if the value exceeds the u32 block-height range.
`Nonce` offers `as_u64()`, `as_felt()` and `From<Nonce> for Felt`.

Packing them back into a `Word` needs an explicit conversion:

```rust
let ref_block_num = tx::get_block_number();
let final_nonce = self.incr_nonce();
let w = Word::from([felt!(0), felt!(0), ref_block_num.into(), final_nonce.into()]);
```

## P17: `AssetId` at the Protocol Layer Is the Vault Key, Not the Asset Class

**Severity**: High — a naive find-and-replace compiles and is silently wrong

At the protocol layer the vault key type is `AssetId`:

```rust
pub struct AssetId {
    asset_class: AssetClass,      // {suffix, prefix}; both zero for fungible assets
    faucet_id: AccountId,
    composition: AssetComposition,
}
```

Word layout: `[asset_class_suffix, asset_class_prefix, [faucet_id_suffix | reserved | composition],
faucet_id_prefix]`. The actual SMT key is `AssetId::hash() -> AssetIdHash`.

`AssetClass` is a *component of* `AssetId` — it distinguishes assets issued by the same
faucet — not the asset id itself. Treating `AssetId` as the per-faucet class compiles and is
silently wrong. The vault-key accessors are `Asset::id()` and `Asset::to_id_word()`, and the client
re-exports `AssetId` (not `AssetClass`) from `miden_client::asset`.

There is **no `AssetVaultKey` type** in either the protocol or the client — searching for one is a
dead end, and a type of that name in your code or in generated bindings is stale. The vault-key type
is `AssetId`, declared at
`protocol:v0.16.0-rc.6:crates/miden-protocol/src/asset/vault/asset_id.rs:42` and re-exported by the
client at `miden-client:v0.16.0-rc.2:crates/rust-client/src/lib.rs:195`.

On the guest side nothing renamed: `miden::Asset` still has a field literally named `key`, and that
word is the asset-ID word (P7).

## P18: `MAX_ASSETS_PER_NOTE` Is 16

**Severity**: Medium — a loop or note builder sized for a larger bound fails

`pub const MAX_ASSETS_PER_NOTE: usize = 16;` (mirrored by `NoteAssets::MAX_NUM_ASSETS` and by the
kernel's `constants.masm`). Any code that assumed 64 assets per note — fixed-size buffers, batching
logic, test fixtures — needs resizing.

## P19: A Transaction Summary Is Six Words (24 Felts)

**Severity**: High — an auth procedure hashing a four-word layout compiles and fails at runtime

`TransactionSummary::NUM_ELEMENTS` covers six words. The standards MASM matches with
`const TX_SUMMARY_NUM_ELEMENTS = 24` and six word-sized locals
(`SUMMARY_ACCOUNT_DELTA_LOC = 0`, `SUMMARY_INPUT_NOTES_LOC = 4`, `SUMMARY_OUTPUT_NOTES_LOC = 8`,
`SUMMARY_BLOCK_COMMITMENT_LOC = 12`, `SUMMARY_PARAMS_HEAD_LOC = 16`,
`SUMMARY_PARAMS_TAIL_LOC = 20`), and
`pub proc create_tx_summary(user_params: [felt; 7]) -> (word, word, word, word, word, word)`.

Preimage order:

```text
[ACCOUNT_DELTA_COMMITMENT, INPUT_NOTES_COMMITMENT, OUTPUT_NOTES_COMMITMENT,
 BLOCK_COMMITMENT, [expiration_delta, user_param0..2], [user_param3..6]]
```

Sources: `protocol:v0.16.0-rc.6:crates/miden-protocol/src/transaction/tx_summary.rs` and
`protocol:v0.16.0-rc.6:crates/miden-standards/asm/standards/auth/mod.masm`.

## P20: Match the Project Version Line and Keep Build Tools Separate

**Severity**: High - mixing lower bounds, resolved lockfile versions, and build tools creates false migrations

The current project-template uses published final contract SDK crates and release-candidate host crates. Copy the local manifests and lockfile before changing versions:

```toml
# contracts/<name>/Cargo.toml
miden = { version = "0.14" }
miden-sdk-build-script-support = { version = "0.14" }

# integration/Cargo.toml
miden-client = { version = "0.16.0-rc.1", features = ["tonic"] }
miden-client-sqlite-store = { version = "0.16.0-rc.1", package = "miden-client-sqlite-store" }
miden-standards = { version = "0.16.0-rc.4", features = ["testing"] }
miden-testing = "0.16.0-rc.4"
miden-mast-package = { version = "0.29", default-features = false }
```

`Cargo.lock` currently resolves `miden-client` / `miden-client-sqlite-store` to `0.16.0-rc.2`, protocol/standards/testing to `0.16.0-rc.6`, and VM/package crates such as `miden-mast-package` to `0.29.4`. Treat the manifest requirements as lower bounds and the lockfile as the exact local graph.

**MSRV split**: use the highest applicable toolchain. The published SDK and `cargo-miden` line require Rust 1.99, and this project pins `nightly-2026-09-01` with target `wasm32-wasip2` for contract builds.

**Build tool separation**: `cargo-miden` is an out-of-process build tool, not an `integration` library dependency. The test helper calls `miden` / `cargo miden build` through `build_project_in_dir(...)`, then tests the produced `.masp` packages with the client, standards, and `miden-testing` dependencies.

## P21: `miden-project.toml` Requires `[lib] path`

**Severity**: Medium — a missing key is a parse error, an unknown key is also a parse error

`[lib]` and every `[[bin]]` target carry a **mandatory** `path` (`Span<Uri>`, no `serde(default)`),
and both target structs are `deny_unknown_fields` — every other key (`kind`, `namespace`, `name`) is
optional. `kind` exists on `[lib]` only; `[[bin]]` targets are always executables. Accepted `kind`
spellings: `lib` / `library`, `kernel`, `account` / `account-component`, `note`, `tx-script` /
`transaction-script`; an executable kind on `[lib]` errors with `this is not a valid target type for
a library`.

Full account-component manifest, matching
`compiler:sdk/v0.14.0:examples/counter-contract/miden-project.toml`:

```toml
[package]
name = "counter-contract"
version = "0.1.0"

[lib]
kind = "account-component"
namespace = "miden:counter-contract/counter-contract@0.1.0"
path = "src/lib.rs"          # mandatory

[dependencies]
miden-core = "*"
miden-protocol = "*"

[package.metadata.miden]
supported-types = ["RegularAccountUpdatableCode"]
```

`[package.metadata]` is a free-form bag as far as the `miden-project` parser is concerned, but the
SDK macros read `[package.metadata.miden] project-kind` out of it and treat
`project-kind = "authentication-component"` as the switch that requires exactly one `#[auth_script]`
method (P14). Other observed values of `supported-types`: `"RegularAccountImmutableCode"`, and
`["FungibleFaucet", "NonFungibleFaucet"]` for faucets.

Cross-component dependencies go in `miden-project.toml`'s `[dependencies]` - never in `Cargo.toml`,
which the macros read only for `[package] name` / `description`. Embedded component WIT is
authoritative: a `wit` override is rejected when the package embeds WIT, and is only a fallback
for packages without embedded WIT.

For plain Cargo checks, builds, and IDE analysis with source dependencies, the **consuming crate**
must have `miden-sdk-build-script-support = { version = "0.14" }` under `[build-dependencies]` and
call `miden_sdk_build_script_support::prepare_package_cache()` from its own `build.rs`. This
prepares `MIDENC_PACKAGE_CACHE` for that crate's macro expansion; the dependency crate's hook
does not configure its consumers. `cargo miden build`, a direct `.masp` dependency, or an already
valid package cache bypasses that requirement.

## P22: MASM-Side Facts That Bite Rust SDK Developers

**Severity**: Medium — relevant when hand-writing component MASM, or reading the standards / kernel MASM

- **An undeclared `.masm` file is silently dropped.** A file is compiled only if its parent module
  declares it with `mod` / `pub mod`. Module discovery is seeded exclusively from the root module's
  `submodules()` and extended from each child's — there is no directory walk, so an undeclared file
  is never opened. The *error* direction is a declaration with no matching `<name>.masm` or
  `<name>/mod.masm` (`ParsingError::UndefinedSubmodule`; both present is
  `AmbiguousSubmoduleLocation`), and a module reaching the linker without a parent declaration is
  `LinkerError::UndeclaredSubmodule`.
- **Import syntax**: `use x -> y` is rejected — the parser says *import aliases use `as`; `->` is no
  longer supported*. Item imports are `use {a, b} from path::to::module`, with per-item `as` aliases.
  `pub use` is legal only for braced item imports — write `pub use {c} from a::b`, not
  `pub use a::b::c`. Modules cannot be re-exported at all (use `pub mod`); wildcard and digest
  imports are rejected.
- **`debug.*` decorators are gone.** `debug.stack.4`, `debug.mem`, `debug.local.0.2` and
  `debug.adv_stack.4` are rejected by the parser. The replacement is the `miden::core::debug`
  module (`miden-vm:v0.29.4:crates/lib/core/asm/debug.masm`), exporting `print_stack`, `print_mem`,
  `print_mem_addr`, `print_mem_all`, `print_adv_stack`, `print_adv_stack_all`, `print_adv_map_all`,
  `print_adv_map_item`. These are **ordinary procedure calls that print unconditionally**,
  regardless of debug mode, and the ones taking stack inputs consume them — strip them from
  production code. The advice-stack / advice-map printers additionally need host handlers
  registered.
- **`.masl` is gone.** The artefact is a `Package` with extension `.masp` (magic `b"MASP\0"`);
  `Library` and `KernelLibrary` were deleted. `Assembler::link_package(Arc<Package>, Linkage)` and
  `Assembler::with_package(..)` are the linking entry points, kernels come in via
  `Assembler::with_kernel(source_manager, Arc<Package>)`, and `assemble_library` /
  `assemble_kernel` / `assemble_program` all return `Box<Package>`.
