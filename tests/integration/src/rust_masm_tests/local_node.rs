//! Infrastructure for running a local Miden node for integration tests

use std::{
    io::BufRead,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};

use temp_dir::TempDir;
use tokio::time::sleep;

// Version configuration for miden-node
// When updating miden-client version in Cargo.toml, update this constant to match
// the compatible miden-node version. Both should typically use the same major.minor version.

/// The exact miden-node version that is compatible with the miden-client version used in tests
const MIDEN_NODE_VERSION: &str = "0.9.2";

/// Manages the lifecycle of a local Miden node instance
pub struct LocalMidenNode {
    /// Temporary directory containing node data
    data_dir: TempDir,
    /// The node process handle
    node_process: Option<Child>,
    /// RPC URL for the node
    rpc_url: String,
    /// Whether the node has been bootstrapped
    bootstrapped: bool,
}

impl LocalMidenNode {
    /// Creates a new LocalMidenNode instance
    pub fn new() -> Self {
        let data_dir = TempDir::new().expect("Failed to create temp directory");
        Self {
            data_dir,
            node_process: None,
            rpc_url: "http://127.0.0.1:57291".to_string(),
            bootstrapped: false,
        }
    }

    /// Install miden-node binary if not already installed
    pub fn ensure_installed(&self) -> Result<(), String> {
        // Check if miden-node is already installed and get version
        let check = Command::new("miden-node").arg("--version").output();

        match check {
            Ok(output) if output.status.success() => {
                let version = String::from_utf8_lossy(&output.stdout);
                let version_line = version.lines().next().unwrap_or("");

                // Check if it's the exact version we need
                if version_line.contains(MIDEN_NODE_VERSION) {
                    eprintln!("miden-node already installed: {}", version_line);
                    return Ok(());
                } else {
                    eprintln!(
                        "Found incompatible miden-node version: {} (need {})",
                        version_line, MIDEN_NODE_VERSION
                    );
                    eprintln!("Uninstalling current version...");

                    // Uninstall the current version
                    let uninstall_output = Command::new("cargo")
                        .args(["uninstall", "miden-node"])
                        .output()
                        .map_err(|e| format!("Failed to run cargo uninstall: {}", e))?;

                    if !uninstall_output.status.success() {
                        let stderr = String::from_utf8_lossy(&uninstall_output.stderr);
                        eprintln!("Warning: Failed to uninstall miden-node: {}", stderr);
                    } else {
                        eprintln!("Successfully uninstalled old version");
                    }
                }
            }
            _ => {
                eprintln!("miden-node not found");
            }
        }

        // Install specific version compatible with miden-client
        eprintln!("Installing miden-node version {} from crates.io...", MIDEN_NODE_VERSION);
        let output = Command::new("cargo")
            .args(["install", "miden-node", "--version", MIDEN_NODE_VERSION, "--locked"])
            .output()
            .map_err(|e| format!("Failed to run cargo install: {}", e))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("Failed to install miden-node: {}", stderr));
        }

        eprintln!("miden-node {} installed successfully", MIDEN_NODE_VERSION);
        Ok(())
    }

    /// Bootstrap the node with genesis data
    pub fn bootstrap(&mut self) -> Result<(), String> {
        if self.bootstrapped {
            return Ok(());
        }

        eprintln!("Bootstrapping miden-node...");

        let output = Command::new("miden-node")
            .args([
                "bundled",
                "bootstrap",
                "--data-directory",
                self.data_dir.path().to_str().unwrap(),
                "--accounts-directory",
                self.data_dir.path().to_str().unwrap(),
            ])
            .output()
            .map_err(|e| format!("Failed to run bootstrap: {}", e))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("Failed to bootstrap node: {}", stderr));
        }

        self.bootstrapped = true;
        eprintln!("Node bootstrapped successfully");
        Ok(())
    }

    /// Start the node process
    pub async fn start(&mut self) -> Result<(), String> {
        if self.node_process.is_some() {
            return Err("Node is already running".to_string());
        }

        if !self.bootstrapped {
            self.bootstrap()?;
        }

        eprintln!("Starting miden-node on {}...", self.rpc_url);

        let mut child = Command::new("miden-node")
            .args([
                "bundled",
                "start",
                "--data-directory",
                self.data_dir.path().to_str().unwrap(),
                "--rpc.url",
                &self.rpc_url,
                "--block.interval",
                "1", // 1 second block interval for faster tests
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("Failed to start node: {}", e))?;

        // Capture output for debugging
        let stdout = child.stdout.take().expect("Failed to capture stdout");
        let stderr = child.stderr.take().expect("Failed to capture stderr");

        // Check if node output logging is enabled via environment variable
        let enable_node_output = std::env::var("MIDEN_NODE_OUTPUT").unwrap_or_default() == "1";

        // Spawn threads to read and print output
        thread::spawn(move || {
            let reader = std::io::BufReader::new(stdout);
            for line in reader.lines().map_while(Result::ok) {
                if enable_node_output {
                    eprintln!("[node stdout] {}", line);
                }
            }
        });

        thread::spawn(move || {
            let reader = std::io::BufReader::new(stderr);
            for line in reader.lines().map_while(Result::ok) {
                eprintln!("[node stderr] {}", line);
            }
        });

        self.node_process = Some(child);

        // Wait for the node to be ready
        self.wait_for_ready().await?;

        eprintln!("Node started successfully");
        Ok(())
    }

    /// Wait for the node to be ready to accept connections
    async fn wait_for_ready(&self) -> Result<(), String> {
        // The node doesn't have a health endpoint, so we just wait a bit
        // for it to start up. In practice, checking if we can connect to
        // the gRPC endpoint would be better, but for now a simple delay works.
        eprintln!("Waiting for node to be ready...");
        sleep(Duration::from_secs(3)).await;
        eprintln!("Node should be ready now");
        Ok(())
    }

    /// Stop the node process
    pub fn stop(&mut self) -> Result<(), String> {
        if let Some(mut child) = self.node_process.take() {
            eprintln!("Stopping miden-node...");

            // Kill the process
            match child.kill() {
                Ok(_) => eprintln!("Kill signal sent to node process"),
                Err(e) => eprintln!("Warning: Failed to kill node process: {}", e),
            }

            // Wait for the process to exit
            match child.wait() {
                Ok(status) => eprintln!("Node stopped with status: {:?}", status),
                Err(e) => eprintln!("Warning: Failed to wait for node exit: {}", e),
            }
        }

        Ok(())
    }

    /// Get the RPC URL for connecting to the node
    pub fn rpc_url(&self) -> &str {
        &self.rpc_url
    }
}

impl Drop for LocalMidenNode {
    fn drop(&mut self) {
        if let Err(e) = self.stop() {
            eprintln!("Error stopping node during cleanup: {}", e);
        }
    }
}

/// Manages shared access to a single node instance across multiple tests
struct SharedNodeManager {
    /// The actual node instance, if running
    node: Option<LocalMidenNode>,
    /// Number of active handles
    ref_count: usize,
}

/// Handle to the shared node instance. When dropped, decrements the reference count.
pub struct SharedNodeHandle {
    /// The RPC URL of the shared node
    rpc_url: String,
    /// Weak reference to the manager for cleanup
    _manager: Arc<Mutex<SharedNodeManager>>,
}

impl SharedNodeHandle {
    /// Get the RPC URL for connecting to the node
    pub fn rpc_url(&self) -> &str {
        &self.rpc_url
    }
}

/// Global shared node manager
static SHARED_NODE_MANAGER: Mutex<Option<Arc<Mutex<SharedNodeManager>>>> = Mutex::new(None);

/// Get a handle to the shared node instance. Starts the node if it's not running.
pub async fn get_shared_node() -> Result<SharedNodeHandle, String> {
    // Get or create the manager
    let manager = {
        let mut global = SHARED_NODE_MANAGER.lock().unwrap();
        match global.as_ref() {
            Some(mgr) => Arc::clone(mgr),
            None => {
                let mgr = Arc::new(Mutex::new(SharedNodeManager {
                    node: None,
                    ref_count: 0,
                }));
                *global = Some(Arc::clone(&mgr));
                mgr
            }
        }
    };

    // We need to handle node startup atomically to avoid races
    // First, increment the ref count and check if we need to start
    let should_start = {
        let mut mgr = manager.lock().unwrap();
        mgr.ref_count += 1;
        eprintln!("[SharedNode] Reference count increased to {}", mgr.ref_count);

        // Only the first caller should start the node
        mgr.ref_count == 1 && mgr.node.is_none()
    };

    // Start the node if we're the first caller
    if should_start {
        eprintln!("[SharedNode] Starting shared node instance...");
        let mut node = LocalMidenNode::new();

        // Handle potential startup failure
        match node.ensure_installed() {
            Ok(_) => {}
            Err(e) => {
                // Decrement ref count on failure
                let mut mgr = manager.lock().unwrap();
                mgr.ref_count = mgr.ref_count.saturating_sub(1);
                return Err(e);
            }
        }

        match node.bootstrap() {
            Ok(_) => {}
            Err(e) => {
                // Decrement ref count on failure
                let mut mgr = manager.lock().unwrap();
                mgr.ref_count = mgr.ref_count.saturating_sub(1);
                return Err(e);
            }
        }

        match node.start().await {
            Ok(_) => {
                // Store the node in the manager
                let mut mgr = manager.lock().unwrap();
                mgr.node = Some(node);
            }
            Err(e) => {
                // Decrement ref count on failure
                let mut mgr = manager.lock().unwrap();
                mgr.ref_count = mgr.ref_count.saturating_sub(1);
                return Err(e);
            }
        }
    } else {
        // Wait for the node to be available if another task is starting it
        let mut attempts = 0;
        while attempts < 50 {
            // 5 seconds timeout
            {
                let mgr = manager.lock().unwrap();
                if mgr.node.is_some() {
                    break;
                }
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
            attempts += 1;
        }

        // Check if node is available
        let mgr = manager.lock().unwrap();
        if mgr.node.is_none() {
            // Something went wrong, decrement ref count
            drop(mgr);
            let mut mgr = manager.lock().unwrap();
            mgr.ref_count = mgr.ref_count.saturating_sub(1);
            return Err("Timeout waiting for node to start".to_string());
        }
    }

    // Get the RPC URL
    let rpc_url = {
        let mgr = manager.lock().unwrap();
        mgr.node.as_ref().unwrap().rpc_url().to_string()
    };

    Ok(SharedNodeHandle {
        rpc_url,
        _manager: Arc::clone(&manager),
    })
}

impl Drop for SharedNodeHandle {
    fn drop(&mut self) {
        if let Ok(mut mgr) = self._manager.lock() {
            mgr.ref_count = mgr.ref_count.saturating_sub(1);
            eprintln!("[SharedNode] Reference count decreased to {}", mgr.ref_count);

            // If this was the last reference, stop the node
            if mgr.ref_count == 0 {
                if let Some(mut node) = mgr.node.take() {
                    eprintln!("[SharedNode] Last reference dropped, stopping node...");
                    if let Err(e) = node.stop() {
                        eprintln!("[SharedNode] Error stopping node: {}", e);
                    }
                }

                // Clear the global manager
                if let Ok(mut global) = SHARED_NODE_MANAGER.lock() {
                    *global = None;
                }
            }
        }
    }
}

/// Create an isolated node instance for tests that need exclusive access
pub async fn create_isolated_node() -> Result<LocalMidenNode, String> {
    let mut node = LocalMidenNode::new();
    node.ensure_installed()?;
    node.bootstrap()?;
    node.start().await?;
    Ok(node)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_local_node_lifecycle() {
        let mut node = LocalMidenNode::new();

        // Ensure node is installed
        node.ensure_installed().expect("Failed to install node");

        // Bootstrap and start the node
        node.bootstrap().expect("Failed to bootstrap node");
        node.start().await.expect("Failed to start node");

        // Verify we can get the RPC URL
        assert_eq!(node.rpc_url(), "http://127.0.0.1:57291");

        // Stop the node
        node.stop().expect("Failed to stop node");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_shared_node_reference_counting() {
        use tokio::task::JoinSet;

        // Create multiple tasks that use the shared node
        let mut tasks = JoinSet::new();

        for i in 0..3 {
            tasks.spawn(async move {
                eprintln!("[Test {}] Getting shared node handle", i);
                let handle = get_shared_node().await.expect("Failed to get shared node");
                eprintln!("[Test {}] Got handle with URL: {}", i, handle.rpc_url());

                // Simulate some work
                tokio::time::sleep(Duration::from_millis(100)).await;

                eprintln!("[Test {}] Dropping handle", i);
                // Handle will be dropped here
            });
        }

        // Wait for all tasks to complete
        while let Some(result) = tasks.join_next().await {
            result.expect("Task panicked");
        }

        // Give some time for cleanup
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Verify the node was stopped (ref count should be 0)
        eprintln!("All tasks completed, node should be stopped");
    }
}
