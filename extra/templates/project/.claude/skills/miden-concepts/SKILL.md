---
name: miden-concepts
description: Miden architecture and core concepts from a developer perspective. Covers the actor model, accounts, notes, transactions, assets, privacy model, and standard patterns. Use when designing Miden applications or understanding how Miden differs from traditional blockchains.
---

# Miden Architecture for Developers

## What is Miden?

Miden is a zero-knowledge rollup that uses an **actor model** where each account is an independent smart contract. It settles on Ethereum via validity proofs through Agglayer.

Key properties:
- **Privacy by default** — accounts, notes, and transactions are private; the network stores only cryptographic commitments
- **Client-side execution** — transactions are executed and proven locally by the user's device
- **Programmable everything** — accounts hold code and storage; notes carry scripts and assets

## Mental Model Shifts from Traditional Blockchains

| Traditional (Ethereum) | Miden |
|------------------------|-------|
| Transactions involve sender + receiver | Transactions involve **one account only** |
| Public state by default | **Private by default** |
| Validators execute transactions | **Client executes and proves** locally |
| Gas metering | **Fees are paid by the account's own auth procedure**, which funds a public `TX_FEE` note out of the vault — not burned by the kernel. Execution is separately bounded by `MAX_TX_EXECUTION_CYCLES` |
| Synchronous contract calls | **Asynchronous** communication via notes |
| Accounts are balances + storage | Accounts are **full smart contracts** with code, storage, and vault |

## Core Concepts

### Accounts
Each account is an independent smart contract containing:
- **Code** — Logic compiled from Rust components
- **Storage** — Up to 255 slots (`AccountStorage::MAX_NUM_STORAGE_SLOTS`), exposed in guest Rust as `StorageValue<T>` or `StorageMap<K, V>`. Slots are **named, not positional**: each is a `StorageSlotName` paired with its content, kept sorted by name, and a duplicate name is rejected (`AccountError::DuplicateStorageSlotName`). The slot id is derived from the name, which is why a component reading its own named slot is portable across accounts without taking the slot as a parameter
- **Vault** — Holds fungible and non-fungible assets
- **Nonce** — Must increase whenever the account's state changes; an account update records the amount it increased by, not just the fact that it did
- **ID** — Unique identifier (prefix + suffix, 2 Felts). `AccountId` does **not** convert into `[Felt; 2]`; reach the parts with `id.prefix().as_felt()` and `id.suffix()`

Account state changes reach the network as an **`AccountPatch`** (`miden_protocol::account::AccountPatch`), which describes the account's new state. `AccountDelta` still exists and is still *relative* — it records changes rather than final values — and is what a `TransactionSummary` commits to. Don't assume the two are interchangeable: `TransactionSummary::account_delta()` deliberately returns the relative `AccountDelta`.

Accounts are composed from **components** — reusable Rust modules annotated with `#[component]`.

### Notes
Notes are **UTXO-like messages** for asynchronous inter-account communication. A note contains:
- **Script** — Logic that executes when the note is consumed
- **Storage** — Data accessible to the script during execution (`NoteStorage`, backed by `Vec<Felt>`), capped at `MAX_NOTE_STORAGE_ITEMS = 1024`
- **Assets** — Fungible/non-fungible tokens attached to the note, capped at `MAX_ASSETS_PER_NOTE = 16`
- **Metadata** — Sender, tag, note type (public/private). That trio is the *partial* metadata; the full `NoteMetadata` also carries the attachment headers and their commitment
- **Attachments** — Up to `NoteAttachments::MAX_COUNT = 4` attachments, addressed by scheme rather than position. Each holds 1–`NoteAttachment::MAX_NUM_WORDS` (256) words, capped at 512 words per note across all of them — so this is a real payload channel, not a four-word field

Notes are created as **output notes** by one transaction and consumed as **input notes** by another.

### Transactions
A transaction is a **single-account state transition**. The kernel runs four phases:
1. **Prologue** — prepare the root context from the transaction inputs
2. **Note processing** — run every input note's script against the account
3. **Transaction script** — optional one-off logic
4. **Epilogue** — run the account's **authentication procedure** (this is where the fee is paid), then compute and validate the final state

Updating account state and producing output notes are effects of phases 2-3, not phases of their own. The thing people most often leave out of this list is that **authentication and fee payment happen in the epilogue**, after all scripts have run.

**Transaction summaries are six words.** A `TransactionSummary` is what an account's authentication procedure signs, and its commitment preimage is laid out as:

```text
[ACCOUNT_DELTA_COMMITMENT, INPUT_NOTES_COMMITMENT, OUTPUT_NOTES_COMMITMENT,
 BLOCK_COMMITMENT, [expiration_delta, user_param0, user_param1, user_param2],
 [user_param3, user_param4, user_param5, user_param6]]
```

The trailing user parameters give an auth procedure a way to bind extra data (a replay-protection salt, a maximum fee) into the same signature. A component that hashes a shorter layout **compiles and fails at runtime**; the MASM side of this constant is `TX_SUMMARY_NUM_ELEMENTS = 24` in the standard auth library.

**Fees are paid from the authentication procedure.** The auth procedure computes the fee and funds a public `TX_FEE` note out of the account's vault before the summary is created. Callers supply the conversion data with `TransactionRequestBuilder::fee_conversion_info(conversion_info, salt)`; network accounts need a fee policy of their own.

**Important**: A two-party transfer (Alice sends Bob tokens) requires TWO transactions:
1. Alice's transaction creates a P2ID note with tokens attached
2. Bob's transaction consumes that note, receiving the tokens

### Assets

An asset is **two words**: an identifier word and a value word. On the operand stack and in MASM doc comments they appear as `ASSET_ID` followed by `ASSET_VALUE`; the protocol type alias is `Asset = struct { id: word, value: word }`.

- **Fungible**: asset amount lives in `asset.value[0]`
- **Non-fungible**: Unique token tied to a faucet account
- Assets live in account **vaults** and move between accounts via notes
- Minted and destroyed by **faucet accounts** via `faucet::mint(asset)` / `faucet::burn(asset)`, which take an already-built `Asset`. There is no in-transaction asset construction: the kernel exposes no `create_fungible_asset` / `create_non_fungible_asset`.
- A note may carry at most **`MAX_ASSETS_PER_NOTE` = 16** assets.

**`AssetId` and `AssetClass` are different things, and the names are a trap.** `AssetId` is the *unique identifier of an asset in the vault*; its Word layout is `[asset_class_suffix, asset_class_prefix, faucet_id_suffix|reserved|composition, faucet_id_prefix]`, and `AssetId::hash()` produces the `AssetIdHash` used as the vault SMT key. `AssetClass` is the narrower thing that *distinguishes different assets issued by the same faucet* — two felts, and one component of an `AssetId`. Code that treats an `AssetId` as if it were a per-faucet class (or vice versa) type-checks and is wrong.

> **Layer note.** The Rust *contract* SDK (the guest `miden` crate) builds against an earlier protocol snapshot than the client/protocol line, and there the guest type is still `Asset { key: Word, value: Word }` with `asset.value[0]` as the fungible amount. The field is named `key`, not `id`, in guest contract code. Read the layer you are actually writing for rather than renaming across the boundary.

### Felt and Word
- **Felt**: Field element in the Goldilocks prime field (p = 2^64 - 2^32 + 1). The fundamental data unit.
- **Word**: Array of 4 Felts (32 bytes). Used for cryptographic hashes, storage keys, account IDs.
- **Felt constructors** (Rust `miden_field::Felt` — the same type used host-side in clients/tests *and* guest-side inside `#[component]`/`#[note]` contract code, which re-exports it): `Felt::new(u64)` is **fallible** — it returns `Result<Felt, FeltFromIntError>` and rejects values at or above `Felt::ORDER` (it delegates to `from_canonical_checked`), so callers must `?`/match it (guest code typically `Felt::new(0).unwrap()`). `Felt::new_unchecked(u64)` is the raw, non-reducing constructor (any `u64`, no validation) — an out-of-range value yields a non-canonical `Felt`. The field order constant is `Felt::ORDER`; there is no `Felt::MODULUS`. Always-succeed constructors (return a bare `Felt`): `Felt::from_u8` / `from_u16` / `from_u32`. Non-panicking but fallible: `Felt::from_canonical_checked(u64) -> Option<Felt>` (returns `None` when out of range). The JS/React SDK's `Felt` is a *different* type: construct it with `new Felt(42n)`, which takes a `bigint` and throws for non-canonical values, and read it with `.asInt()`.
- **Word constructors**: `Word::new`, `Word::from([u32; 4])`, `Word::from([Felt; 4])`, `Word::try_from([u64; 4])`
- **Current accessors**: `felt.as_canonical_u64()`, `word.as_elements()`, `word.into_elements()`, `word.as_bytes()`, `word.to_hex()`

**WARNING**: Felt arithmetic is **modular**. Subtraction wraps around the prime. Always validate with `.as_canonical_u64()` before subtracting (`.asInt()` in the JS/React SDK). See the rust-sdk-pitfalls skill (or frontend-pitfalls for the JS side) for details.

## Standard Note Patterns

| Pattern | Purpose | How It Works |
|---------|---------|-------------|
| **P2ID** | Send assets to a specific account | Note script checks consumer's ID matches target |
| **P2IDE** | P2ID with expiration | Adds block-height timelock; sender can reclaim after expiry |
| **SWAP** | Atomic asset exchange | Note offers asset A, requests asset B; consumer provides B |
| **PSWAP** | Partial-fill swap | A SWAP that can be consumed for part of the offered amount, leaving a remainder note |

Those are the ones you write by hand. The full `StandardNote` set is larger — it also covers `MINT`, `BURN`, `FEE_SPONSORSHIP`, `TX_FEE`, and the component-configuration notes (`OWNER_CONFIG`, `RBAC_CONFIG`, `PAUSE_CONFIG`, `ALLOWLIST_CONFIG`, `BLOCKLIST_CONFIG`, `NETWORK_ACCOUNT_CONFIG`, `FAUCET_POLICY_CONFIG`, `FAUCET_METADATA_CONFIG`, `CONSTANT_FEE_POLICY_CONFIG`, `MIN_BURN_AMOUNT_CONFIG`).

Standard notes are built with typed builders rather than a `create(..)` constructor: `P2idNote::builder()…build()?`, with fluent `.asset(..)` / `.assets(..)` / `.attachment(..)` / `.attachments(..)`. `MINT` and `BURN` are unified across faucet kinds — one `MintNote` / `BurnNote` rather than per-faucet-kind scripts.

## Standard Components (miden-standards)

| Component | Purpose |
|-----------|---------|
| `BasicWallet` | Standard wallet. Three interface procedures: `receive_asset`, `move_asset_to_note`, `create_note` (roots via `receive_asset_root()`, `move_asset_to_note_root()`, `create_note_root()`) |
| `FungibleFaucet` | Mint/burn fungible tokens (`mint_and_send`, `receive_and_burn`, plus metadata accessors and owner-gated setters); built via `FungibleFaucet::builder()` |
| `NoAuth` | No authentication (for testing) — but it still pays the transaction fee |
| `AuthSingleSig` | Production signature authentication — one component covering both Falcon-512 and ECDSA-K256 key types |

`output_note::create` is account-context only, so a transaction or note script cannot create a note directly — it goes through an account component wrapper such as `BasicWallet::create_note`.

**Auth**: `AuthSingleSig` dispatches on the key type, so one component handles both Falcon-512 and ECDSA-K256 keys. The Falcon-512 scheme uses Poseidon2 as its hash function and is named `Falcon512Poseidon2`. Construct it with `AuthSingleSig::new(approver)` or the typed helpers `falcon512_poseidon2(pk)` / `ecdsa_k256_keccak(pk)` / `from_public_key(pk)`.

Keys are wrapped in `Approver { pub_key, auth_scheme }`, and multi-signature setups use `ApproverSet { approvers, threshold }`. There is no `AccountBuilder::with_auth_component` and no `AuthMethod` or `AuthSingleSigAcl`: auth components are added with `with_component(s)` like any other component.

The auth roster is wider than `NoAuth` + `AuthSingleSig` — `miden_standards::account::auth` also exports `AuthMultisig`, `AuthMultisigSmart`, `AuthGuardedMultisig` (each with a matching `*Config` type), and `AuthNetworkAccount`, which takes its parts directly rather than a config struct.

**Fungible faucet**: `FungibleFaucet` is the fungible-faucet component, constructed with the `bon`-generated `FungibleFaucet::builder()` (required setters `.name(TokenName::new(..)?)`, `.symbol(TokenSymbol::new(..)?)`, `.decimals(n)`, `.max_supply(AssetAmount)`, then `.build()?`).

## Development Model

```
Developer writes Rust → Compiler produces MASM → VM executes and proves
```

Three contract types:
- `#[component]` — Account logic and storage (can have multiple per account)
- `#[note]` — Note script (executes when consumed)
- `#[tx_script]` — One-off transaction logic

Contracts are tested locally with **MockChain** (no network needed) and deployed via the Miden Rust client. That client lives in the **`0xMiden/miden-client`** repository and is published as the crate `miden-client`; the browser client is a separate repository, `0xMiden/web-sdk`.

A component's methods are not implicitly part of the account interface. In Rust, mark each callable method with `#[account_procedure]` on the `#[component]` **trait**; in hand-written component MASM, annotate the exported procedure with `@account_procedure` (or `@auth_script` for an authentication component). An unmarked procedure still compiles and is still exported by the package, but is not reachable as an account procedure.

## Key Design Decisions for App Architects

1. **One account per service** — Each bank, vault, or DEX pool is a separate account
2. **Notes for communication** — Use deposit/withdraw/request notes instead of direct calls
3. **Storage for state** — Use `StorageValue<T>` for single slots and `StorageMap<K, V>` for mappings
4. **Privacy by default** — Choose `NoteType::Public` only when discoverability is needed
5. **Components for reuse** — Standard wallet, auth, and faucet components compose into accounts
