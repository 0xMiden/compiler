//! Cross-process fixture cache, scoped to one nextest invocation.

use std::{
    fs::{self, OpenOptions},
    io,
    path::{Path, PathBuf},
};

pub(crate) fn nextest_cache_dir(target: &Path) -> io::Result<Option<PathBuf>> {
    std::env::var_os("NEXTEST_RUN_ID")
        .map(|run| cache_dir(target, &run))
        .transpose()
}

fn cache_dir(target: &Path, run: &std::ffi::OsStr) -> io::Result<PathBuf> {
    // Preserve the entire run identity without hashing or permitting path traversal.
    let mut components = Path::new(run).components();
    if !matches!(components.next(), Some(std::path::Component::Normal(_)))
        || components.next().is_some()
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "nextest run ID must be one path component",
        ));
    }
    Ok(target.join("nextest").join(run).join("swapp"))
}

/// Publish a complete payload under an exclusive process lock. A panic or killed
/// producer releases the lock without publishing a partially-written fixture.
pub(crate) fn load_or_build(
    directory: &Path,
    build: impl FnOnce() -> Vec<u8>,
) -> io::Result<Vec<u8>> {
    fs::create_dir_all(directory)?;
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(directory.join("lock"))?;
    lock.lock()?;
    let payload = directory.join("packages");
    match fs::read(&payload) {
        Ok(bytes) => return Ok(bytes),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    let bytes = build();
    let pending = directory.join("packages.pending");
    fs::write(&pending, &bytes)?;
    fs::rename(pending, payload)?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use std::{
        io::Write,
        process::Command,
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::*;

    #[test]
    fn cache_identity_preserves_full_run_id_and_rejects_paths() {
        let target = Path::new("/workspace/target");
        let first = cache_dir(target, "run-one".as_ref()).unwrap();
        assert_eq!(first, target.join("nextest/run-one/swapp"));
        assert_ne!(first, cache_dir(target, "run-two".as_ref()).unwrap());
        for invalid in ["", ".", "..", "../other", "/absolute", "one/two"] {
            assert!(cache_dir(target, invalid.as_ref()).is_err(), "{invalid}");
        }
    }

    #[test]
    fn cache_child() {
        let Some(directory) = std::env::var_os("MIDENC_CACHE_TEST_DIR") else {
            return;
        };
        let directory = PathBuf::from(directory);
        let bytes = load_or_build(&directory, || {
            let mut counter = OpenOptions::new()
                .create(true)
                .append(true)
                .open(directory.join("compiles"))
                .unwrap();
            // Stand-ins for wallet then note compilation, observable across processes.
            counter.write_all(b"wallet\nnote\n").unwrap();
            std::thread::sleep(std::time::Duration::from_millis(20));
            b"compiled wallet and note".to_vec()
        })
        .unwrap();
        assert_eq!(bytes, b"compiled wallet and note");
    }

    #[test]
    fn ten_processes_compile_one_fixture_pair() {
        let directory = std::env::temp_dir().join(format!(
            "midenc-cache-test-{}-{}",
            std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()
        ));
        let mut children = (0..10)
            .map(|_| {
                Command::new(std::env::current_exe().unwrap())
                    .arg("cache_child")
                    .env("MIDENC_CACHE_TEST_DIR", &directory)
                    .stdout(std::process::Stdio::null())
                    .spawn()
                    .unwrap()
            })
            .collect::<Vec<_>>();
        for child in &mut children {
            assert!(child.wait().unwrap().success());
        }
        assert_eq!(fs::read_to_string(directory.join("compiles")).unwrap(), "wallet\nnote\n");
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn failed_producer_releases_lock_without_publishing() {
        let directory =
            std::env::temp_dir().join(format!("midenc-cache-failure-{}", std::process::id()));
        let _ = fs::remove_dir_all(&directory);
        assert!(
            std::panic::catch_unwind(|| load_or_build(&directory, || panic!("compilation failed")))
                .is_err()
        );
        assert!(!directory.join("packages").exists());
        assert_eq!(load_or_build(&directory, || b"retry".to_vec()).unwrap(), b"retry");
        fs::remove_dir_all(directory).unwrap();
    }
}
