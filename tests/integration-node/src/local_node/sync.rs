//! Synchronization and reference counting logic for the shared node

use std::{
    fs::{self, File, OpenOptions},
    thread,
    time::Duration,
};

use anyhow::{Context, Result, anyhow};
use fs2::FileExt;

use super::{
    COORD_DIR, lock_file, pid_file,
    process::{is_process_running, kill_process},
    ref_count_dir,
};

/// Lock guard using fs2 file locking
pub struct LockGuard(File);

impl Drop for LockGuard {
    fn drop(&mut self) {
        let _ = self.0.unlock();
    }
}

/// Acquire a file lock for atomic operations
pub fn acquire_lock() -> Result<LockGuard> {
    // Ensure coordination directory exists
    fs::create_dir_all(COORD_DIR).context("Failed to create coordination directory")?;

    // Open or create lock file
    let file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(lock_file())
        .context("Failed to open lock file")?;

    // Try to acquire exclusive lock with retries
    let mut attempts = 0;
    const MAX_ATTEMPTS: u32 = 100; // 10 seconds max wait

    loop {
        match file.try_lock_exclusive() {
            Ok(_) => return Ok(LockGuard(file)),
            Err(e) => {
                if attempts >= MAX_ATTEMPTS {
                    return Err(anyhow!("Timeout acquiring lock: {e}"));
                }
                attempts += 1;
                thread::sleep(Duration::from_millis(100));
            }
        }
    }
}

/// Read PID from file
pub fn read_pid() -> Result<Option<u32>> {
    match fs::read_to_string(pid_file()) {
        Ok(contents) => {
            let pid = contents.trim().parse::<u32>().context("Failed to parse PID")?;
            Ok(Some(pid))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(anyhow!("Failed to read PID file: {e}")),
    }
}

/// Write PID to file
pub fn write_pid(pid: u32) -> Result<()> {
    fs::write(pid_file(), pid.to_string()).context("Failed to write PID file")
}

/// Remove PID file
pub fn remove_pid() -> Result<()> {
    match fs::remove_file(pid_file()) {
        Ok(_) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(anyhow!("Failed to remove PID file: {e}")),
    }
}

/// Get count of active references, cleaning up stale ones
pub fn get_ref_count() -> Result<usize> {
    fs::create_dir_all(ref_count_dir()).context("Failed to create reference count directory")?;

    let entries =
        fs::read_dir(ref_count_dir()).context("Failed to read reference count directory")?;

    let mut active_count = 0;
    for entry in entries.flatten() {
        let file_name = entry.file_name();
        let file_name_str = file_name.to_string_lossy();

        // Extract PID from handle name (format: handle-{pid}-{uuid})
        if let Some(pid_str) = file_name_str.split('-').nth(1)
            && let Ok(pid) = pid_str.parse::<u32>()
        {
            if is_process_running(pid) {
                active_count += 1;
            } else {
                // Clean up stale reference from dead process
                eprintln!("[SharedNode] Cleaning up stale reference from dead process {pid}");
                let _ = fs::remove_file(entry.path());
            }
        }
    }

    Ok(active_count)
}

/// Add a reference
pub fn add_reference(handle_id: &str) -> Result<()> {
    fs::create_dir_all(ref_count_dir()).context("Failed to create reference count directory")?;

    let ref_file = ref_count_dir().join(handle_id);
    File::create(&ref_file).context("Failed to create reference file")?;

    Ok(())
}

/// Check and stop the node if no more references exist
pub fn stop_node_if_no_references() {
    // Acquire lock for atomic operation
    let _lock = match acquire_lock() {
        Ok(lock) => lock,
        Err(e) => {
            eprintln!("[SharedNode] Failed to acquire lock: {e}");
            return;
        }
    };

    // Check reference count
    let ref_count = match get_ref_count() {
        Ok(count) => count,
        Err(e) => {
            eprintln!("[SharedNode] Failed to get reference count: {e}");
            return;
        }
    };

    eprintln!("[SharedNode] Reference count: {ref_count}");

    if ref_count == 0 {
        // No more references, stop the node
        if let Ok(Some(pid)) = read_pid() {
            eprintln!("[SharedNode] No more references, stopping node process {pid}");

            if let Err(e) = kill_process(pid) {
                eprintln!("[SharedNode] Failed to kill node process: {e}");
            }

            if let Err(e) = remove_pid() {
                eprintln!("[SharedNode] Failed to remove PID file: {e}");
            }

            // Clean up coordination directory
            if let Err(e) = fs::remove_dir_all(COORD_DIR) {
                eprintln!("[SharedNode] Failed to clean up coordination directory: {e}");
            }
        }
    }
}
