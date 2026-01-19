# Miden Integration Network Tests

This crate contains integration tests that exercise contract deployment and execution on a mock
chain (`miden_client::testing::MockChain`).

## Overview

The tests in this crate are separated from the main integration tests because they:
- Exercise multi-step end-to-end scenarios (account setup, block production, tx execution)
- Can be slower due to proving and compiling example packages

## Running Tests

```bash
cargo test -p midenc-integration-network-tests
```

To see test output:

```bash
cargo test -p midenc-integration-network-tests -- --nocapture
```
