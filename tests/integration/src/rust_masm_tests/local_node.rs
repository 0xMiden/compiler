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
        // Check if miden-node is already installed
        let check = Command::new("miden-node").arg("--version").output();

        match check {
            Ok(output) if output.status.success() => {
                let version = String::from_utf8_lossy(&output.stdout);
                eprintln!("miden-node already installed: {}", version.trim());
                Ok(())
            }
            _ => {
                eprintln!("Installing miden-node from crates.io...");
                let output = Command::new("cargo")
                    .args(["install", "miden-node", "--locked"])
                    .output()
                    .map_err(|e| format!("Failed to run cargo install: {}", e))?;

                if !output.status.success() {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    return Err(format!("Failed to install miden-node: {}", stderr));
                }

                eprintln!("miden-node installed successfully");
                Ok(())
            }
        }
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

        // Spawn threads to read and print output
        thread::spawn(move || {
            let reader = std::io::BufReader::new(stdout);
            for line in reader.lines().map_while(Result::ok) {
                eprintln!("[node stdout] {}", line);
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

/// Create an isolated node instance for tests
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
}
