# Migration Guide

This guide explains how to update Rust code and project configuration written against the Miden
SDK (the `miden` crate) and compiler when upgrading between released versions. Each section covers
one release step and shows the concrete *before*/*after* edits a breaking change requires.

The most recent migration is at the top. When cutting a new release, add its migration section
directly below this paragraph, above the previous one (newest first, like the
[CHANGELOG](./CHANGELOG.md)).

<!-- Add the next migration section here, above `## 0.12.0 -> 0.13.0`. -->

## Unreleased

### Kernel scalars are typed instead of `Felt` (counts, block heights, nonces, attachments)

Binding surfaces whose values are counts now return `u32`: `tx::get_num_input_notes`,
`tx::get_num_output_notes`, `active_account::get_num_procedures`, and the
`num_assets` / `num_storage_items` fields of the note info structs. Matching this,
`active_account::get_procedure_root` takes the procedure index as `u32` (previously `u8`), so
count-driven loops index directly. Code that compared or computed with these as felts now works
with plain integers:

```rust
// before
let all_consumed: Felt = tx::get_num_input_notes();
assert_eq!(all_consumed, felt!(2));

// after
let all_consumed: u32 = tx::get_num_input_notes();
assert_eq!(all_consumed, 2);
```

Block heights are wrapped in the new `BlockNumber` type (comparable as integers; heights read
from note storage convert with the validated `BlockNumber::try_from(felt)`), block timestamps
are `u32` seconds, and expiration deltas are `u16`:

```rust
// before
let timelock_height: Felt = inputs[3];
assert!(tx::get_block_number() >= timelock_height);
tx::update_expiration_block_delta(Felt::new(42).unwrap());

// after
let timelock_height = BlockNumber::try_from(inputs[3]).unwrap();
assert!(tx::get_block_number() >= timelock_height);
tx::update_expiration_block_delta(42);
```

Account nonces are wrapped in the new `Nonce` type (comparable as integers; use
`as_felt()`/`as_u64()` or `Felt::from(nonce)` where the raw value is needed, e.g. when packing a
nonce into a `Word` — `ref_block_num` below is a `BlockNumber` from `tx::get_block_number()` and
converts the same way):

```rust
// before
let final_nonce: Felt = self.incr_nonce();
let salt = Word::from([felt!(0), felt!(0), ref_block_num, final_nonce]);

// after
let final_nonce: Nonce = self.incr_nonce();
let salt = Word::from([felt!(0), felt!(0), ref_block_num.into(), final_nonce.into()]);
```

Attachment lookups return `Option<u32>` instead of the removed `AttachmentLocation` struct, and
attachment indexes are passed as `u32`:

```rust
// before
let location = active_note::find_attachment(scheme);
if location.found() {
    let attachment = active_note::write_attachment_to_memory(location.index);
}

// after
if let Some(index) = active_note::find_attachment(scheme) {
    let attachment = active_note::write_attachment_to_memory(index);
}
```

### Fungible asset amounts use `AssetAmount` instead of `Felt`

The fungible-asset bindings no longer expose raw `Felt` amounts. `asset::create_fungible_asset`
and `faucet::create_fungible_asset` take an `AssetAmount`, and `active_account::get_balance` /
`get_initial_balance` return one:

```rust
// before
let asset = faucet::create_fungible_asset(Felt::new(100).unwrap());
let balance: Felt = account.get_balance(asset_key);

// after
let asset = faucet::create_fungible_asset(AssetAmount::new(100).unwrap());
let balance: AssetAmount = account.get_balance(asset_key);
```

`AssetAmount::new` accepts a `u64` up to `AssetAmount::MAX_U64` (`2^63 - 2^31`, the protocol's
maximum fungible amount); values that fit in a `u32` convert infallibly with
`AssetAmount::from(100u32)`. Amounts compare like integers, and `+`/`-` are bounds-checked,
panicking on overflow and underflow instead of wrapping at the field modulus:

```rust
let total = balance + deposit; // panics above MAX_U64 instead of wrapping
let rest = balance - payment;  // panics on underflow
```

For anything beyond that, convert explicitly: `amount.as_u64()` for full integer functionality,
or `amount.as_felt()` to opt back into field arithmetic. A raw fungible `Asset`'s amount is
available as `asset.amount()` (which panics for non-fungible assets) instead of reading
`asset.value[0]` directly.

Component WIT interfaces can import `asset-amount` from `miden:base/core-types@1.0.0` to use
`AssetAmount` in exported method signatures, and typed account storage supports it directly via
`StorageValue<AssetAmount>` and `StorageMap<K, AssetAmount>`.

### `#[account(...)]` generates one trait per component

In 0.13 the `#[account(...)]` macro generated each component's methods as inherent methods on the
wrapper struct. In 0.14 it generates **one trait per referenced interface** (named after the
interface, with the wrapper's visibility) and implements it for the wrapper, so two components that
export the same method name can coexist on one wrapper. Single-component accounts keep calling
`account.method(..)` unchanged **when the generated trait is in scope** — a same-module
`#[note]`/`#[tx_script]` entrypoint sees it automatically, but a call site in a different module
than the wrapper needs `use` of the generated trait (e.g. `use crate::BasicWallet;`).

Because the methods are now on a generated trait, a referenced interface must export at least one
method: `#[account(...)]` now errors if a selected interface has no callable exports, where 0.13
silently generated nothing for it.

**The wrapper struct must be named differently from every generated trait.**
`#[account(counter_contract::CounterContract)] struct CounterContract;` no longer compiles; rename
the struct (e.g. `Counter`):

```rust
#[account(counter_contract::CounterContract)]
struct Counter;

let counter = Counter::new(counter_account_id);
let count = counter.get_count();
```

**Shared method names are disambiguated with UFCS.** When an account derives two components that
export the same method name — or a component method shares a name with an `ActiveAccount` built-in
such as `get_id` — the bare call is ambiguous:

```rust
#[account(basic_wallet::BasicWallet, vault::Vault)]
struct Wallet;

// both BasicWallet and Vault export `deposit`:
<Wallet as BasicWallet>::deposit(account, asset);
<Wallet as Vault>::deposit(account, asset);
```

Generated component traits are same-module, so `<Wallet as Interface>::…` needs no import. The
`ActiveAccount` built-in trait, however, is not in the `miden::*` prelude, so disambiguating a
component method against a built-in needs an explicit import:

```rust
use miden::active_account::ActiveAccount;

// a component method named `get_id` shares the `ActiveAccount::get_id` name:
<Wallet as CounterContract>::get_id(account); // the component method
<Wallet as ActiveAccount>::get_id(account);   // the built-in
```

For the same reason, a component method named `new` is now permitted (it was previously rejected):
it lives on the generated trait and coexists with the inherent `Wallet::new(account_id)`
constructor — `Wallet::new(id)` resolves to the constructor, `wallet.new()` to the component method.

**Name clashes between generated traits are resolved with `as`.** When the generated trait *name*
would clash — the struct shares the interface name, two packages export the same interface name,
two separate `#[account]` wrappers in one module select the same interface, or the crate already
uses the interface as a sibling `#[component(...)]` — rename the generated trait with `as` (the path
still selects the interface):

```rust
// a component that both calls a sibling counter and reaches a remote counter through FPI:
#[account(counter_contract::CounterContract as RemoteCounter)] // FPI trait `RemoteCounter`
struct Remote;

#[component(counter_contract::CounterContract)]                // sibling trait `CounterContract`
trait Caller: NativeAccount + CounterContract { /* ... */ }
```

## 0.13.0 -> 0.13.1

### `*_note::get_metadata` returns a single-word `NoteMetadata`

`get_metadata` no longer includes the note attachment word — it returns only the metadata header,
and `NoteMetadata` is now a single-field struct `{ header: Word }`. Retrieve attachments through the
dedicated attachment procedures instead.

```rust
// before — NoteMetadata { attachment: Word, header: Word }
let meta = active_note::get_metadata();
let attachment = meta.attachment;
let header = meta.header;

// after — NoteMetadata { header: Word }
let meta = active_note::get_metadata();
let header = meta.header;
// attachments are now retrieved separately:
let attachments_commitment = active_note::get_attachments_commitment();
```

## 0.12.0 -> 0.13.0

This release reworks how account components, accounts, and authentication are declared, introduces
a required `miden-project.toml` project manifest, and aligns the tx-kernel bindings with
VM v0.23 / protocol v0.15. The macro changes touch every account component, every authentication
component, and every note/tx-script that references an account. Work through the sections in order:
rewrite the component (1), add the project manifest (2), update account references (3), then the
bindings (4).

### 1. Account and authentication components: `#[component]` is now a trait + a storage struct

`#[component]` no longer applies to a `struct` or an inherent `impl`. An account component is now
three pieces:

1. a `#[component_storage]` struct holding the `#[storage(...)]` fields,
2. a `#[component]` **trait** declaring the API (the trait name yields the WIT interface), and
3. a `#[component] impl Trait for Storage` block providing the behavior.

Method receivers (`&self` / `&mut self`) and method bodies are unchanged.

Before (0.12.0):

```rust
use miden::{component, felt, Felt, StorageMap, Word};

#[component]
struct CounterContract {
    #[storage(description = "counter contract storage map")]
    count_map: StorageMap<Word, Felt>,
}

#[component]
impl CounterContract {
    pub fn get_count(&self) -> Felt {
        let key = Word::new([felt!(0), felt!(0), felt!(0), felt!(1)]);
        self.count_map.get(key)
    }

    pub fn increment_count(&mut self) -> Felt {
        let key = Word::new([felt!(0), felt!(0), felt!(0), felt!(1)]);
        let new_value = self.count_map.get(key) + felt!(1);
        self.count_map.set(key, new_value);
        new_value
    }
}
```

After (0.13.0):

```rust
use miden::{component, component_storage, felt, Felt, StorageMap, Word};

// 1. storage fields move to a `#[component_storage]` struct
#[component_storage]
struct CounterContractStorage {
    #[storage(description = "counter contract storage map")]
    count_map: StorageMap<Word, Felt>,
}

// 2. the API becomes a `#[component]` trait (its name is the WIT interface)
#[component]
trait CounterContract {
    fn get_count(&self) -> Felt;
    fn increment_count(&mut self) -> Felt;
}

// 3. the behavior is a `#[component] impl Trait for Storage` block
#[component]
impl CounterContract for CounterContractStorage {
    fn get_count(&self) -> Felt {
        let key = Word::new([felt!(0), felt!(0), felt!(0), felt!(1)]);
        self.count_map.get(key)
    }

    fn increment_count(&mut self) -> Felt {
        let key = Word::new([felt!(0), felt!(0), felt!(0), felt!(1)]);
        let new_value = self.count_map.get(key) + felt!(1);
        self.count_map.set(key, new_value);
        new_value
    }
}
```

**Authentication components migrate the same way.** `#[auth_script]` was already required in
0.12.0; in 0.13.0 it simply moves onto the trait method declaration (the `impl` method no longer
repeats it):

```rust
// before (0.12.0): inherent impl
#[component]
struct AuthComponent;
#[component]
impl AuthComponent {
    #[auth_script]
    pub fn auth_procedure(&mut self, _arg: Word) { /* ... */ }
}

// after (0.13.0): trait + storage, `#[auth_script]` on the trait method
#[component_storage]
struct AuthComponentStorage;
#[component]
trait AuthComponent {
    #[auth_script]
    fn auth_procedure(&mut self, _arg: Word);
}
#[component]
impl AuthComponent for AuthComponentStorage {
    fn auth_procedure(&mut self, _arg: Word) { /* ... */ }
}
```

### 2. `miden-project.toml` is now a required file

0.13.0 introduces a dedicated project manifest, `miden-project.toml`, placed next to `Cargo.toml`
at the crate root. The Miden-specific configuration that previously lived in `Cargo.toml`
`[package.metadata.*]` now lives here, and the proc-macros read it to resolve the WIT interface
name, the project kind, and any FPI/sibling dependencies. **Building a 0.13.0 project without it
fails** (for components, with an undefined `::init` link error).

Create `miden-project.toml` like this (account component without dependencies):

```toml
[package]
name = "counter-contract"   # crate name; kebab-case
version = "0.1.0"           # project version; supplies the WIT `@version`

[lib]
kind = "account-component"  # project kind: "account-component" | "note" | "tx-script"
# Full WIT id: miden:<package>/<interface>@<version>
#   <package>   = the kebab-cased [package].name
#   <interface> = the kebab-cased `#[component]` trait name  (here: `CounterContract`)
#   <version>   = the [package].version above
namespace = "miden:counter-contract/counter-contract@0.1.0"

[dependencies]
miden-core = "*"
miden-protocol = "*"

# account components only: which account types may host this component
[package.metadata.miden]
supported-types = ["RegularAccountUpdatableCode"]
```

Walking through the fields:

- **`[package]`** — `name` and `version`. The version feeds the `@version` suffix of the WIT id,
  so bumping it changes the component's interface id.
- **`[lib].kind`** — the project kind: `account-component`, `note`, or `tx-script`.
- **`[lib].namespace`** — the full `miden:<package>/<interface>@<version>` WIT id. The
  **interface segment must equal the kebab-cased `#[component]` trait name**; a mismatch fails to
  link with an undefined `::init`. (For `note`/`tx-script` projects, the interface segment is the
  project's own name rather than a component trait.)
- **`[dependencies]`** — the Miden crates the project links against (`miden-core`,
  `miden-protocol`), plus any FPI/sibling dependency packages by `path` (see section 3).
- **`[package.metadata.miden].supported-types`** — account components only.

> **Storage-slot caution.** Storage slot names derive from the `[lib].namespace` interface
> segment (which mirrors the component trait name), and slot names feed `StorageSlotId` derivation.
> Renaming the component trait (and updating `[lib].namespace` to match) **re-keys the storage
> slot ids of an already-deployed component**. Keep the trait name stable across upgrades of a
> live component.

For a project that calls another account/component (FPI or sibling), add the dependency in both
`[dependencies]` (the package) and `[package.metadata.miden.dependencies]` (its generated WIT):

```toml
[dependencies]
miden-core = "*"
miden-protocol = "*"
basic-wallet = { path = "../basic-wallet" }

# the dependency's generated WIT, used to generate the call bindings
[package.metadata.miden.dependencies]
basic-wallet = { wit = "../basic-wallet/target/generated-wit/" }
```

The `[package.metadata.miden.dependencies].<name>.wit` entry is what the macros read to generate
the typed call bindings, and it is the **same entry used by note scripts, by account components
doing FPI, and by sibling component calls**.

### 3. Accounts: declare `#[account(...)]` explicitly with an interface

The auto-generated `crate::bindings::Account` struct is gone. Declare the account explicitly with
`#[account(...)]` and use that type as the note/tx-script entrypoint account parameter. The
dependency reference now **requires the exported WIT interface** (kebab-cased and validated):
write `#[account(basic_wallet::BasicWallet)]`, not `#[account(basic_wallet)]`.

Before (0.12.0):

```rust
use miden::{active_note, note, AccountId, Word};
use crate::bindings::Account; // auto-generated

#[note]
struct P2idNote {
    target_account_id: AccountId,
}

#[note]
impl P2idNote {
    #[note_script]
    pub fn script(self, _arg: Word, account: &mut Account) {
        for asset in active_note::get_assets() {
            account.receive_asset(asset);
        }
    }
}
```

After (0.13.0):

```rust
use miden::{account, active_note, note, AccountId, Word};

// declare the native account explicitly; pick the package's WIT interface
#[account(basic_wallet::BasicWallet)]
pub struct Wallet;

#[note]
struct P2idNote {
    target_account_id: AccountId,
}

#[note]
impl P2idNote {
    #[note_script]
    pub fn script(self, _arg: Word, account: &mut Wallet) {
        for asset in active_note::get_assets() {
            account.receive_asset(asset);
        }
    }
}
```

The same `#[account(...)]` type serves two roles. Passed to a `#[note]`/`#[tx_script]` entrypoint
it is the transaction's native (active) account. Constructed with `new(account_id)` it is a
**foreign account caller**, whose method calls are routed through `execute_foreign_procedure`
(FPI):

```rust
let counter = CounterContract::new(counter_account_id);
let count = counter.get_count();
```

**FPI is not limited to note/tx scripts — an account component can call another account through
FPI too.** Declare the `#[account(...)]` wrapper in the component crate (and the dependency in
`miden-project.toml`, section 2) and use it from inside the `#[component] impl`:

```rust
#[account(callee_account::CounterContract)]
struct CalleeAccount;

#[component]
impl CallerAccount for CallerAccountStorage {
    fn read_foreign_count(&self, callee_account_id: AccountId) -> Felt {
        let callee = CalleeAccount::new(callee_account_id);
        callee.get_count(key)
    }
}
```

### 4. Tx-kernel bindings: protocol v0.15

The SDK bindings were aligned with VM v0.23 / protocol v0.15 (`miden-field` bumped to `^0.25`).

- **`Felt::new` is now fallible** — it returns `Result<Felt, _>` instead of `Felt`. Replace
  `Felt::new(x)` with `Felt::new(x).unwrap()` (or handle the error). The `felt!(x)` macro is
  unchanged and remains the preferred constructor for literals.
- **`asset::{create_fungible_asset, create_non_fungible_asset}`** now take a trailing
  `enable_callbacks: bool` argument.
- **`active_account::{get_balance, get_initial_balance}`** (and the corresponding `ActiveAccount`
  trait methods) now take an asset key `Word` instead of a faucet `AccountId`.
- **`faucet::{mint, burn}`** no longer return an `Asset`; the `faucet::{mint_value, burn_value}`
  helpers were removed. Use the returned value-free API to match the tx kernel.
- **`output_note::set_attachment` was removed.** The attachment shape is selected by function
  instead of a runtime `attachment_kind` argument:

  ```rust
  // before: output_note::set_attachment(note_idx, scheme, kind, attachment);
  // after, for a single word:
  output_note::add_word_attachment(note_idx, scheme, attachment);
  // or `add_attachment` for a commitment, `add_attachment_from_memory` for multiple words.
  ```

> **Storage encoding note.** Scalar `Felt` values stored in `StorageValue<Felt>` /
> `StorageMap<_, Felt>` are now packed into the low word limb (`[v, 0, 0, 0]`) instead of the high
> limb (`[0, 0, 0, v]`), matching protocol v0.15. This is transparent when you recompile and
> redeploy, but state written by 0.12.0 code is read back differently by 0.13.0 code.

### New in 0.13.0 (no migration required)

- **Sibling component calls**: `#[component(package::Interface, ...)]` on the component trait lets
  one component call another component deployed on the same account. See the
  [CHANGELOG](./CHANGELOG.md) for details.
- **`println!`** macro (and `debug::println`) for emitting a debug message during execution.
