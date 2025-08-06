//! Handle for managing shared node instances

use std::fs;

use anyhow::{Context, Result};
use uuid::Uuid;

use super::{
    process::{is_port_in_use, is_process_running, start_shared_node},
    ref_count_dir, rpc_url,
    setup::LocalMidenNode,
    sync::{
        acquire_lock, add_reference, get_ref_count, read_pid, stop_node_if_no_references, write_pid,
    },
    RPC_PORT,
};

/// Handle to the shared node instance. When dropped, decrements the reference count.
pub struct SharedNodeHandle {
    /// The RPC URL of the shared node
    rpc_url: String,
    /// Unique ID for this handle
    handle_id: String,
}

impl SharedNodeHandle {
    /// Get the RPC URL for connecting to the node
    pub fn rpc_url(&self) -> &str {
        &self.rpc_url
    }
}

impl Drop for SharedNodeHandle {
    fn drop(&mut self) {
        eprintln!("[SharedNode] Dropping handle {}", self.handle_id);

        // Remove our reference file
        let ref_file = ref_count_dir().join(&self.handle_id);
        if let Err(e) = fs::remove_file(&ref_file) {
            eprintln!("[SharedNode] Warning: Failed to remove ref file: {e}");
        }

        stop_node_if_no_references();
    }
}

/// Ensure the shared node is running and return a handle to it
pub async fn ensure_shared_node() -> Result<SharedNodeHandle> {
    LocalMidenNode::ensure_installed().context("Failed to ensure miden-node is installed")?;

    let handle_id = format!("handle-{}-{}", std::process::id(), Uuid::new_v4());
    let _lock = acquire_lock().context("Failed to acquire lock for node coordination")?;

    let existing_pid = read_pid().context("Failed to read PID file")?;

    let pid = match existing_pid {
        Some(pid) if is_process_running(pid) => {
            // Check if the node is actually responding
            if is_port_in_use(RPC_PORT) {
                eprintln!("[SharedNode] Using existing node process {pid}");
                pid
            } else {
                eprintln!("[SharedNode] Found dead node process {pid}, restarting...");
                // Node process exists but isn't responding, start a new one
                let new_pid = start_shared_node()
                    .await
                    .context("Failed to start new node after finding dead process")?;
                write_pid(new_pid).context("Failed to write PID file")?;
                new_pid
            }
        }
        _ => {
            // No running node, start a new one
            eprintln!("[SharedNode] No existing node found, starting new instance");
            let new_pid = start_shared_node().await.context("Failed to start new node instance")?;
            write_pid(new_pid).context("Failed to write PID file")?;
            new_pid
        }
    };

    // Add our reference
    add_reference(&handle_id).context("Failed to add reference for handle")?;

    // Log current state
    let ref_count = get_ref_count().context("Failed to get reference count")?;
    eprintln!("[SharedNode] Node PID: {pid}, Reference count: {ref_count}");

    Ok(SharedNodeHandle {
        rpc_url: rpc_url(),
        handle_id,
    })
}
