---
name: miden-client-cli
description: Use when an agent needs to create accounts, query state, mint, transfer, or consume notes against a running Miden node from the shell.
---

# Miden Client CLI

The Miden client CLI is the command-line wrapper around the `miden-client` library. It creates accounts, syncs state, mints assets, submits transactions, and consumes notes against a running Miden node.

This project uses protocol and released client crates `0.18.0` from crates.io. Match the CLI release to the client version recorded in `Cargo.lock`.

When to reach for this skill: the user wants to interact with a node from the shell. For Rust-library work against a localhost node from a binary, see the `local-node-validation` skill. For test-side note construction, see `rust-sdk-testing-patterns`.

## Installation and Version Check

The repository README uses midenup for the managed Miden toolchain. After installing and initializing midenup, the `miden` wrapper delegates `miden client <args>` to the toolchain's `client` component, whose installed executable is `miden-client`. Always check the installed component before using an existing store:

```sh
miden client --version
```

Midenup components are independent of this repository's `Cargo.lock`. Use a managed client only when it matches the protocol and client release you are validating. A `0.17` client does not match this project's protocol `0.18.0` types.

To build the matching CLI from source, check out the `v0.18.0` release of `0xMiden/rust-sdk` and build its `miden-client-cli` package with `cargo build --release --locked -p miden-client-cli`. The resulting executable is `target/release/miden-client`. Check its `--version` before using it with this project's store.

References:
- midenup install, init, toolchain delegation, and component docs: [github.com/0xMiden/midenup](https://github.com/0xMiden/midenup).
- Exact direct CLI install and setup: [rust-sdk release `bin/miden-cli/README.md`](https://github.com/0xMiden/rust-sdk/blob/v0.18.0/bin/miden-cli/README.md).

## First-Time Initialization

`init` is optional because the CLI can self-initialize. Explicit initialization is safer when selecting a network and a fresh store. By default it creates global configuration at `~/.miden/miden-client.toml`; pass `--local` to create `./.miden/miden-client.toml`. A local config takes precedence over the global config.

Use a fresh local configuration and store for the selected client release and network. With the CLI built from the matching source:

```sh
miden-client init --local --network testnet --store-path store.sqlite3
```

With midenup, the equivalent shape is `miden client init --local --network testnet --store-path store.sqlite3`. Omitting `--network` selects Testnet. Other accurate choices are `localhost`, `testnet`, or a full custom HTTP(S) RPC URL. Keep stores separate across incompatible client versions and networks; archive an old store and initialize a fresh path. For localhost workflows, pair this skill with `local-node-validation`.

Historical v0.16 command changes included `transfer` in place of the removed `send` subcommand, and `account --inspect <ID> --verbose` in place of the removed `account --show --with-code`. Use `miden-client <command> --help` for the exact installed command surface. The old client/CLI debug-mode toggle and `--debug` flag are removed.

## Canonical Command Reference

Use the references for the client release below, matching the version recorded in the project's `Cargo.lock`.

- CLI Reference: [`docs/external/src/rust-client/cli/index.md`](https://github.com/0xMiden/rust-sdk/blob/v0.18.0/docs/external/src/rust-client/cli/index.md)
- CLI Configuration: [`docs/external/src/rust-client/cli/cli-config.md`](https://github.com/0xMiden/rust-sdk/blob/v0.18.0/docs/external/src/rust-client/cli/cli-config.md)
- Release behavior: [`CHANGELOG.md`](https://github.com/0xMiden/rust-sdk/blob/v0.18.0/CHANGELOG.md).

For the live command list and flags, run `miden-client --help` and drill into a command with `miden-client <command> --help`. If an exact-version midenup component is deliberately selected, the equivalent invocation is `miden client ...`.

## Cross-References

- `local-node-validation`: Rust-binary path against a localhost node, plus node bootstrap and a clean-keystore recipe.
- `rust-sdk-testing-patterns` ("Note Construction"): building notes from compiled `.masp` packages in tests.
- `rust-sdk-patterns` ("Cross-Component Note Pattern"): note scripts that read inputs and call account-component methods, the source of many `consume-notes` flows.
