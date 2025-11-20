//! Node installation and bootstrap functionality

use std::{fs, path::Path, process::Command};

use anyhow::{anyhow, Context, Result};

use super::{process::kill_process, sync::read_pid, COORD_DIR};

// Version configuration for miden-node
// NOTE: When updating miden-client version in Cargo.toml, update this constant to match
// the compatible miden-node version. Both should typically use the same major.minor version.

/// The exact miden-node version that is compatible with the miden-client version used in tests
const MIDEN_NODE_VERSION: &str = "0.12.2";

/// Manages the lifecycle of a local Miden node instance
pub struct LocalMidenNode;

impl LocalMidenNode {
    /// Install miden-node binary if not already installed
    pub fn ensure_installed() -> Result<()> {
        // Check if miden-node is already installed and get version
        let check = Command::new("miden-node").arg("--version").output();

        let need_install = match check {
            Ok(output) if output.status.success() => {
                let version = String::from_utf8_lossy(&output.stdout);
                let version_line = version.lines().next().unwrap_or("");

                // Check if it's the exact version we need
                if version_line.contains(MIDEN_NODE_VERSION) {
                    eprintln!("miden-node already installed: {version_line}");
                    false
                } else {
                    eprintln!(
                        "Found incompatible miden-node version: {version_line} (need \
                         {MIDEN_NODE_VERSION})"
                    );
                    eprintln!("Uninstalling current version...");

                    // Uninstall the current version
                    let uninstall_output = Command::new("cargo")
                        .args(["uninstall", "miden-node"])
                        .output()
                        .context("Failed to run cargo uninstall")?;

                    if !uninstall_output.status.success() {
                        let stderr = String::from_utf8_lossy(&uninstall_output.stderr);
                        eprintln!("Warning: Failed to uninstall miden-node: {stderr}");
                    } else {
                        eprintln!("Successfully uninstalled old version");
                    }

                    // Clean all node-related data when version changes
                    eprintln!("Cleaning node data due to version change...");

                    // Kill any running node process
                    if let Ok(Some(pid)) = read_pid() {
                        eprintln!("Stopping existing node process {pid}");
                        let _ = kill_process(pid);
                    }

                    // Clean the entire coordination directory
                    if let Err(e) = fs::remove_dir_all(COORD_DIR) {
                        if e.kind() != std::io::ErrorKind::NotFound {
                            eprintln!("Warning: Failed to clean coordination directory: {e}");
                        }
                    }

                    true
                }
            }
            _ => {
                eprintln!("miden-node not found");
                true
            }
        };

        if need_install {
            // Install specific version compatible with miden-client
            eprintln!("Installing miden-node version {MIDEN_NODE_VERSION} from crates.io...");
            let output = Command::new("cargo")
                .args(["install", "miden-node", "--version", MIDEN_NODE_VERSION, "--locked"])
                .output()
                .context("Failed to run cargo install")?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Err(anyhow!("Failed to install miden-node: {stderr}"));
            }

            eprintln!("miden-node {MIDEN_NODE_VERSION} installed successfully");
        }

        Ok(())
    }

    /// Bootstrap the node with genesis data
    pub fn bootstrap(data_dir: &Path) -> Result<()> {
        eprintln!("Bootstrapping miden-node...");

        let output = Command::new("miden-node")
            .args([
                "bundled",
                "bootstrap",
                "--data-directory",
                data_dir.to_str().unwrap(),
                "--accounts-directory",
                data_dir.to_str().unwrap(),
            ])
            .output()
            .context("Failed to run miden-node bootstrap command")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow!("Failed to bootstrap node: {stderr}"));
        }

        eprintln!("Node bootstrapped successfully");
        Ok(())
    }
}
