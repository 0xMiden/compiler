//! Process management functionality for the shared node

use std::{
    fs,
    net::TcpStream,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, anyhow};

use super::{RPC_PORT, data_dir, rpc_url, setup::LocalMidenNode};

/// Check if a port is in use
pub fn is_port_in_use(port: u16) -> bool {
    TcpStream::connect(("127.0.0.1", port)).is_ok()
}

/// Check if a process is running
pub fn is_process_running(pid: u32) -> bool {
    // Try to read from /proc/{pid}/stat on Linux/macOS
    #[cfg(target_os = "linux")]
    {
        std::path::Path::new(&format!("/proc/{pid}")).exists()
    }

    #[cfg(not(target_os = "linux"))]
    {
        // On macOS, use ps command
        Command::new("ps")
            .args(["-p", &pid.to_string()])
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false)
    }
}

/// Kill a process by PID
pub fn kill_process(pid: u32) -> Result<()> {
    eprintln!("[SharedNode] Killing process {pid}");

    // Use kill command for cross-platform compatibility
    // First try SIGTERM
    let term_result = Command::new("kill")
        .args(["-TERM", &pid.to_string()])
        .output()
        .context("Failed to execute kill command")?;

    if !term_result.status.success() {
        let stderr = String::from_utf8_lossy(&term_result.stderr);
        // If process doesn't exist, that's fine
        if stderr.contains("No such process") {
            return Ok(());
        }
        return Err(anyhow!("Failed to send SIGTERM to process {pid}: {stderr}"));
    }

    // Wait a bit for graceful shutdown
    thread::sleep(Duration::from_millis(500));

    // If still running, use SIGKILL
    if is_process_running(pid) {
        let kill_result = Command::new("kill")
            .args(["-KILL", &pid.to_string()])
            .output()
            .context("Failed to execute kill command")?;

        if !kill_result.status.success() {
            let stderr = String::from_utf8_lossy(&kill_result.stderr);
            if !stderr.contains("No such process") {
                return Err(anyhow!("Failed to send SIGKILL to process {pid}: {stderr}"));
            }
        }
    }

    Ok(())
}

/// Start the shared node process
pub async fn start_shared_node() -> Result<u32> {
    eprintln!("[SharedNode] Starting shared node process...");

    // Bootstrap if needed (data directory empty or doesn't exist)
    let data_dir_path = data_dir();
    let needs_bootstrap = !data_dir_path.exists()
        || fs::read_dir(&data_dir_path)
            .map(|mut entries| entries.next().is_none())
            .unwrap_or(true);

    if needs_bootstrap {
        // Ensure we have a clean, empty data directory for bootstrap
        if data_dir_path.exists() {
            fs::remove_dir_all(&data_dir_path).context("Failed to remove data directory")?;
        }
        fs::create_dir_all(&data_dir_path).context("Failed to create data directory")?;
        LocalMidenNode::bootstrap(&data_dir_path).context("Failed to bootstrap miden-node")?;
    }

    // Start the node process
    // Use Stdio::null() for stdout/stderr to avoid buffer blocking issues.
    // When pipes are used, the child process can block if the pipe buffer fills up
    // and the reading end doesn't consume data fast enough. Using inherit() also
    // causes issues with nextest's parallel test execution.
    //
    // For debugging, users can run the node manually with RUST_LOG=debug.
    let child = Command::new("miden-node")
        .args([
            "bundled",
            "start",
            "--data-directory",
            data_dir_path.to_str().unwrap(),
            "--rpc.url",
            &rpc_url(),
            "--block.interval",
            "1sec",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("Failed to start miden-node process")?;

    let pid = child.id();

    // Detach the child process so it continues running after we exit
    drop(child);

    // Wait for node to be ready
    eprintln!("[SharedNode] Waiting for node to be ready...");
    let start = Instant::now();
    let timeout = Duration::from_secs(10);

    while start.elapsed() < timeout {
        if is_port_in_use(RPC_PORT) {
            eprintln!("[SharedNode] Node is ready");
            return Ok(pid);
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    // If we get here, node failed to start
    kill_process(pid).context("Failed to kill unresponsive node process")?;
    Err(anyhow!("Timeout waiting for node to be ready"))
}
