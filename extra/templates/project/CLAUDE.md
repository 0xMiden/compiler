# Miden Project

This is a Miden smart contract project using the Rust SDK and compiler.

## Project Structure

- `contracts/`: Smart contracts (each is a separate crate, excluded from workspace)
  - Account components (`#[component]`)
  - Note scripts (`#[note]`)
  - Transaction scripts (`#[tx_script]`)
- `integration/`: Integration tests and deployment scripts (workspace member)

## Build & Test

Contracts are built individually with cargo-miden (not `cargo build`):
```
cargo miden build --manifest-path contracts/<name>/Cargo.toml --release
```

Tests run via the workspace:
```
cargo test -p integration --release
```

Always build contracts before running tests; tests compile contracts via `build_project_in_dir()`.

## SDK Quick Reference

See the working examples in this project:
- `contracts/counter-account/src/lib.rs`: Account component with typed `StorageMap<Word, Felt>`
- `contracts/increment-note/src/lib.rs`: Note script with cross-component call
- `integration/tests/counter_test.rs`: MockChain integration test

Common cargo commands:
- `cargo build -p integration --bin <name> --release`: build a specific binary from the integration crate (e.g. `validate_local`, `increment_count`)
- `cargo clean -p integration`: run after editing shared library code (e.g. `helpers.rs`) before re-running tests, to avoid stale compiled binaries

## Critical Pitfalls

**Felt arithmetic is modular (SECURITY CRITICAL)**: Subtraction wraps around the field modulus instead of panicking. ALWAYS validate before subtraction:
```rust
assert!(
    current.as_canonical_u64() >= amount.as_canonical_u64(),
    "Insufficient balance"
);
let result = current - amount;
```

**Felt comparisons are misleading for quantity logic**: `<`, `>`, `<=`, `>=` on Felt compare field elements, which differs from natural number ordering. For business logic (balances, amounts, counts), ALWAYS convert first: `a.as_canonical_u64() < b.as_canonical_u64()`

**No-std required**: All contracts must use `#![no_std]` and `#![feature(alloc_error_handler)]`. For heap allocation, use `extern crate alloc;` and `BumpAlloc`.

## Advanced Development

For complex applications beyond basic patterns (multi-contract apps, novel note flows, custom asset handling):

1. Clone Miden source repos alongside this project (see `rust-sdk-source-guide` skill for repo list and clone commands)
2. Use Plan Mode first; explore source repos to design the architecture before writing code
3. Use sub-agents to explore repos efficiently without filling main context

## Verification Workflow

After modifying contract code, always:
1. Write tests alongside contracts; tests are the primary verification, builds are the secondary check
2. Build the contract: `cargo miden build --manifest-path contracts/<name>/Cargo.toml --release`
3. Run tests: `cargo test -p integration --release`
