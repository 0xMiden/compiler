---
name: rust-sdk-testing-patterns
description: Guide to testing Miden smart contracts with MockChain. Covers test setup, contract building, account/note creation, transaction execution, storage verification, faucet setup, output note verification, block numbering, multi-transaction tests, and asset-bearing notes. Use when writing, editing, or debugging Miden integration tests.
---

# Miden Testing Patterns (MockChain)

## Test File Setup

Tests go in `integration/tests/`. All tests are async and use MockChain for local execution without a network.

See [counter_test.rs](../../../integration/tests/counter_test.rs) for a complete working test covering imports, MockChain setup, contract building, account creation with storage, note creation, transaction execution, and storage verification.

## Step-by-Step Test Pattern

### 1. Initialize MockChain Builder

See [counter_test.rs](../../../integration/tests/counter_test.rs) line 21 for the pattern: `let mut builder = MockChain::builder();`

### 2. Create Sender/Wallet Accounts

See [counter_test.rs](../../../integration/tests/counter_test.rs) lines 24-26 for the basic wallet pattern. For wallets with pre-funded assets, use `builder.add_existing_wallet_with_assets(Auth::BasicAuth { auth_scheme: AuthSchemeId::Falcon512Poseidon2 }, [FungibleAsset::new(faucet.id(), 100)?.into()])`.

### 3. Set Up Faucets (for fungible assets)
```rust
let faucet = builder.add_existing_basic_faucet(
    Auth::BasicAuth {
        auth_scheme: AuthSchemeId::Falcon512Poseidon2,
    },
    "TOKEN",     // token symbol
    1000,        // max supply
    Some(10),    // total_issuance (None for 0)
)?;
```

### 4. Build Contracts

See [counter_test.rs](../../../integration/tests/counter_test.rs) lines 29-35 for the pattern using `build_project_in_dir`.

### 5. Create Account with Storage

**Storage slot naming convention** (CRITICAL):
```
[component_package_or_name]::[snake_case(component_struct)]::[field_name]
```

Examples:
- Package `miden:counter-account`, component `CounterContract`, field `count_map` -> `miden_counter_account::counter_contract::count_map`
- Package `miden:bank-account`, component `BankAccount`, field `balances` -> `miden_bank_account::bank_account::balances`

Rule: Replace characters outside `[A-Za-z0-9_]` with `_` in the package or component name.

See [counter_test.rs](../../../integration/tests/counter_test.rs) lines 38-54 for the current pattern: populate `InitStorageData`, build the component from the compiled package, then register the account with `builder.add_account_from_builder(...)`.

```rust
let counter_storage_slot = counter_storage_slot()?;
let mut init_storage_data = InitStorageData::default();
init_storage_data.insert_map_entry(counter_storage_slot.clone(), COUNTER_STORAGE_KEY, 0_u64)?;

let counter_component =
    AccountComponent::from_package(&contract_package, &init_storage_data)?;
let counter_account = builder.add_account_from_builder(
    Auth::BasicAuth {
        auth_scheme: AuthSchemeId::Falcon512Poseidon2,
    },
    AccountBuilder::new([3_u8; 32])
        .account_type(AccountType::RegularAccountImmutableCode)
        .storage_mode(AccountStorageMode::Public)
        .with_component(counter_component),
    AccountState::Exists,
)?;
```

For a single-value contract slot (paired with `StorageValue<T>` on-chain) instead of a map:
```rust
let mut init_storage_data = InitStorageData::default();
init_storage_data.insert_value(
    "miden_bank_account::bank_account::initialized",
    0_u64,
)?;
```

### 6. Create Notes

See [counter_test.rs](../../../integration/tests/counter_test.rs) lines 56-64 for basic note creation with `RandomCoin`, `NoteScript::from_package`, and `NoteBuilder`.

For notes with assets and inputs:
```rust
use miden_client::{asset::FungibleAsset, crypto::RandomCoin, note::NoteScript, Felt};
use miden_standards::testing::note::NoteBuilder;

let mut note_rng = RandomCoin::new(NoteScript::from_package(note_package.as_ref())?.root());
let note = NoteBuilder::new(sender.id(), &mut note_rng)
    .package((*note_package).clone())
    .add_assets([FungibleAsset::new(faucet.id(), 50)?.into()])
    .note_storage([Felt::new(42), Felt::new(0)])?
    .build()?;
```

### 7. Add to MockChain and Build

See [counter_test.rs](../../../integration/tests/counter_test.rs) lines 66-70 for seeding the note and building the mock chain. `add_account_from_builder(...)` has already registered the account in the builder, so at this stage you usually only need to add notes.

### 8. Execute Transaction

See [counter_test.rs](../../../integration/tests/counter_test.rs) lines 73-82 for the full execution flow: `build_tx_context` -> `execute()` -> `add_pending_executed_transaction()` -> `prove_next_block()`. The single-transaction counter test does not call `apply_delta()` because `counter_account` is not reused after the build; final state is read from `mock_chain.committed_account(...)` after the block is proven. Multi-transaction tests that keep using the in-memory `Account` variable across steps should call `account.apply_delta(&executed.account_delta())?` after each `execute()` (see "Multi-Transaction Test Pattern" below).

### 9. Execute with Transaction Script
```rust
use miden_client::transaction::TransactionScript;

let tx_script_package = Arc::new(build_project_in_dir(
    Path::new("../contracts/my-tx-script"),
    true,
)?);
let program = tx_script_package.unwrap_program();
let tx_script = TransactionScript::new((*program).clone());

let executed = mock_chain
    .build_tx_context(account.clone(), &[], &[])?
    .tx_script(tx_script)
    .build()?
    .execute()
    .await?;

mock_chain.add_pending_executed_transaction(&executed)?;
mock_chain.prove_next_block()?;

let updated_account = mock_chain.committed_account(account.id())?;
```

### 10. Verify Storage State

See [counter_test.rs](../../../integration/tests/counter_test.rs) lines 84-96 for reading the committed account state and asserting on the result.

### 11. Verify Output Notes

**Important**: `add_output_note()` is only available on `MockChainBuilder` (before `build()`); use it to seed the chain with existing notes. To verify output notes from a transaction, use `extend_expected_output_notes()` on `TxContextBuilder`:

```rust
use miden_client::{note::{Note, NoteAssets, NoteMetadata, NoteRecipient}, transaction::RawOutputNote};

let expected_note = Note::new(expected_assets, expected_metadata, expected_recipient);

let tx_context = mock_chain
    .build_tx_context(account.id(), &[note.id()], &[])?
    .extend_expected_output_notes(vec![RawOutputNote::Full(expected_note)])
    .build()?;

// execute() will verify output notes match
let executed = tx_context.execute().await?;
```

## MockChain Note Interaction

Notes flow through MockChain in four steps:

1. **Build** the note from a compiled `.masp` package (see "Note Construction" below) or via `NoteBuilder`.
2. **Seed** with `MockChainBuilder::add_output_note(RawOutputNote::Full(note.clone()))` BEFORE `builder.build()`. This places the note on the chain so a later transaction can consume it. `add_output_note(...)` is only available on the builder; once `builder.build()` returns the `MockChain`, output notes can only appear as the result of executing a transaction. `RawOutputNote` is re-exported from `miden_client::transaction`.
3. **Consume** by passing the note ID to `mock_chain.build_tx_context(account, &[note.id()], &[])`. The transaction's note-script execution reads the consumed note's storage and assets.
4. **Verify** expected output notes with `.extend_expected_output_notes(vec![RawOutputNote::Full(expected.clone())])` on the `TxContextBuilder`. `tx_context.execute().await?` will assert the produced output notes match.

After `execute()` and before `add_pending_executed_transaction(...) + prove_next_block()`: if a later step will keep using the in-memory `Account` variable (for example, to build another `tx_context` or assert account state directly), call `account.apply_delta(&executed.account_delta())?` to keep the variable in sync with the chain. Post-block reads should use `mock_chain.committed_account(account.id())?` (see Step 8 above and "Multi-Transaction Test Pattern" below). For block advancement and reference-block semantics, see "MockChain Block Numbering" below.

End-to-end multi-note example: see [miden-bank withdraw_test.rs](https://github.com/0xMiden/tutorials/blob/main/examples/miden-bank/integration/tests/withdraw_test.rs) for seeding deposit + withdraw-request notes via `add_output_note(RawOutputNote::Full(...))` before `builder.build()`, then consuming the withdraw-request note and asserting an expected P2ID output note via `extend_expected_output_notes(...)` plus `prove_next_block()`. See [miden-bank deposit_test.rs](https://github.com/0xMiden/tutorials/blob/main/examples/miden-bank/integration/tests/deposit_test.rs) for the simpler single-note consume + prove cycle.

## Multi-Transaction Test Pattern

For contracts requiring initialization before use, each step usually needs its own `execute()` → `add_pending_executed_transaction()` → `prove_next_block()` cycle. Fetch the committed account or note state from `mock_chain` between steps before building the next context.

Call `account.apply_delta(&executed.account_delta())?` after each `execute()` to keep the in-memory `Account` variable in sync with the chain whenever later steps reuse it (for `build_tx_context(...)`, asserting account state directly, etc.). The miden-bank tutorial tests follow this pattern between every `execute()` and `prove_next_block()`. If the test only reads final state via `mock_chain.committed_account(...)` after the last `prove_next_block()` and never reuses the in-memory variable, `apply_delta` is unnecessary; see [counter_test.rs](../../../integration/tests/counter_test.rs).

See [miden-bank withdraw_test.rs](https://github.com/0xMiden/tutorials/blob/main/examples/miden-bank/integration/tests/withdraw_test.rs) for a complete multi-transaction test demonstrating: initialize bank → deposit assets → withdraw assets (3 sequential transactions with state verification between each step).

See [miden-bank deposit_test.rs](https://github.com/0xMiden/tutorials/blob/main/examples/miden-bank/integration/tests/deposit_test.rs) for an end-to-end asset-bearing note test.

## MockChain Block Numbering

Genesis is block 0. Each `prove_next_block()` advances the block number by 1. In contract code, `tx::get_block_number()` returns the **reference block**: the last proven block at the time the transaction started, not the block the transaction will be included in.

## Note Construction

Prefer `NoteBuilder` (or mirror its logic with compiled `.masp` package files) for creating notes in tests. Start from `NoteBuilder::new(sender.id(), &mut note_rng)`, then configure `.package(...)`, optional `.add_assets(...)`, optional `.note_storage(...)`, and finally `.build()`. See [counter_test.rs](../../../integration/tests/counter_test.rs) for the working pattern.

### Building Notes from `.masp` Packages

When a test or binary needs full control over the note (custom storage Felts, deterministic serial number, P2ID-style metadata, or a real-client publish + consume flow), build directly from a compiled `.masp` package. The canonical pipeline is `NoteScript::from_package(package.as_ref())` paired with `NoteBuilder::new(sender_id, &mut RandomCoin::new(note_script.root())).package((*package).clone()).note_type(...).tag(...).add_assets(...).note_storage(...).serial_number(...).build()`. Project-template's own [counter_test.rs](../../../integration/tests/counter_test.rs) follows this same shape with `NoteBuilder` directly.

The miden-bank tutorial codifies this as two helpers built on top of `NoteScript::from_package` + `NoteBuilder`:

- **Real-client path** (`create_note_from_package`): calls `client.rng().draw_word()` and threads it through `NoteBuilder::serial_number(...)` for a fresh per-note serial. Used when the note will be published via a real `TransactionRequestBuilder`. See [miden-bank helpers.rs](https://github.com/0xMiden/tutorials/blob/main/examples/miden-bank/integration/src/helpers.rs) (`create_note_from_package`).
- **Deterministic test path** (`create_testing_note_from_package`): omits `serial_number(...)`, letting `NoteBuilder` derive the serial deterministically from `RandomCoin::new(note_script.root())`. Used when seeding `MockChainBuilder` with a freshly-built note. See [miden-bank helpers.rs](https://github.com/0xMiden/tutorials/blob/main/examples/miden-bank/integration/src/helpers.rs) (`create_testing_note_from_package`).

Both helpers take a `NoteCreationConfig` with four fields: `note_type: NoteType`, `tag: NoteTag`, `assets: NoteAssets`, `storage: Vec<Felt>`. See [miden-bank helpers.rs](https://github.com/0xMiden/tutorials/blob/main/examples/miden-bank/integration/src/helpers.rs) (`NoteCreationConfig` struct + `Default` impl). To drive a cross-component note (see `rust-sdk-patterns` "Cross-Component Note Pattern"), populate `NoteCreationConfig.storage` with the serialized Felt representation of the typed note struct's fields in declaration order; the `#[note]` macro deserializes that slice into `self` before the script runs.

Test-side example: see [miden-bank withdraw_test.rs](https://github.com/0xMiden/tutorials/blob/main/examples/miden-bank/integration/tests/withdraw_test.rs) for a storage vector reaching the note via `NoteCreationConfig { storage, ..Default::default() }` and the seeded `MockChainBuilder.add_output_note(RawOutputNote::Full(...))` call before `builder.build()`.

Binary-side example: see [miden-bank deposit.rs](https://github.com/0xMiden/tutorials/blob/main/examples/miden-bank/integration/src/bin/deposit.rs) for `build_project_in_dir(...)` to produce the `.masp` package, `create_note_from_package(...)` to assemble the note, then `TransactionRequestBuilder::own_output_notes(vec![note.clone()])` and `input_notes([(note.clone(), None)])` to publish and consume. For the surrounding client setup (CLI side), see the `miden-client-cli` skill.

## Asset-Bearing Note Example

To create a note that carries fungible assets in tests:

1. Create a `FungibleAsset` from a faucet ID and amount.
2. Seed a `RandomCoin` from `NoteScript::from_package(note_package.as_ref())?.root()`.
3. Pass the asset into `NoteBuilder::add_assets(...)` and any note inputs into `note_storage(...)`.
4. Finish with `.package((*note_package).clone()).build()?`.

The faucet must be set up first (see Step 3) and the sender wallet must hold sufficient assets (see Step 2).

## Key Dependencies

See [integration/Cargo.toml](../../../integration/Cargo.toml) for the current dependency versions used in this project.

## Validation Checklist

- [ ] Test function is `async` and uses `#[tokio::test]`
- [ ] Storage slot names follow `package_or_name::component_struct::field_name` pattern
- [ ] All contracts built before account/note creation
- [ ] Account storage seeded via `InitStorageData`
- [ ] `prove_next_block()` called after `add_pending_executed_transaction()`
- [ ] Post-block assertions read state from `mock_chain.committed_account(...)` or other committed chain views
- [ ] Notes added to `MockChainBuilder` via `add_output_note(RawOutputNote::Full(...))` before `build()`
- [ ] Faucet set up before creating assets
