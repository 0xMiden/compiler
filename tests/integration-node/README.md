# Miden Integration Node Tests

This crate contains integration tests that require a local Miden node instance or testnet connection.

## Overview

The tests in this crate are separated from the main integration tests because they:
- Require a local Miden node to be running or testnet connectivity
- Are slower due to network operations and multi-step nature of the test scenarios

## Running Tests

To see debug output from the node:

```bash
MIDEN_NODE_OUTPUT=1 cargo test -p miden-integration-node-tests
```

## Process Cleanup

The local node management system ensures that:
- Only one node instance runs at a time, shared across all tests
- The node is automatically stopped when the last test using the node is finished
- No orphaned miden-node processes remain after test execution
