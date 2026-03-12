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

/// Serializes tests that mutate the process working directory.
pub(crate) fn current_dir_lock() -> MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(())).lock().unwrap()
}
