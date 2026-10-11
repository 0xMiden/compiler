---
name: local-node-validation
description: Validates Miden contracts against a local node. Covers node setup, Rust binary adaptation, state verification, and troubleshooting. Use after MockChain tests pass to verify contracts work against a real node.
---

# Local Node Validation

Validates that contracts working in MockChain also work against a real Miden node. This catches known MockChain/live-node behavior gaps before they become harder to debug in production or the frontend.

## Why This Matters

MockChain simplifies execution in ways that hide real-world failures:

1. **No automatic block production** -- MockChain requires explicit `prove_next_block()`. A live node produces blocks on its own schedule.
2. **No network transport** -- MockChain does not simulate the network transaction builder (ntx-builder) that handles network notes.
3. **No RPC latency or timeouts** -- MockChain executes locally and instantly. Live nodes have gRPC round-trips with configurable timeouts.
4. **No version/genesis validation** -- MockChain skips the protocol-version check that a live node negotiates at connect. The live negotiation rides on an `accept` header of the form `application/vnd.miden; version=<crate version>; genesis=<commitment>`; a rejection surfaces as `RpcError::AcceptHeaderError`.
5. **Different scheduling** -- MockChain has no block-production cadence, no RPC round-trip, and no rate limiting, so timing-dependent bugs do not reproduce.

## Prerequisites

- [ ] Your MockChain integration tests pass (e.g. `cargo test -p <your-integration-crate> --release`)
- [ ] A Miden node available locally. The node is **not** a single binary -- it is composed of standalone executables (validator, sequencer, ntx-builder, transaction prover). The client's own test infra installs the full set: `miden-validator`, `miden-node`, `miden-ntx-builder`, `miden-remote-prover` (see `scripts/start-test-node.sh` in the `miden-client` repo), with `cargo install --locked`. The node version the client is built against resolves from crates.io via your `Cargo.lock`, not from a git source.
- [ ] A working network/integration validation binary exists in your project that you can use as the starting template for the localhost variant

> The local-node launch CLI lives in the 0xMiden/node repo, not in `miden-client`. The commands below are the topology the client's `make start-node` target (`scripts/start-test-node.sh`) drives; confirm exact flags against your installed node's `--help` for the version you run.

## Step 1: Clean State and Start Local Node

**Every node session must start from clean state.** Stale store files and keystore directories cause conflicts, deserialization errors, and misleading test results. Always wipe before starting. Serialized artifacts do not round-trip across protocol versions -- the MAST wire format is `[0, 0, 4]` and the package format is `[6, 0, 0]` -- so a fresh store is required after any version change. The helper script does `rm -rf "$DATA"` on every start for exactly this reason.

The simplest path is the client's `make start-node` target, which runs the bundled `scripts/start-test-node.sh` helper: it installs the node binaries (pinned to your `Cargo.lock`), generates genesis, bootstraps each component, and starts the split topology for you:

```bash
# From a checkout of the miden-client repo pinned to your client version:
make start-node              # foreground, streams logs; Ctrl+C stops
# or
make start-node-background   # returns once RPC is ready (used by CI)
```

This brings up the four-component topology and exposes the RPC on `127.0.0.1:57291` (the client default, `MIDEN_NODE_PORT`).

If you run the node binaries directly instead of via `make start-node`, the shape is below. Treat it as a reference skeleton, not a copy-paste recipe: it omits details the script handles for you (it does not show generating the genesis config the validator bootstraps from, and it leaves out the shared network-tx auth header that the sequencer and ntx-builder must agree on or the sequencer rejects the ntx-builder's transactions). Verify every subcommand and flag against `--help` for your node version, or just use `make start-node`.

```bash
# 0. Build the genesis block ONCE with the dedicated `genesis` subcommand.
#    (The genesis.toml it consumes is produced by the client's `gen-genesis` binary:
#     cargo build --release -p test-node-genesis --bin gen-genesis
#     ./target/release/gen-genesis <data>/genesis-config)
miden-validator genesis \
  --genesis-block-directory <data>/genesis --accounts-directory <data>/accounts \
  --config <data>/genesis-config/genesis.toml

# 1. Bootstrap each component from that genesis block. All three take --genesis.
#    Create each component's data directory first -- they open their SQLite DB
#    directly and do not mkdir it for you.
miden-validator   bootstrap --data-directory <data>/validator   --genesis <data>/genesis/genesis.dat
miden-node        bootstrap --data-directory <data>/node        --genesis <data>/genesis/genesis.dat
miden-ntx-builder bootstrap --data-directory <data>/ntx-builder --genesis <data>/genesis/genesis.dat

# 2. Start the components in order: validator, then sequencer (which carries the RPC)
#    and prover, then ntx-builder. The sequencer and ntx-builder must agree on the
#    network-tx auth header or the sequencer rejects the ntx-builder's transactions.
#    The validator requires threshold storage-key material to start at all.
miden-validator start --listen 127.0.0.1:50101 --data-directory <data>/validator \
  --storage-key.epoch <64 hex chars> \
  --storage-key.setup-context  <keydir>/setup-context.wire \
  --storage-key.public-key-set <keydir>/public-key-set.wire \
  --storage-key.secret-share   <keydir>/secret-share.wire

miden-node sequencer --rpc.listen 127.0.0.1:57291 --data-directory <data>/node \
  --validator.url http://127.0.0.1:50101 --ntx-builder.url http://127.0.0.1:50301 \
  --block.interval 3s --batch.interval 1s \
  --rpc.network-tx-auth-header-value "$NETWORK_TX_AUTH" \
  --rpc.rate-limit.burst-size 10000 --rpc.rate-limit.replenish-per-second 10000

miden-remote-prover --kind=transaction --port=50051

miden-ntx-builder start --listen 127.0.0.1:50301 --rpc.url http://127.0.0.1:57291 \
  --tx-prover.url http://127.0.0.1:50051 --data-directory <data>/ntx-builder \
  --rpc.auth-header-value "$NETWORK_TX_AUTH" --max-cycles $((1 << 18))
```

Three things here bite hard if you skip them:

- **The validator will not start without threshold storage-key material.** The client vendors insecure development fixtures for exactly this at `scripts/testdata/insecure-golden-storage-key/`. Never use those outside a local test node.
- **Without the rate-limit bump, an integration run gets throttled** by the sequencer's default limiter and starts failing in ways that look like network flakiness.
- **Start ordering matters.** The script sleeps ~2s after the validator and again after the sequencer, then polls the RPC socket for up to 60 seconds.

Tear down with `make stop-node` (`scripts/stop-test-node.sh`), which kills by pid file and falls back to `pkill` on the installed binary paths.

### Private notes need a separate service

`ClientBuilder::for_localhost()` configures **no note transport**. Private-note flows against a local node therefore silently do nothing until you both run the transport service (`make start-note-transport`, which installs `miden-note-transport-node` from `0xMiden/miden-note-transport`) and point the client at it:

```rust
.note_transport(Arc::new(GrpcNoteTransportClient::new(url, timeout_ms)))
```

**This clean-start sequence is mandatory every time.** Do not attempt to reuse state from a previous session.

## Step 2: Add a localhost client helper

Add a `setup_local_client()` function to whichever helper module in your integration / test harness already hosts the network `setup_client()` equivalent. Name and location are up to you -- adjust to your repo's layout.

`.sqlite_store(..)` is **not** an inherent `ClientBuilder` method -- it comes from an extension trait in the `miden-client-sqlite-store` crate. You must bring it into scope or the call fails to compile (method not found):

```rust
use miden_client_sqlite_store::ClientBuilderSqliteExt; // required for .sqlite_store(..)
```

```rust
pub async fn setup_local_client() -> Result<ClientSetup> {
    let endpoint = Endpoint::localhost();          // http://localhost:57291
    let timeout_ms = 10_000;                        // DEFAULT_GRPC_TIMEOUT_MS

    // `.rpc()` uses the client AS PROVIDED. Wrap it, or you silently lose
    // response verification. (`.grpc_client(&endpoint, Some(timeout_ms))` and the
    // `for_*` constructors wrap for you.)
    let rpc_client = Arc::new(VerifyingRpcClient::new(GrpcClient::new(&endpoint, timeout_ms)));

    let keystore_path = std::path::PathBuf::from("../local-keystore");
    let keystore = Arc::new(FilesystemKeyStore::new(keystore_path)
        .context("Failed to initialize local keystore")?);

    let store_path = std::path::PathBuf::from("../local-store.sqlite3");

    let client = ClientBuilder::new()
        .rpc(rpc_client)
        .sqlite_store(store_path)
        .authenticator(keystore.clone())
        .build()
        .await
        .context("Failed to build local Miden client")?;

    Ok(ClientSetup { client, keystore })
}
```

Imports: `use miden_client::rpc::{Endpoint, GrpcClient, VerifyingRpcClient};`.

**There is no debug-mode switch.** `ClientBuilder::in_debug_mode`, `Client::in_debug_mode`, the `DebugMode` type, the CLI `--debug` flag and the `MIDEN_DEBUG` environment variable do not exist — a `.in_debug_mode(..)` line will not compile. What replaced them is a debug adapter, and it is narrower than it sounds: the CLI has its own `dap` feature that is **not** enabled by default, so a stock `miden-client` binary exposes nothing. Built with it, `--start-debug-adapter <ADDR>` (optionally `--record <FILE>`) is accepted by exactly two commands — `exec` and `consume-notes`. Passing it to `mint`, `transfer`, `swap` or a PSWAP command is an unknown-argument error. MASM print-style debugging goes through the `miden::core::debug` procedures, which print unconditionally, so there is nothing to gate.

`build()` fails with `ClientInitializationError` if **either** the RPC client or the store is missing — they are two independent checks, so supplying only one is not enough.

Use separate paths (`local-keystore/`, `local-store.sqlite3`) to avoid contaminating testnet state.

## Step 3: Create a local validation binary

Add a local validation binary alongside your existing network/testnet validation binary, mirroring its structure but swapping in `setup_local_client()`. Pick any conventional name for it (for example `validate_local`) -- adjust to your repo's binary layout.

The binary must:
1. Call `setup_local_client()` instead of the network setup function
2. Sync state: `client.sync_state().await?`. This is **mandatory before the first submit**, not just good hygiene: transaction inputs are sealed against chain state, and a client that has not synced genesis and the chain tip cannot resolve the encryption key.
3. Build contracts (same as the existing binary)
4. Create accounts, create notes, submit transactions
5. Sync again after each transaction submission
6. Wait for transaction inclusion (poll `sync_state` until account state updates)
7. Verify final state matches MockChain test expectations
8. Print clear pass/fail for each verification step

Key differences from the network binary:
- Localhost endpoint (port 57291)
- Separate keystore and store paths
- Must handle block production timing (sync + wait between submissions)

## Step 4: Run and Verify

Ensure clean client state before running (the node should already be clean from Step 1):
```bash
rm -rf local-keystore/ local-store.sqlite3
cargo run --bin <your-local-validation-binary> --release
```

### Verification Checklist

- [ ] `sync_state()` succeeds (node reachable, no version mismatch)
- [ ] Account creation succeeds (account appears after sync)
- [ ] Note publication succeeds (transaction accepted by node)
- [ ] Note consumption succeeds (state transitions as expected)
- [ ] Final state matches MockChain test expectations
- [ ] No RPC timeout errors
- [ ] Node logs show no errors

## Step 5: Inspect Node Logs

Run the node with verbose logging. The helper script honors `RUST_LOG` and writes a per-component log file per service; if you launch the binaries directly, set it on the process you want to inspect (the sequencer carries the RPC):

```bash
RUST_LOG=info make start-node
# or, running the sequencer directly:
RUST_LOG=info miden-node sequencer --rpc.listen 127.0.0.1:57291 --data-directory <data>/node \
  --validator.url http://127.0.0.1:50101 --ntx-builder.url http://127.0.0.1:50301 \
  --block.interval 3s --batch.interval 1s
```

Look for:
- Transaction acceptance/rejection messages
- Block production confirmations
- Error or warning lines

## Troubleshooting

| Symptom | Cause | Fix |
|---------|-------|-----|
| `Unavailable` RPC error | Node not running or wrong port | Start node, verify the sequencer's RPC is listening on 57291 |
| `RpcError::AcceptHeaderError` / "The node rejected the request due to a version mismatch." | Node and client crate versions differ | Run the node version resolved by your client's `Cargo.lock`. The version and genesis commitment are negotiated at connect via the `accept` header and a mismatch is rejected; there is no mixed-version mode |
| `miden-validator start` exits immediately | Missing threshold storage-key material | Pass all four `--storage-key.*` flags; for a local test node use the vendored `scripts/testdata/insecure-golden-storage-key/` fixtures |
| Unknown-argument error on `bootstrap` | Using the old flag names | Genesis is its own `miden-validator genesis --config <toml>` step, and all three `bootstrap` commands take `--genesis <path>` (not `--file`, not `--genesis-config-file`) |
| Requests throttled / intermittent failures under load | Sequencer rate limiter at its default | Start the sequencer with `--rpc.rate-limit.burst-size 10000 --rpc.rate-limit.replenish-per-second 10000` |
| Private notes never arrive | No note transport configured | `ClientBuilder::for_localhost()` sets none — run `make start-note-transport` and pass `.note_transport(..)` |
| Transaction rejected | Invalid proof or state | Check contract code, reset node data, try again |
| Account not found after creation | Haven't synced | Call `sync_state()` after account creation |
| Store errors or deserialization failures | Stale state from previous session (or artifacts from an earlier protocol version, which do not round-trip) | Wipe the node data, keystore, and client store, then re-bootstrap from a fresh genesis |
| `.sqlite_store(..)` does not compile | Extension trait not in scope | `use miden_client_sqlite_store::ClientBuilderSqliteExt;` |
| Block not produced | Node produces blocks on the sequencer's configured cadence | Submit a transaction; check the sequencer's `--block.interval` (and `--batch.interval`) settings, or consult `miden-node sequencer --help` |
