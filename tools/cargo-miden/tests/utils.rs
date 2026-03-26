use std::{
    env,
    path::PathBuf,
    sync::{Mutex, MutexGuard, OnceLock},
};

#[allow(dead_code)]
pub(crate) fn get_test_path(test_dir_name: &str) -> PathBuf {
    let mut test_dir =
        PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set"));
    test_dir.push("tests");
    test_dir.push("data");
    test_dir.push(test_dir_name);
    test_dir
}

/// A guard that serializes cwd-mutating tests and restores the original cwd on drop.
pub(crate) struct CurrentDirGuard {
    guard: MutexGuard<'static, ()>,
    original_dir: PathBuf,
}

impl Drop for CurrentDirGuard {
    fn drop(&mut self) {
        let _ = env::set_current_dir(&self.original_dir);
        let _ = &self.guard;
    }
}

/// Serializes tests that mutate the process working directory.
pub(crate) fn current_dir_lock() -> CurrentDirGuard {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    let guard = LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let original_dir = env::current_dir().expect("current working directory should be available");
    CurrentDirGuard {
        guard,
        original_dir,
    }
}
