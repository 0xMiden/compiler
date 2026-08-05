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
| Gas metering | No gas (computational bounds exist) |
| Synchronous contract calls | **Asynchronous** communication via notes |
| Accounts are balances + storage | Accounts are **full smart contracts** with code, storage, and vault |

## Core Concepts

### Accounts
Each account is an independent smart contract containing:
- **Code** — Immutable logic compiled from Rust components
- **Storage** — Up to 255 slots exposed in Rust as `StorageValue<T>` or `StorageMap<K, V>`
- **Vault** — Holds fungible and non-fungible assets
- **Nonce** — Incremented with each state change
- **ID** — Unique identifier (prefix + suffix, 2 Felts)

Accounts are composed from **components** — reusable Rust modules annotated with `#[component]`.

### Notes
Notes are **UTXO-like messages** for asynchronous inter-account communication. A note contains:
- **Script** — Logic that executes when the note is consumed
- **Inputs** — Data passed to the script (Vec<Felt>)
- **Assets** — Fungible/non-fungible tokens attached to the note
- **Metadata** — Sender, tag, note type (public/private)

Notes are created as **output notes** by one transaction and consumed as **input notes** by another.

### Transactions
A transaction is a **single-account state transition** with 4 phases:
1. Consume input notes (execute their scripts against the account)
2. Execute transaction script (optional, for one-off logic)
3. Update account state (storage, vault, nonce)
4. Produce output notes (for other accounts to consume later)

**Important**: A two-party transfer (Alice sends Bob tokens) requires TWO transactions:
1. Alice's transaction creates a P2ID note with tokens attached
2. Bob's transaction consumes that note, receiving the tokens

### Assets
- **SDK shape**: `Asset { key: Word, value: Word }`
- **Fungible**: asset amount lives in `asset.value[0]`
- **Non-fungible**: Unique token tied to a faucet account
- Assets live in account **vaults** and move between accounts via notes
- Created by **faucet accounts** using `faucet::create_fungible_asset()` or `faucet::mint()`

### Felt and Word
- **Felt**: Field element in the Goldilocks prime field (p = 2^64 - 2^32 + 1). The fundamental data unit.
- **Word**: Array of 4 Felts (32 bytes). Used for cryptographic hashes, storage keys, account IDs.
- **Current constructors**: `Felt::new`, `Felt::from_u8` / `from_u16` / `from_u32`, `Felt::from_canonical_checked`, `Word::new`, `Word::from([u32; 4])`, `Word::from([Felt; 4])`, `Word::try_from([u64; 4])`
- **Current accessors**: `felt.as_canonical_u64()`, `word.as_elements()`, `word.into_elements()`, `word.as_bytes()`, `word.to_hex()`

**WARNING**: Felt arithmetic is **modular**. Subtraction wraps around the prime. Always validate with `.as_canonical_u64()` before subtracting. See the rust-sdk-pitfalls skill for details.

## Standard Note Patterns

| Pattern | Purpose | How It Works |
|---------|---------|-------------|
| **P2ID** | Send assets to a specific account | Note script checks consumer's ID matches target |
| **P2IDE** | P2ID with expiration | Adds block-height timelock; sender can reclaim after expiry |
| **SWAP** | Atomic asset exchange | Note offers asset A, requests asset B; consumer provides B |

## Development Model

```
Developer writes Rust → Compiler produces MASM → VM executes and proves
```

Three contract types:
- `#[component]` — Account logic and storage (can have multiple per account)
- `#[note]` — Note script (executes when consumed)
- `#[tx_script]` — One-off transaction logic

Contracts are tested locally with **MockChain** (no network needed) and deployed via **miden-client**.

## Key Design Decisions for App Architects

1. **One account per service** — Each bank, vault, or DEX pool is a separate account
2. **Notes for communication** — Use deposit/withdraw/request notes instead of direct calls
3. **Storage for state** — Use `StorageValue<T>` for single slots and `StorageMap<K, V>` for mappings
4. **Privacy by default** — Choose `NoteType::Public` only when discoverability is needed
5. **Components for reuse** — Standard wallet, auth, and faucet components compose into accounts
