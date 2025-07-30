//! Process management functionality for the shared node

use std::{
    fs::{self, File},
    io::BufRead,
    net::TcpStream,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use anyhow::{anyhow, Context, Result};

use super::{data_dir, rpc_url, setup::LocalMidenNode, RPC_PORT};

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

    // Ensure data directory exists
    let data_dir_path = data_dir();
    fs::create_dir_all(&data_dir_path).context("Failed to create data directory")?;

    // Bootstrap if needed
    let marker_file = data_dir_path.join(".bootstrapped");
    if !marker_file.exists() {
        LocalMidenNode::bootstrap(&data_dir_path).context("Failed to bootstrap miden-node")?;
        // Create marker file
        File::create(&marker_file).context("Failed to create bootstrap marker file")?;
    }

    // Start the node process
    let mut child = Command::new("miden-node")
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
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("Failed to start miden-node process")?;

    let pid = child.id();

    // Capture output for debugging
    let stdout = child.stdout.take().expect("Failed to capture stdout");
    let stderr = child.stderr.take().expect("Failed to capture stderr");

    // Check if node output logging is enabled
    let enable_node_output = std::env::var("MIDEN_NODE_OUTPUT").unwrap_or_default() == "1";

    // Spawn threads to read output
    thread::spawn(move || {
        let reader = std::io::BufReader::new(stdout);
        for line in reader.lines().map_while(Result::ok) {
            if enable_node_output {
                eprintln!("[shared node stdout] {line}");
            }
        }
    });

    thread::spawn(move || {
        let reader = std::io::BufReader::new(stderr);
        for line in reader.lines().map_while(Result::ok) {
            eprintln!("[shared node stderr] {line}");
        }
    });

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
