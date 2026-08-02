---
name: miden-client-cli
description: Map to the official Miden client CLI. The recommended path is via midenup, which installs a managed `client` toolchain component and is invoked as `miden client ...` (component invocation through the midenup `miden` wrapper). The underlying / direct-install path is the `miden-client-cli` crate (`cargo install miden-client-cli --locked`), which exposes the binary `miden-client` and is invoked as `miden-client ...`. Both paths run the same upstream binary. Covers install, init, network selection, and where to find the canonical command reference and configuration docs. Use when an agent needs to create accounts, query state, mint, send, or consume notes against a running Miden node from the command line; pair with the local-node-validation skill for localhost workflows.
---

# Miden Client CLI

The Miden client CLI is the command-line wrapper around the `miden-client` library. It creates accounts, syncs state, mints assets, sends transactions, and consumes notes against a running Miden node.

This skill maps agents to the canonical install paths and upstream command reference. Do not memorize commands or config keys here. Follow the links.

When to reach for this skill: the user wants to interact with a node from the shell. For Rust-library work against a localhost node from a binary, see the `local-node-validation` skill. For test-side note construction, see `rust-sdk-testing-patterns`.

## Two Invocation Paths

There are two ways to invoke the same upstream binary.

**Through midenup (recommended).** Run `miden client <args>`. This is component invocation: the midenup `miden` wrapper looks up the `client` component in the active toolchain and runs its installed executable. `miden client` is **not** an alias; the midenup README alias table only documents short-hand aliases (such as `miden account`, `miden faucet`, `miden new-wallet`, `miden send`), and component invocation is independent of the alias map. The toolchain manifest declares `installed_executable: miden-client` for the `client` component.

**Direct install.** Run `cargo install miden-client-cli --locked`, then invoke `miden-client <args>`. This installs the same upstream binary that midenup delegates to.

Both paths execute identically.

References:
- midenup install, init, toolchain delegation, and alias docs: [github.com/0xMiden/midenup](https://github.com/0xMiden/midenup). midenup is unpublished; the canonical install command is `cargo install --path .` or `cargo install --git <repo_uri>`.
- miden-client CLI install and setup: [miden-client `bin/miden-cli/README.md`](https://github.com/0xMiden/miden-client/blob/main/bin/miden-cli/README.md).

## Install via midenup

```sh
cargo install midenup && midenup init
midenup install stable
```

`midenup init` creates a `miden` symlink in `$CARGO_HOME/bin` (default `~/.cargo/bin`). If `miden` is not found after init, ensure `$CARGO_HOME/bin` is on your `PATH`.

## First-Time Initialization

`init` writes a `miden-client.toml` config in the current directory:

```sh
miden client init --network localhost     # or testnet | devnet | http://<custom-rpc>[:port]
```

Subsequent commands operate against that config. For localhost workflows, pair this skill with `local-node-validation`: it boots a local node on `http://0.0.0.0:57291` and prepares a clean keystore.

## Canonical Command Reference

Follow the canonical in-repo references on `0xMiden/miden-client` (the active line, which matches project-template's pinned `miden-client = "0.14"`). The `miden-docs` site (`0xMiden.github.io/miden-docs/...`) is not used here because those URLs are not stable.

- CLI Reference: [`docs/external/src/rust-client/cli/index.md`](https://github.com/0xMiden/miden-client/blob/main/docs/external/src/rust-client/cli/index.md)
- CLI Configuration: [`docs/external/src/rust-client/cli/cli-config.md`](https://github.com/0xMiden/miden-client/blob/main/docs/external/src/rust-client/cli/cli-config.md)
- Repo overview and recent release notes: [`0xMiden/miden-client`](https://github.com/0xMiden/miden-client) (browse the `CHANGELOG.md` on `main` for the latest behavioral changes; pin to a release tag if you need a snapshot).

For the live command list and flags on the installed binary, run `miden client --help` (midenup path) or `miden-client --help` (direct-install path). Drill into a specific command with `miden client <command> --help`. The canonical references above remain authoritative for deeper documentation.

## Cross-References

- `local-node-validation`: Rust-binary path against a localhost node, plus node bootstrap and a clean-keystore recipe.
- `rust-sdk-testing-patterns` ("Note Construction"): building notes from compiled `.masp` packages in tests.
- `rust-sdk-patterns` ("Cross-Component Note Pattern"): note scripts that read inputs and call account-component methods, the source of many `consume-notes` flows.
