//! Infrastructure for running a local Miden node for integration tests

use std::path::PathBuf;

mod handle;
mod process;
mod setup;
mod sync;

pub use handle::{ensure_shared_node, SharedNodeHandle};

// Base directory for all miden test node files
const BASE_DIR: &str = "/tmp/miden-test-node";

// Re-export constants that are used in multiple modules
pub(crate) const COORD_DIR: &str = BASE_DIR;

// Construct paths at runtime since concat! doesn't work with const values
pub(crate) fn pid_file() -> PathBuf {
    PathBuf::from(BASE_DIR).join("node.pid")
}

pub(crate) fn ref_count_dir() -> PathBuf {
    PathBuf::from(BASE_DIR).join("refs")
}

pub(crate) fn lock_file() -> PathBuf {
    PathBuf::from(BASE_DIR).join("node.lock")
}

pub(crate) fn data_dir() -> PathBuf {
    PathBuf::from(BASE_DIR).join("data")
}

pub(crate) const RPC_PORT: u16 = 57291;

// Construct RPC URL using the port constant
pub(crate) fn rpc_url() -> String {
    format!("http://127.0.0.1:{RPC_PORT}")
}
