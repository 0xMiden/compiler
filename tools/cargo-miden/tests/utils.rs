use std::{
    env, fs,
    io::ErrorKind,
    path::{Path, PathBuf},
    sync::{Mutex, MutexGuard, OnceLock},
    thread,
    time::Duration,
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

pub(crate) fn project_template_arg(template: &str) -> String {
    let template = template.trim_start_matches("--");
    let templates_path = match env::var("TEST_LOCAL_TEMPLATES_PATH") {
        Ok(path) => PathBuf::from(path),
        Err(_) => cached_rust_templates_path().expect("failed to prepare rust-templates cache"),
    };
    format!("--template-path={}", templates_path.join(template).display())
}

fn cached_rust_templates_path() -> anyhow::Result<PathBuf> {
    let cache_root = env::temp_dir().join("cargo_miden_local_rust_templates_v0_30_0_features");
    let ready_marker = cache_root.join(".ready");
    if templates_cache_is_ready(&cache_root, &ready_marker) {
        return Ok(cache_root);
    }

    let lock_dir = cache_root.with_extension("lock");
    loop {
        match fs::create_dir(&lock_dir) {
            Ok(()) => break,
            Err(err) if err.kind() == ErrorKind::AlreadyExists => {
                if templates_cache_is_ready(&cache_root, &ready_marker) {
                    return Ok(cache_root);
                }
                thread::sleep(Duration::from_millis(100));
            }
            Err(err) => return Err(err.into()),
        }
    }

    let _lock = LockDir { path: lock_dir };
    if templates_cache_is_ready(&cache_root, &ready_marker) {
        return Ok(cache_root);
    }

    if cache_root.exists() {
        fs::remove_dir_all(&cache_root)?;
    }

    write_local_test_templates(&cache_root)?;
    fs::write(&ready_marker, "local")?;
    Ok(cache_root)
}

fn templates_cache_is_ready(cache_root: &Path, ready_marker: &Path) -> bool {
    ready_marker.exists()
        && ["account", "auth-component", "note", "program", "tx-script"]
            .iter()
            .all(|template| cache_root.join(template).is_dir())
}

fn write_local_test_templates(cache_root: &Path) -> anyhow::Result<()> {
    write_template(
        cache_root,
        "account",
        cargo_toml("account", true),
        r#"#![no_std]
#![feature(alloc_error_handler)]

use miden::{component, felt, Felt};

#[component]
struct TestAccount;

#[component]
impl TestAccount {
    pub fn value(&self) -> Felt {
        felt!(1)
    }
}
"#,
    )?;
    write_template(
        cache_root,
        "auth-component",
        cargo_toml("authentication-component", true),
        r#"#![no_std]
#![feature(alloc_error_handler)]

use miden::{component, Word};

#[component]
struct AuthComponent;

#[component]
impl AuthComponent {
    #[auth_script]
    pub fn auth(&mut self, _arg: Word) {}
}
"#,
    )?;
    write_template(
        cache_root,
        "note",
        cargo_toml("note-script", true),
        r#"#![no_std]
#![feature(alloc_error_handler)]

use miden::{note, Word};

#[note]
struct TestNote;

#[note]
impl TestNote {
    #[note_script]
    pub fn run(self, _arg: Word) {}
}
"#,
    )?;
    write_template(
        cache_root,
        "tx-script",
        cargo_toml("transaction-script", true),
        r#"#![no_std]
#![feature(alloc_error_handler)]

use miden::{tx_script, Word};

#[tx_script]
fn run(_arg: Word) {}
"#,
    )?;
    write_template(
        cache_root,
        "program",
        cargo_toml("program", false),
        r#"#![no_std]
#![feature(alloc_error_handler)]

#[cfg(not(test))]
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}

#[cfg(not(test))]
#[alloc_error_handler]
fn alloc_error(_layout: core::alloc::Layout) -> ! {
    loop {}
}

#[unsafe(no_mangle)]
pub fn entrypoint(value: u32) -> u32 {
    value + 1
}
"#,
    )?;

    Ok(())
}

fn cargo_toml(project_kind: &str, component: bool) -> String {
    let component_metadata = if component {
        r#"
[package.metadata.component]
package = "miden:test-template"
"#
    } else {
        ""
    };
    let supported_types = match project_kind {
        "account" | "authentication-component" => {
            r#"supported-types = ["RegularAccountUpdatableCode"]
"#
        }
        _ => "",
    };

    format!(
        r#"[package]
name = "{{{{crate_name}}}}"
version = "0.1.0"
edition = "2024"

[lib]
crate-type = ["cdylib"]

[dependencies]
miden = {{ path = "{{{{compiler_path}}}}/sdk/sdk" }}
{component_metadata}
[package.metadata.miden]
project-kind = "{project_kind}"
{supported_types}
[profile.release]
panic = "abort"

[profile.dev]
panic = "abort"
"#
    )
}

fn write_template(
    cache_root: &Path,
    template: &str,
    cargo_toml: String,
    lib_rs: &str,
) -> anyhow::Result<()> {
    let template_root = cache_root.join(template);
    fs::create_dir_all(template_root.join("src"))?;
    fs::write(template_root.join("Cargo.toml"), cargo_toml)?;
    fs::write(template_root.join("src/lib.rs"), lib_rs)?;
    fs::copy(workspace_root().join("Cargo.lock"), template_root.join("Cargo.lock"))?;
    Ok(())
}

pub(crate) fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("cargo-miden should live under tools/cargo-miden")
        .to_path_buf()
}

struct LockDir {
    path: PathBuf,
}

impl Drop for LockDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir(&self.path);
    }
}
