//! Infrastructure for running a local Miden node for integration tests

use std::{
    fs::{self, File, OpenOptions},
    io::BufRead,
    net::TcpStream,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use fs2::FileExt;

// Version configuration for miden-node
// When updating miden-client version in Cargo.toml, update this constant to match
// the compatible miden-node version. Both should typically use the same major.minor version.

/// The exact miden-node version that is compatible with the miden-client version used in tests
const MIDEN_NODE_VERSION: &str = "0.10.0";

/// Default RPC URL for the node
const RPC_URL: &str = "http://127.0.0.1:57291";

/// Port number for the node
const RPC_PORT: u16 = 57291;

/// Coordination directory for cross-process synchronization
const COORD_DIR: &str = "/tmp/miden-test-node";

/// PID file path
const PID_FILE: &str = "/tmp/miden-test-node/node.pid";

/// Reference count directory
const REF_COUNT_DIR: &str = "/tmp/miden-test-node/refs";

/// Lock file for atomic operations
const LOCK_FILE: &str = "/tmp/miden-test-node/node.lock";

/// Data directory for the shared node
const DATA_DIR: &str = "/tmp/miden-test-node/data";

/// Manages the lifecycle of a local Miden node instance
struct LocalMidenNode;

impl LocalMidenNode {
    /// Install miden-node binary if not already installed
    pub fn ensure_installed() -> Result<(), String> {
        // Check if miden-node is already installed and get version
        let check = Command::new("miden-node").arg("--version").output();

        let need_install = match check {
            Ok(output) if output.status.success() => {
                let version = String::from_utf8_lossy(&output.stdout);
                let version_line = version.lines().next().unwrap_or("");

                // Check if it's the exact version we need
                if version_line.contains(MIDEN_NODE_VERSION) {
                    eprintln!("miden-node already installed: {}", version_line);
                    false
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

                    // Clean all node-related data when version changes
                    eprintln!("Cleaning node data due to version change...");

                    // Kill any running node process
                    if let Ok(Some(pid)) = read_pid() {
                        eprintln!("Stopping existing node process {}", pid);
                        let _ = kill_process(pid);
                    }

                    // Clean the entire coordination directory
                    if let Err(e) = fs::remove_dir_all(COORD_DIR) {
                        if e.kind() != std::io::ErrorKind::NotFound {
                            eprintln!("Warning: Failed to clean coordination directory: {}", e);
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
        }

        Ok(())
    }

    /// Bootstrap the node with genesis data
    fn bootstrap(data_dir: &Path) -> Result<(), String> {
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
            .map_err(|e| format!("Failed to run bootstrap: {}", e))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("Failed to bootstrap node: {}", stderr));
        }

        eprintln!("Node bootstrapped successfully");
        Ok(())
    }

    /// Check if a port is in use
    fn is_port_in_use(port: u16) -> bool {
        TcpStream::connect(("127.0.0.1", port)).is_ok()
    }
}

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
        let ref_file = PathBuf::from(REF_COUNT_DIR).join(&self.handle_id);
        if let Err(e) = fs::remove_file(&ref_file) {
            eprintln!("[SharedNode] Warning: Failed to remove ref file: {}", e);
        }

        // Check if we're the last reference and should stop the node
        check_and_stop_node_if_needed();
    }
}

// Lock guard using fs2 file locking
struct LockGuard(File);

impl Drop for LockGuard {
    fn drop(&mut self) {
        let _ = self.0.unlock();
    }
}

/// Acquire a file lock for atomic operations
fn acquire_lock() -> Result<LockGuard, String> {
    // Ensure coordination directory exists
    fs::create_dir_all(COORD_DIR)
        .map_err(|e| format!("Failed to create coordination directory: {}", e))?;

    // Open or create lock file
    let file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(LOCK_FILE)
        .map_err(|e| format!("Failed to open lock file: {}", e))?;

    // Try to acquire exclusive lock with retries
    let mut attempts = 0;
    const MAX_ATTEMPTS: u32 = 100; // 10 seconds max wait

    loop {
        match file.try_lock_exclusive() {
            Ok(_) => return Ok(LockGuard(file)),
            Err(e) => {
                if attempts >= MAX_ATTEMPTS {
                    return Err(format!("Timeout acquiring lock: {}", e));
                }
                attempts += 1;
                thread::sleep(Duration::from_millis(100));
            }
        }
    }
}

/// Read PID from file
fn read_pid() -> Result<Option<u32>, String> {
    match fs::read_to_string(PID_FILE) {
        Ok(contents) => {
            let pid = contents
                .trim()
                .parse::<u32>()
                .map_err(|e| format!("Failed to parse PID: {}", e))?;
            Ok(Some(pid))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("Failed to read PID file: {}", e)),
    }
}

/// Write PID to file
fn write_pid(pid: u32) -> Result<(), String> {
    fs::write(PID_FILE, pid.to_string()).map_err(|e| format!("Failed to write PID file: {}", e))
}

/// Remove PID file
fn remove_pid() -> Result<(), String> {
    match fs::remove_file(PID_FILE) {
        Ok(_) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("Failed to remove PID file: {}", e)),
    }
}

/// Check if a process is running
fn is_process_running(pid: u32) -> bool {
    // Try to read from /proc/{pid}/stat on Linux/macOS
    #[cfg(target_os = "linux")]
    {
        std::path::Path::new(&format!("/proc/{}", pid)).exists()
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
fn kill_process(pid: u32) -> Result<(), String> {
    eprintln!("[SharedNode] Killing process {}", pid);

    // Use kill command for cross-platform compatibility
    // First try SIGTERM
    let term_result = Command::new("kill")
        .args(["-TERM", &pid.to_string()])
        .output()
        .map_err(|e| format!("Failed to execute kill command: {}", e))?;

    if !term_result.status.success() {
        let stderr = String::from_utf8_lossy(&term_result.stderr);
        // If process doesn't exist, that's fine
        if stderr.contains("No such process") {
            return Ok(());
        }
        return Err(format!("Failed to send SIGTERM to process {}: {}", pid, stderr));
    }

    // Wait a bit for graceful shutdown
    thread::sleep(Duration::from_millis(500));

    // If still running, use SIGKILL
    if is_process_running(pid) {
        let kill_result = Command::new("kill")
            .args(["-KILL", &pid.to_string()])
            .output()
            .map_err(|e| format!("Failed to execute kill command: {}", e))?;

        if !kill_result.status.success() {
            let stderr = String::from_utf8_lossy(&kill_result.stderr);
            if !stderr.contains("No such process") {
                return Err(format!("Failed to send SIGKILL to process {}: {}", pid, stderr));
            }
        }
    }

    Ok(())
}

/// Get count of active references, cleaning up stale ones
fn get_ref_count() -> Result<usize, String> {
    fs::create_dir_all(REF_COUNT_DIR)
        .map_err(|e| format!("Failed to create ref count directory: {}", e))?;

    let entries = fs::read_dir(REF_COUNT_DIR)
        .map_err(|e| format!("Failed to read ref count directory: {}", e))?;

    let mut active_count = 0;
    for entry in entries.flatten() {
        let file_name = entry.file_name();
        let file_name_str = file_name.to_string_lossy();

        // Extract PID from handle name (format: handle-{pid}-{uuid})
        if let Some(pid_str) = file_name_str.split('-').nth(1) {
            if let Ok(pid) = pid_str.parse::<u32>() {
                if is_process_running(pid) {
                    active_count += 1;
                } else {
                    // Clean up stale reference from dead process
                    eprintln!("[SharedNode] Cleaning up stale reference from dead process {}", pid);
                    let _ = fs::remove_file(entry.path());
                }
            }
        }
    }

    Ok(active_count)
}

/// Add a reference
fn add_reference(handle_id: &str) -> Result<(), String> {
    fs::create_dir_all(REF_COUNT_DIR)
        .map_err(|e| format!("Failed to create ref count directory: {}", e))?;

    let ref_file = PathBuf::from(REF_COUNT_DIR).join(handle_id);
    File::create(&ref_file).map_err(|e| format!("Failed to create reference file: {}", e))?;

    Ok(())
}

/// Start the shared node process
async fn start_shared_node() -> Result<u32, String> {
    eprintln!("[SharedNode] Starting shared node process...");

    // Ensure data directory exists
    fs::create_dir_all(DATA_DIR).map_err(|e| format!("Failed to create data directory: {}", e))?;

    // Bootstrap if needed
    let marker_file = PathBuf::from(DATA_DIR).join(".bootstrapped");
    if !marker_file.exists() {
        LocalMidenNode::bootstrap(Path::new(DATA_DIR))?;
        // Create marker file
        File::create(&marker_file).map_err(|e| format!("Failed to create marker file: {}", e))?;
    }

    // Start the node process
    let mut child = Command::new("miden-node")
        .args([
            "bundled",
            "start",
            "--data-directory",
            DATA_DIR,
            "--rpc.url",
            RPC_URL,
            "--block.interval",
            "1sec",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed to start node: {}", e))?;

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
                eprintln!("[shared node stdout] {}", line);
            }
        }
    });

    thread::spawn(move || {
        let reader = std::io::BufReader::new(stderr);
        for line in reader.lines().map_while(Result::ok) {
            eprintln!("[shared node stderr] {}", line);
        }
    });

    // Detach the child process so it continues running after we exit
    drop(child);

    // Wait for node to be ready
    eprintln!("[SharedNode] Waiting for node to be ready...");
    let start = Instant::now();
    let timeout = Duration::from_secs(10);

    while start.elapsed() < timeout {
        if LocalMidenNode::is_port_in_use(RPC_PORT) {
            eprintln!("[SharedNode] Node is ready");
            return Ok(pid);
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    // If we get here, node failed to start
    kill_process(pid)?;
    Err("Timeout waiting for node to be ready".to_string())
}

/// Check and stop the node if no more references exist
fn check_and_stop_node_if_needed() {
    // Acquire lock for atomic operation
    let _lock = match acquire_lock() {
        Ok(lock) => lock,
        Err(e) => {
            eprintln!("[SharedNode] Failed to acquire lock: {}", e);
            return;
        }
    };

    // Check reference count
    let ref_count = match get_ref_count() {
        Ok(count) => count,
        Err(e) => {
            eprintln!("[SharedNode] Failed to get reference count: {}", e);
            return;
        }
    };

    eprintln!("[SharedNode] Reference count: {}", ref_count);

    if ref_count == 0 {
        // No more references, stop the node
        if let Ok(Some(pid)) = read_pid() {
            eprintln!("[SharedNode] No more references, stopping node process {}", pid);

            if let Err(e) = kill_process(pid) {
                eprintln!("[SharedNode] Failed to kill node process: {}", e);
            }

            if let Err(e) = remove_pid() {
                eprintln!("[SharedNode] Failed to remove PID file: {}", e);
            }

            // Clean up coordination directory
            if let Err(e) = fs::remove_dir_all(COORD_DIR) {
                eprintln!("[SharedNode] Failed to clean up coordination directory: {}", e);
            }
        }
    }
}

/// Get a handle to the shared node instance. Starts the node if it's not running.
pub async fn get_shared_node() -> Result<SharedNodeHandle, String> {
    // Ensure miden-node is installed
    LocalMidenNode::ensure_installed()?;

    // Generate unique handle ID
    let handle_id = format!("handle-{}-{}", std::process::id(), uuid::Uuid::new_v4());

    // Acquire lock for atomic operation
    let _lock = acquire_lock()?;

    // Clean up any stale references first
    let _ = get_ref_count()?;

    // Add our reference first
    add_reference(&handle_id)?;
    let ref_count = get_ref_count()?;
    eprintln!("[SharedNode] Added reference {}, total count: {}", handle_id, ref_count);

    // Check if node is already running
    let need_start = if let Some(pid) = read_pid()? {
        // Check if the process is still alive
        if is_process_running(pid) && LocalMidenNode::is_port_in_use(RPC_PORT) {
            eprintln!("[SharedNode] Node already running with PID {}", pid);
            false
        } else {
            eprintln!("[SharedNode] PID file exists but process {} is not running", pid);
            remove_pid()?;
            true
        }
    } else {
        eprintln!("[SharedNode] No PID file found, need to start node");
        true
    };

    // Start node if needed - keep lock held to prevent race conditions
    if need_start {
        let pid = start_shared_node().await?;
        write_pid(pid)?;
        eprintln!("[SharedNode] Started node with PID {}", pid);
    }

    // Lock is automatically released when _lock is dropped

    Ok(SharedNodeHandle {
        rpc_url: RPC_URL.to_string(),
        handle_id,
    })
}
