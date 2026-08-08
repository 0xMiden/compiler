//! Helpers for the project template integration tests.
//!
//! [`build_new_project_from_template`] scaffolds a fresh project from a local
//! template (supplied via `--template-path`) using the `cargo-miden` library
//! built from this checkout, then builds the project for both the `dev` and
//! `release` profiles.
//!
//! Because the template is supplied by path, these exercise the *templates*
//! and not template resolution. What `cargo miden new` renders by default is
//! covered by `tools/cargo-miden/tests/templates_from_bundle.rs`. The `note` and `tx-script` templates
//! additionally depend on a sibling account contract, which is generated and
//! built first.
//!
//! The `#[test]` entry points live in `tests/templates.rs`.

use std::{
    env, fs,
    path::{Path, PathBuf},
    sync::{Mutex, MutexGuard, OnceLock},
    time::{SystemTime, UNIX_EPOCH},
};

use cargo_miden::{CommandOutput, run};

/// Guard that serializes the cwd-mutating tests and restores the original
/// working directory when dropped.
struct CurrentDirGuard {
    _lock: MutexGuard<'static, ()>,
    original_dir: PathBuf,
}

impl Drop for CurrentDirGuard {
    fn drop(&mut self) {
        let _ = env::set_current_dir(&self.original_dir);
    }
}

/// Acquires the global lock that serializes tests mutating the process working
/// directory.
fn current_dir_lock() -> CurrentDirGuard {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    let lock = LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let original_dir = env::current_dir().expect("current working directory should be available");
    CurrentDirGuard {
        _lock: lock,
        original_dir,
    }
}

/// Where one template's generated projects put their build artifacts.
///
/// Under the workspace's target directory, following the convention
/// `cargo_projects_root` establishes in `tests/support`, so the artifacts
/// survive between runs -- building a generated project from cold takes
/// minutes, and throwing that away in a temporary directory each time is what
/// made these tests slow.
///
/// **One directory per template, not one shared by all of them.** Every test
/// generates a project called `template_test`, and `note` and `tx-script` both
/// generate a sibling called `add-contract` -- names fixed by the templates
/// themselves, which reference `../add-contract` by path. Under nextest each
/// test is its own process, so they run concurrently; pointing them at one
/// directory means identically named packages writing over each other's output,
/// and a test picking up another template's artifact. That is not theoretical:
/// it failed in CI with `account` building a package whose namespace was
/// `auth-component`.
///
/// It must not simply inherit the ambient `CARGO_TARGET_DIR` either, since
/// `cargo make` points that at the workspace's own target directory.
struct TargetDirGuard(Option<String>);

impl TargetDirGuard {
    fn for_template(template: &str) -> Self {
        let previous = env::var("CARGO_TARGET_DIR").ok();
        let dir = workspace_root()
            .join("target/miden_test_template_projects")
            .join(template.replace('-', "_"));
        fs::create_dir_all(&dir).expect("create the template build directory");
        // Safety: `set_var` is unsafe because of threads elsewhere in the
        // process; under nextest each test is its own process, and this is set
        // once before any subprocess is spawned.
        unsafe { env::set_var("CARGO_TARGET_DIR", &dir) };
        Self(previous)
    }
}

impl Drop for TargetDirGuard {
    fn drop(&mut self) {
        match self.0.take() {
            Some(previous) => unsafe { env::set_var("CARGO_TARGET_DIR", previous) },
            None => unsafe { env::remove_var("CARGO_TARGET_DIR") },
        }
    }
}

/// The repository root, two levels above `tests/templates`.
fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("this crate lives at tests/templates")
        .to_path_buf()
}

/// The directory holding the `rust/` templates this crate builds.
fn templates_root() -> PathBuf {
    workspace_root().join("extra/templates/rust")
}

/// Builds the `cargo miden new` argument vector for `template` into a project
/// named `name`, sourcing the template locally and the SDK from the compiler
/// branch.
fn new_project_args(name: &str, template: &str) -> Vec<String> {
    let template_path = templates_root().join(template);
    let compiler_path = workspace_root();
    vec![
        "cargo".into(),
        "miden".into(),
        "new".into(),
        name.into(),
        format!("--template-path={}", template_path.display()),
        format!("--compiler-path={}", compiler_path.display()),
    ]
}

/// Builds the `cargo miden build` argument vector, optionally for the `release`
/// profile.
fn build_args(release: bool) -> Vec<String> {
    let mut args = vec!["cargo".into(), "miden".into(), "build".into()];
    if release {
        args.push("--release".into());
    }
    args
}

/// Runs `cargo miden build` in the current directory and asserts that a single,
/// non-empty `.masp` package was emitted under the expected profile directory.
fn build_and_assert(release: bool) {
    let profile_dir = if release { "/release/" } else { "/dev/" };
    let output = run(build_args(release).into_iter())
        .unwrap_or_else(|e| {
            let profile = if release { " --release" } else { "" };
            panic!("`cargo miden build{profile}` failed: {e}")
        })
        .expect("`cargo miden build` should return a command output");

    let artifact = match output {
        CommandOutput::BuildCommandOutput { output } => match output.as_slice() {
            [artifact] => artifact.clone(),
            outputs => panic!("expected a single package artifact, got {outputs:#?}"),
        },
        other => panic!("expected a build output, got {other:?}"),
    };

    assert!(artifact.exists(), "package artifact does not exist: {}", artifact.display());
    assert_eq!(
        artifact.extension().and_then(|ext| ext.to_str()),
        Some("masp"),
        "unexpected artifact extension: {}",
        artifact.display()
    );
    assert!(
        artifact.to_string_lossy().contains(profile_dir),
        "expected `{profile_dir}` in artifact path: {}",
        artifact.display()
    );
    assert!(
        artifact.metadata().expect("artifact metadata should be readable").len() > 0,
        "package artifact is empty: {}",
        artifact.display()
    );
}

/// Scaffolds a new project from `template` and builds it for both profiles.
///
/// The `note` and `tx-script` templates import an account contract, so an
/// `add-contract` project is generated from the account template and built first
/// to emit the package and WIT interface they depend on.
pub fn build_new_project_from_template(template: &str) {
    let _cwd = current_dir_lock();

    let temp_dir = env::temp_dir().join(format!(
        "rust_templates_{}_{}",
        template.replace('-', "_"),
        SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()
    ));
    if temp_dir.exists() {
        fs::remove_dir_all(&temp_dir).unwrap();
    }
    fs::create_dir_all(&temp_dir).unwrap();
    env::set_current_dir(&temp_dir).unwrap();

    let _target_dir = TargetDirGuard::for_template(template);

    if matches!(template, "note" | "tx-script") {
        run(new_project_args("add-contract", "account").into_iter())
            .expect("failed to create the add-contract dependency project")
            .expect("`cargo miden new` should return a command output");
        env::set_current_dir(temp_dir.join("add-contract")).unwrap();
        // Build both profiles so the dependency package and its WIT exist for
        // whichever profile the dependent project is built with.
        build_and_assert(false);
        build_and_assert(true);
        env::set_current_dir(&temp_dir).unwrap();
    }

    let project_name = "template_test";
    run(new_project_args(project_name, template).into_iter())
        .unwrap_or_else(|e| panic!("failed to create project from `{template}` template: {e}"))
        .expect("`cargo miden new` should return a command output");
    let project_dir = temp_dir.join(project_name);
    assert!(project_dir.exists(), "generated project is missing: {}", project_dir.display());
    env::set_current_dir(&project_dir).unwrap();

    build_and_assert(false);
    build_and_assert(true);

    // Leave the temp dir (cwd is restored by the guard on drop).
    env::set_current_dir(workspace_root()).unwrap();
    let _ = fs::remove_dir_all(&temp_dir);
}
