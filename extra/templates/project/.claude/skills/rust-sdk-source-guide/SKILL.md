---
name: rust-sdk-source-guide
description: Guide for advanced Miden smart contract development using source repo exploration. Covers AI development practices (Plan Mode, verification-driven development, context engineering, sub-agents) and maps Miden source repositories for discovering advanced patterns. Use when building complex multi-contract applications, novel note flows, or anything beyond basic SDK patterns.
---

# Advanced Miden Development: Source-Guided Context Engineering

## Development Approach

### 1. Plan Mode First

For any non-trivial smart contract application, start in Plan Mode before writing code.

- Explore Miden source repos to understand existing patterns
- Design the account/note architecture and present it to the user before implementing
- Identify which standard components can be reused vs what needs to be custom
- Map out the note flow: which accounts exist, what notes flow between them, what storage each needs

Rule of thumb: if the task involves more than one contract or a pattern not covered by the basic skills, plan first.

### 2. Verification-Driven Development

This is the single highest-leverage practice for AI-assisted Miden development.

**Build loop**: After every contract edit, run `cargo miden build --manifest-path contracts/<name>/Cargo.toml --release` from the project root (adjust the path to your contract layout). With the midenup `miden` wrapper, run `miden build --release` from the contract directory instead: `miden build --manifest-path ...` alone does not select that contract from the workspace root. The output is a `.masp` package written under `<target_dir>/<profile>/`. If your project has a build hook configured, it may run this automatically. If the build fails:
1. Read the error message
2. Translate obvious SDK/compiler errors first:
   - `.as_u64()` -> `.as_canonical_u64()`
   - `Recipient::compute(...)` -> `note::build_recipient(...)`
   - `Value` -> `StorageValue<T>`
   - `StorageMap` -> `StorageMap<K, V>`
   - `active_note::get_assets()` -> `active_note::get_initial_assets()`
   - a trait method that a note or tx script cannot reach -> it is missing `#[account_procedure]`
3. Search the source repos for a working example of the pattern that failed
4. Adapt the working pattern to your use case
5. Rebuild

**Test loop**: Write tests alongside contracts. Run full repo checks with `cargo test` — or `make test` if the repo ships a GNU `Makefile`. In the protocol repo the `test` target runs `cargo nextest run --profile default --cargo-profile test-dev --features concurrent,testing,std` (siblings: `test-dev`, `test-release` / `testr`, `test-docs`, `lint`, `clippy`, `format`, `doc`). `cargo make test` needs a `Makefile.toml` configuring `cargo-make`; among the referenced repos only the compiler ships one — protocol, the client and the VM each ship a GNU `Makefile` and no `Makefile.toml`, while the compiler ships a `Makefile.toml` and no GNU `Makefile`. When tests fail:
1. Check the error — is it a build error, a runtime assertion, or a proof failure?
2. For assertion failures: check felt arithmetic (modular wrapping), storage slot naming, and whether the call is legal in its runtime context (`output_note::create` is account-context-only; `incr_nonce()` is auth-procedure-only)
3. For unexpected behavior: compare your code against the closest working example in source repos

Never submit code that doesn't compile and pass tests. The verification loop is your quality guarantee.

### 3. Context Engineering with Source Repos

The basic skills (rust-sdk-patterns, rust-sdk-testing-patterns, miden-concepts, rust-sdk-pitfalls) cover standard patterns. For anything beyond those patterns, Miden's source repositories are the knowledge base.

**How to use source repos effectively**:
- Don't load entire repos into context. Use sub-agents to explore — they search, read relevant files, and summarize findings without filling the main conversation context.
- Read source files only when you need a specific answer (progressive disclosure)
- Look for working examples first, then adapt. Working code that compiles is more reliable than documentation.
- When you find a useful pattern in source, extract just what you need — the exact API call, the exact data layout, the exact test setup.
- Start API questions at `compiler/sdk/sdk/MIGRATION.md`. Its `## Unreleased` section is the authoritative, hand-written list of what changed on the Rust contract surface, with before/after code for each break. `compiler/sdk/CHANGELOG.md` is the companion. For an exact signature, go to `compiler/sdk/base-sys/src/bindings/*.rs`.

**Using sub-agents for exploration**:
- Launch an explore sub-agent with a specific question: "Find how the basic-wallet component creates an output note and moves an asset into it (`compiler/examples/basic-wallet/src/lib.rs`)"
- The sub-agent searches, reads the relevant files, and returns a focused summary
- Your main context stays clean for implementation

### 4. Iterative Multi-Stage Development

Break complex applications into stages. Complete each before starting the next:

1. **Design** (Plan Mode) — Architecture, note flows, storage design
2. **Implement accounts** — Component structs, storage, methods
3. **Implement notes** — Note scripts, cross-component calls, input parsing
4. **Implement tx scripts** — Initialization, admin operations
5. **Write tests** — MockChain setup, multi-step execution, state verification
6. **Integrate** — Connect pieces, end-to-end test

When stuck at any stage: search the source repos for a similar working pattern. Adapt it, don't guess.

---

## Which Version Is Which

The three-way version skew is the most confusing thing about this stack. These are four independent
release lines:

| Line | Crates | Version | MSRV |
|---|---|---|---|
| Contract SDK (guest) | `miden`, `miden-base`, `miden-base-macros`, `miden-base-sys`, `miden-stdlib-sys`, `miden-sdk-alloc` | project manifests use `0.14`, resolving to published `0.14.0` | Rust 1.99 + nightly `2026-09-01`, target `wasm32-wasip2` |
| Compiler / build tool | compiler workspace, `midenc`, `cargo-miden` | `cargo-miden` `0.10.0`, usually installed through midenup / cargo-miden | Rust 1.99 |
| Protocol | `miden-protocol`, `miden-standards`, `miden-testing`, `miden-tx`, `miden-tx-batch`, `miden-block-prover`, `miden-agglayer` | integration manifests lower-bound standards/testing at `0.16.0-rc.4`; `Cargo.lock` resolves protocol/standards/testing to `0.16.0-rc.6` | see each crate |
| Client | `miden-client`, `miden-client-sqlite-store` | integration manifests lower-bound at `0.16.0-rc.1`; `Cargo.lock` resolves both to `0.16.0-rc.2` | see each crate |
| VM / package crates | `miden-assembly`, `miden-assembly-syntax`, `miden-core`, `miden-core-lib`, `miden-crypto`, `miden-mast-package`, `miden-processor`, `miden-project`, `miden-prover` | `Cargo.lock` resolves package crates such as `miden-mast-package` to `0.29.4` | see each crate |

For published final crates, follow the local manifests: contract crates use `miden = { version = "0.14" }` and matching `miden-sdk-build-script-support = { version = "0.14" }`. Full pre-release strings matter for release-candidate dependencies such as `miden-client = { version = "0.16.0-rc.1" }`, `miden-standards = { version = "0.16.0-rc.4" }`, and `miden-testing = "0.16.0-rc.4"`; read both `integration/Cargo.toml` and `Cargo.lock` before changing version requirements.

Keep the build tool out of the integration crate's dependency graph. The current project builds contracts out of process with midenup / cargo-miden via `build_project_in_dir(...)`, then tests those packages with the client, standards, and `miden-testing` libraries resolved in `Cargo.lock`.

---

## Miden Source Repository Map

Clone these repos alongside your project for reference. Claude will explore them when needed for advanced patterns.

```bash
# Required: protocol layer, standard note types and account components (crate: miden-protocol)
git clone --branch v0.16.0-rc.6 https://github.com/0xMiden/protocol.git ../protocol

# Required: client API for deployment and chain interaction (crate: miden-client)
git clone --branch v0.16.0-rc.2 https://github.com/0xMiden/miden-client.git ../miden-client

# Required: the Rust SDK macros and compiler; the sdk tag names the guest SDK version.
git clone --branch sdk/v0.14.0 https://github.com/0xMiden/compiler.git ../compiler

# Optional: the VM, assembler, and package format, when you need MASM or `.masp` internals
git clone --branch v0.29.4 https://github.com/0xMiden/miden-vm.git ../miden-vm
```

`--depth 1` is intentionally omitted so you can check out other refs later if needed.

### `compiler/` — The Rust-to-MASM Compiler and the Guest SDK

Contains the SDK that powers the `#[component_storage]`, `#[component]`, `#[account_procedure]`, `#[auth_script]`, `#[account]`, `#[note]`, `#[note_script]` and `#[tx_script]` macros.

- **`compiler/examples/`** — exactly 12 working example projects, and the most reliable reference for
  "how do I write X":

  | Example | Shows |
  |---|---|
  | `compiler/examples/counter-contract/` | account component with `StorageMap`, `#[account_procedure]`, full `miden-project.toml` |
  | `compiler/examples/basic-wallet/` | asset in/out, `output_note::create` wrapped as an account procedure |
  | `compiler/examples/storage-example/` | `StorageValue` + `StorageMap<Word, AssetAmount>`, `miden::generate!()` + `bindings::export!` over a hand-written WIT interface |
  | `compiler/examples/auth-component-no-auth/` | minimal `#[auth_script]` auth component, `incr_nonce()` |
  | `compiler/examples/auth-component-rpo-falcon512/` | signature-checking auth component, transaction-summary hashing |
  | `compiler/examples/p2id-note/` | typed `#[note]` struct decoded from note storage, `#[account(basic_wallet::BasicWallet)]` wrapper |
  | `compiler/examples/p2ide-note/` | manual `active_note::get_storage()` parsing, `BlockNumber` timelock comparison |
  | `compiler/examples/counter-note/` | note script calling a component through generated bindings, with no `#[account(..)]` wrapper |
  | `compiler/examples/basic-wallet-tx-script/` | `#[tx_script]`, advice-provider input loading, calling wallet procedures |
  | `compiler/examples/collatz/`, `compiler/examples/fibonacci/`, `compiler/examples/is-prime/` | plain compute programs, no account context |

  There is no faucet example here. For faucet reference use
  `protocol/crates/miden-standards/src/account/faucets/fungible/mod.rs` (the `FungibleFaucet`
  component) or the compiler's own `compiler/tests/integration/src/sdk/base/faucet.rs` binding test.

- **`compiler/sdk/`** — the guest SDK, and at v0.16 the single most authoritative API reference. Whitelisted
  for exploration:
  - `compiler/sdk/sdk/MIGRATION.md` — start here; the `## Unreleased` section is the v0.16 contract-surface
    change list with before/after code
  - `compiler/sdk/CHANGELOG.md`
  - `compiler/sdk/base-sys/src/bindings/` — the exact signature of every kernel binding
    (`active_account.rs`, `native_account.rs`, `active_note.rs`, `input_note.rs`, `output_note.rs`,
    `note.rs`, `faucet.rs`, `tx.rs`, `types.rs`)
  - `compiler/sdk/base/src/types/storage.rs` — `StorageValue` / `StorageMap` / `WordKey` / `WordValue`
  - `compiler/sdk/base-macros/src/` — macro behaviour and, importantly, the exact error messages
    (`component_macro/mod.rs`, `component_macro/storage.rs`, `component_macro/sibling.rs`,
    `foreign_account.rs`, `note.rs`, `script.rs`, `wit_world.rs`)

- **`compiler/tests/integration-network/src/mockchain/`** — end-to-end multi-contract MockChain flows
  (counter contract under three auth components, FPI in many shapes, asset transfer), including
  the live storage-slot-name strings in `compiler/tests/integration-network/src/mockchain/support/helpers.rs`.

**WARNING**: Do NOT explore the compiler's own internals — `compiler/codegen/`, `compiler/hir/`, `compiler/hir-analysis/`, `compiler/hir-transform/`, `compiler/frontend/`, `compiler/midenc-compile/`, `compiler/midenc-session/` — they are implementation details that will confuse the agent and lead to incorrect code.
The one narrow exception is `compiler/frontend/wasm/src/component/` when you need the exact wording of a
call-boundary (16-felt) diagnostic.

**Explore when**: Writing any new contract type, checking an exact binding signature, or finding working code for a pattern not covered by skills.

### `protocol/` — Protocol Layer and Standard Library

The protocol repo (`github.com/0xMiden/protocol`; primary crate `miden-protocol`). Contains the protocol specification, standard components, and standard note types. Workspace crates: `miden-agglayer`, `miden-block-prover`, `miden-protocol`, `miden-protocol-build-utils`, `miden-standards`, `miden-testing`, `miden-tx`, `miden-tx-batch`.

- **`protocol/crates/miden-standards/`** — Standard note types (`P2idNote`, `P2ideNote`, `SwapNote`,
  `PswapNote`, `BurnNote`, `MintNote` under `protocol/crates/miden-standards/src/note/`) and standard account
  components (BasicWallet, FungibleFaucet, authentication components). Standard notes are built with
  typed `bon` builders — e.g.
  `P2idNote::builder().sender(..).target(..).serial_number(..).asset(..).build()?` — and convert via
  `impl From<P2idNote> for Note`; there is no `XNote::create(..)`. Each type also exposes the
  associated function `script_root() -> NoteScriptRoot`.
- **`protocol/crates/miden-protocol/asm/`** — the MASM. Four things live here, and the split matters:
  - `protocol/crates/miden-protocol/asm/kernels/transaction/lib/api.masm` — **the syscall surface**. This
    directory contains `api.masm` and nothing else. Start here for a procedure's signature, stack
    contract, and its context assertions (`exec.memory::assert_native_account`,
    `exec.authenticate_account_origin`, `exec.assert_auth_procedure_origin`). The kernel binaries
    are `protocol/crates/miden-protocol/asm/kernels/transaction/bin/main.masm` and
    `protocol/crates/miden-protocol/asm/kernels/transaction/bin/tx_script_main.masm`.
  - `protocol/crates/miden-protocol/asm/kernels/transaction-core/src/` — **the implementations**:
    `mod.masm`, `account.masm`, `account_update.masm`, `asset.masm`, `asset_vault.masm`,
    `callbacks.masm`, `constants.masm`, `epilogue.masm`, `faucet.masm`, `fungible_asset.masm`,
    `input_note.masm`, `link_map.masm`, `memory.masm`, `non_fungible_asset.masm`, `note.masm`,
    `output_note.masm`, `prologue.masm`, `tx.masm`.
  - `protocol/crates/miden-protocol/asm/protocol/src/` — the userspace `miden::protocol::*` modules that the
    Rust SDK bindings actually map onto: `active_account.masm`, `active_note.masm`,
    `native_account.masm`, `note.masm`, `output_note.masm`, `input_note.masm`, `faucet.masm`,
    `tx.masm`, `asset.masm`, `auth.masm`, `account_id.masm`, `kernel_proc_offsets.masm`,
    `types.masm`, `constants.masm`, `mod.masm`.
  - `protocol/crates/miden-protocol/asm/protocol_utils/src/` — shared helpers (`account_id.masm`,
    `asset.masm`, `constants.masm`, `mem.masm`, `note.masm`, `types.masm`, `mod.masm`).

  The batch kernel is separate: `protocol/crates/miden-protocol/asm/kernels/batch/src/main.masm`.
- **`protocol/crates/miden-tx/`** — Rust execution engine (executor, prover, host). Orchestrates transaction execution but rarely needed for understanding contract behavior. Explore only if debugging execution infrastructure or host-level behavior.
- **`protocol/crates/miden-testing/`** — MockChain implementation internals:
  `protocol/crates/miden-testing/src/mock_chain/chain.rs` (`MockChain`),
  `protocol/crates/miden-testing/src/mock_chain/chain_builder.rs` (`MockChainBuilder`),
  `protocol/crates/miden-testing/src/mock_transaction/builder.rs` (`MockTransactionBuilder`).
  `protocol/crates/miden-testing/src/kernel_tests/tx/` is the
  canonical worked usage of the rc.6 API against a MockChain.

**Note on standard components**: `miden-standards` ships them as MASM
(`protocol/crates/miden-standards/asm/standards/wallets/basic.masm`,
`protocol/crates/miden-standards/asm/standards/notes/p2id.masm`,
`protocol/crates/miden-standards/asm/components/auth/singlesig/singlesig.masm`). A MASM
component participates in the account interface by annotating its exports `@account_procedure` or
`@auth_script`. Separately, the compiler now ships a **Rust** `basic-wallet` account component in
`compiler/examples/basic-wallet/`, and `compiler/examples/p2id-note/`, `compiler/examples/p2ide-note/` and
`compiler/examples/basic-wallet-tx-script/` call it through `#[account(basic_wallet::BasicWallet)]` — so a
Rust-compiled wallet component is callable from Rust. Explore `miden-standards` for note flows,
data layouts, and the canonical MASM shape.

**Explore when**: Understanding note flows, P2ID/SWAP/faucet data layouts, or what SDK functions actually do under the hood (via the kernel MASM).

### `miden-client/` - Client Library (crate `miden-client`)

The client repo is `https://github.com/0xMiden/miden-client`; the published crate is `miden-client`,
and the CLI source lives under `bin/miden-cli/`.

- Rust client for building transactions, syncing state, managing accounts and notes
- Re-exports the protocol types you need at the boundary, e.g. `miden_client::note::P2idNote`,
  `miden_client::note::NoteScriptRoot`, `miden_client::asset::AssetId`
- `miden-client/bin/miden-cli/` is CLI tool source, useful as a reference for client usage patterns

**Explore when**: Deploying contracts to testnet, submitting transactions, syncing state, managing notes on-chain.

### `miden-vm/` — VM, Assembler, and Package Format

Only needed for MASM or artefact-level questions, but authoritative for them:

- `miden-vm/crates/project/src/ast/target.rs` — the `miden-project.toml` schema (`[lib]` / `[[bin]]`
  targets, mandatory `path`, `deny_unknown_fields`)
- `miden-vm/crates/mast-package/src/package/` — the `.masp` package format (`Package`, magic `b"MASP\0"`)
- `miden-vm/crates/assembly/src/assembler.rs` — `Assembler::link_package` / `with_package` / `with_kernel`;
  `assemble_library` / `assemble_kernel` / `assemble_program` all return `Box<Package>`. `.masl`,
  `Library` and `KernelLibrary` no longer exist.
- `miden-vm/crates/lib/core/asm/debug.masm` — the `miden::core::debug` printers that replaced the removed
  `debug.*` decorators
- `miden-vm/docs/src/user_docs/assembly/code_organization.md` — module tree, `mod` / `pub mod` declarations,
  and the `use {a, b} from path` / `pub use {a} from path` import syntax

---

## What to Explore for Each Contract Type

| Building This | Explore These Paths | What to Look For |
|---|---|---|
| Account component with storage | `compiler/examples/counter-contract/`, `compiler/examples/storage-example/` | `StorageMap<K, V>` / `StorageValue<T>` patterns, `#[account_procedure]` on the trait, `miden-project.toml` shape |
| Note script | `compiler/examples/p2id-note/`, `compiler/examples/p2ide-note/`, `compiler/examples/counter-note/` | `#[note]` + `#[note_script]`, typed vs manual note-storage parsing, cross-component calls |
| Transaction script | `compiler/examples/basic-wallet-tx-script/` | `#[tx_script]`, `#[account(..)]` wrapper, advice-provider inputs |
| Authentication component | `compiler/examples/auth-component-no-auth/`, `compiler/examples/auth-component-rpo-falcon512/` | exactly one `#[auth_script]`, `project-kind = "authentication-component"`, `incr_nonce()`, tx-summary hashing |
| Faucet (token minting) | `protocol/crates/miden-standards/src/account/faucets/fungible/mod.rs`, `compiler/tests/integration/src/sdk/base/faucet.rs` | `FungibleFaucet::builder()`, `faucet::mint` / `faucet::burn` bindings, `supported-types = ["FungibleFaucet", "NonFungibleFaucet"]` |
| P2ID output notes | `compiler/examples/basic-wallet/src/lib.rs`, `protocol/crates/miden-standards/src/note/p2id.rs` | `note::build_recipient`, `P2idNote::script_root()`, `output_note::create` wrapped as an account procedure |
| Swap notes | `protocol/crates/miden-standards/src/note/swap.rs` | SwapNote data layout, tag construction, payback flow |
| Multi-step / multi-contract tests | `compiler/tests/integration-network/src/mockchain/`, `protocol/crates/miden-testing/src/kernel_tests/tx/` | MockChain setup, init → operate → verify flow, output-note verification, storage-slot names |
| Client deployment | `miden-client/` | TransactionRequestBuilder, sync, submit patterns |
| SDK binding signatures | `compiler/sdk/base-sys/src/bindings/*.rs`, `compiler/sdk/sdk/MIGRATION.md` | exact Rust signature and return type of every kernel binding |
| SDK function internals | `protocol/crates/miden-protocol/asm/kernels/transaction/lib/api.masm` → `protocol/crates/miden-protocol/asm/kernels/transaction-core/src/*.masm` → `protocol/crates/miden-protocol/asm/protocol/src/*.masm` | `api.masm` for signatures + context assertions, `protocol/crates/miden-protocol/asm/kernels/transaction-core/src/` for implementations, `protocol/crates/miden-protocol/asm/protocol/src/` for the userspace wrappers the bindings call |

---

## Common Advanced Patterns

These patterns go beyond what the basic skills cover. For each, the source repos contain working implementations.

### Multi-Component Accounts
Accounts compose several components at creation time: custom logic plus an authentication component, and optionally a standard component. Every account needs exactly one authentication component, whose single `#[auth_script]` procedure is the only place `incr_nonce()` may be called. `compiler/tests/integration-network/src/mockchain/counter/` builds the same counter account against three different auth components and is the clearest worked example. Standard components can be MASM (`miden-standards`) or Rust (`compiler/examples/basic-wallet/`); in both cases the interface is defined by `@account_procedure` / `#[account_procedure]` annotations on the exported procedures.

### Output Note Creation from Contracts
Create output notes (like P2ID) from within contract code: build a recipient with `note::build_recipient(serial_num, script_root, storage)`, call `output_note::create(tag, note_type, recipient)` for the index, then move assets in with `native_account::remove_asset(asset)` + `output_note::add_asset(asset, note_idx)`.

Both `output_note::create` and `native_account::remove_asset` are **account-context-only at
runtime**, so the whole sequence has to live inside an account-component procedure. A tx script or
note script that calls them directly compiles and then fails during execution.
`compiler/examples/basic-wallet/src/lib.rs` is the reference: it exposes
`#[account_procedure] fn create_note(..) -> NoteIdx` and
`#[account_procedure] fn move_asset_to_note(asset, note_idx)`, and
`compiler/examples/basic-wallet-tx-script/src/lib.rs` drives them from the script side.

### Note Storage Protocol
A note's storage is exposed to its `#[note_script]` as a `Vec<Felt>` via `active_note::get_storage()`. Two styles:

- **Typed (preferred)** — give the `#[note]` struct fields and the macro auto-decodes them: it
  generates a `TryFrom<&[Felt]>` that reads each field via `FromFeltRepr` and then calls
  `ensure_eof()`, so trailing felts are a decode error rather than ignored padding. A zero-sized
  `#[note]` struct skips `get_storage()` entirely. See `compiler/examples/p2id-note/src/lib.rs`
  (`#[note] struct P2idNote { target_account_id: AccountId }`).
- **Manual** — read `active_note::get_storage()` and index it yourself, asserting the length. See
  `compiler/examples/p2ide-note/src/lib.rs`, which asserts `inputs.len() == 4` and then builds
  `AccountId` and `BlockNumber` values out of the felts.

Attached assets are separate from storage and are read with `active_note::get_initial_assets()`.

### Atomic Swaps
The standard SwapNote in `protocol/crates/miden-standards/src/note/swap.rs` creates a payback P2ID note automatically when consumed. Explore the `SwapNote` `bon` builder and `SwapNote::script_root()` to understand tag construction, storage layout, and the payback mechanism.

### Account Initialization
Use `#[tx_script]` to run setup or admin operations against an account before or between note flows. The signature is validated by the macro: at most two parameters, the first literally typed `Word`, the second a reference to an `#[account(...)]` wrapper type. `compiler/examples/basic-wallet-tx-script/src/lib.rs` is the reference — `fn run(arg: Word, account: &mut Wallet)`, loading its inputs from the advice provider and then calling account procedures.

### Token Creation (Faucets)
Faucet accounts mint and burn tokens. The `FungibleFaucet` standard component
(`protocol/crates/miden-standards/src/account/faucets/fungible/mod.rs`) is built with
`FungibleFaucet::builder().name(..).symbol(..).decimals(..).max_supply(..).build()?`, where the
required setters take `TokenName`, `TokenSymbol`, `u8` and `AssetAmount`, `build()` returns
`Result<FungibleFaucet, FungibleFaucetError>`, and `MAX_DECIMALS = 12`. Optional setters:
`token_supply` (`AssetAmount`), `description`, `logo_uri`, `external_link`,
`is_description_mutable`, `is_logo_uri_mutable`, `is_external_link_mutable`,
`is_max_supply_mutable`.

On the contract side only `faucet::mint(Asset)` and `faucet::burn(Asset)` exist — in-transaction
asset construction was removed, so build the `Asset` outside the transaction. There is no faucet
example in `compiler/examples/`; use `compiler/tests/integration/src/sdk/base/faucet.rs`, a
compile-only binding test (it ends at `test.compile_package()`) that also shows the faucet manifest
(`supported-types = ["FungibleFaucet", "NonFungibleFaucet"]`).

### P2ID with Expiration (P2IDE)
Send assets with a deadline — the sender can reclaim after the block height passes. `compiler/examples/p2ide-note/src/lib.rs` and `protocol/crates/miden-standards/src/note/p2ide.rs` show the timelock pattern. Note that the example works in typed `BlockNumber` values (`BlockNumber::try_from(inputs[n]).unwrap()`, then plain `>=` comparison), not raw felts, because `BlockNumber` orders as an integer.
