//! Cargo project generation in-place for testing.
//! Based on cargo-test-support v0.2.0 crate code.

#![allow(dead_code)]

use std::{
    borrow::Cow,
    env,
    fmt::Write,
    fs, os,
    path::{Path, PathBuf},
    sync::OnceLock,
    time::{self, Duration},
};

use cargo_util::{ProcessBuilder, is_ci};

/// Panics with a formatted message if the expression does not return `Ok`.
#[macro_export]
macro_rules! t {
    ($e:expr) => {
        match $e {
            Ok(e) => e,
            Err(e) => {
                $crate::cargo_proj::panic_error(&format!("failed running {}", stringify!($e)), e)
            }
        }
    };
}

/// Panics with `err`, printing the error chain and cause.
#[track_caller]
#[inline(never)]
#[cold]
pub fn panic_error(what: &str, err: impl Into<anyhow::Error>) -> ! {
    let err = err.into();
    pe(what, err);
    #[track_caller]
    fn pe(what: &str, err: anyhow::Error) -> ! {
        let mut result = format!("{what}\nerror: {err}");
        for cause in err.chain().skip(1) {
            let _ = writeln!(result, "\nCaused by:");
            let _ = write!(result, "{cause}");
        }
        panic!("\n{result}");
    }
}

pub mod paths;
use self::paths::CargoPathExt;

/*
 *
 * ===== Builders =====
 *
 */

#[derive(PartialEq, Clone)]
struct FileBuilder {
    path: PathBuf,
    body: String,
    executable: bool,
}

impl FileBuilder {
    pub fn new(path: PathBuf, body: &str, executable: bool) -> FileBuilder {
        FileBuilder {
            path,
            body: body.to_string(),
            executable,
        }
    }

    /// Return the final path for this file, including any platform-specific executable suffix.
    fn output_path(&self) -> PathBuf {
        if !self.executable {
            return self.path.clone();
        }

        let mut path = self.path.clone().into_os_string();
        write!(path, "{}", env::consts::EXE_SUFFIX).unwrap();
        path.into()
    }

    /// Write the file to disk, but avoid touching the file when content is unchanged.
    ///
    /// Returns `(path, changed)`, where `changed` is `true` if a write occurred.
    fn mk(&self) -> (PathBuf, bool) {
        let path = self.output_path();

        path.parent().unwrap().mkdir_p();

        if fs::read(&path).ok().as_deref() == Some(self.body.as_bytes()) {
            #[cfg(unix)]
            if self.executable {
                use std::os::unix::fs::PermissionsExt;
                let mut perms = fs::metadata(&path).unwrap().permissions();
                let mode = perms.mode();
                perms.set_mode(mode | 0o111);
                fs::set_permissions(&path, perms).unwrap();
            }

            return (path, false);
        }

        fs::write(&path, &self.body)
            .unwrap_or_else(|e| panic!("could not create file {}: {}", path.display(), e));

        #[cfg(unix)]
        if self.executable {
            use std::os::unix::fs::PermissionsExt;

            let mut perms = fs::metadata(&path).unwrap().permissions();
            let mode = perms.mode();
            perms.set_mode(mode | 0o111);
            fs::set_permissions(&path, perms).unwrap();
        }

        (path, true)
    }
}

#[derive(PartialEq, Clone)]
struct SymlinkBuilder {
    dst: PathBuf,
    src: PathBuf,
    src_is_dir: bool,
}

impl SymlinkBuilder {
    pub fn new(dst: PathBuf, src: PathBuf) -> SymlinkBuilder {
        SymlinkBuilder {
            dst,
            src,
            src_is_dir: false,
        }
    }

    pub fn new_dir(dst: PathBuf, src: PathBuf) -> SymlinkBuilder {
        SymlinkBuilder {
            dst,
            src,
            src_is_dir: true,
        }
    }

    fn mk(&self) {
        self.dirname().mkdir_p();
        t!(os::unix::fs::symlink(&self.dst, &self.src));
    }

    fn dirname(&self) -> &Path {
        self.src.parent().unwrap()
    }
}

/// A cargo project to run tests against.
///
/// See [`ProjectBuilder`] to get started.
pub struct Project {
    root: PathBuf,
}

/// Create a project to run tests against
#[must_use]
pub struct ProjectBuilder {
    root: Project,
    files: Vec<FileBuilder>,
    symlinks: Vec<SymlinkBuilder>,
    no_manifest: bool,
}

impl ProjectBuilder {
    /// Root of the project, ex: `/path/to/cargo/target/cit/t0/foo`
    pub fn root(&self) -> &Path {
        self.root.root()
    }

    /// Project's debug dir, ex: `/path/to/cargo/target/cit/t0/foo/target/debug`
    pub fn target_debug_dir(&self) -> PathBuf {
        self.root.target_debug_dir()
    }

    /// Project's debug dir, ex: `/path/to/cargo/target/cit/t0/foo/target/release`
    pub fn target_release_dir(&self) -> PathBuf {
        self.root.target_release_dir()
    }

    /// Creates a new [`ProjectBuilder`] rooted at `root`.
    pub fn new(root: PathBuf) -> ProjectBuilder {
        ProjectBuilder {
            root: Project { root },
            files: vec![],
            symlinks: vec![],
            no_manifest: false,
        }
    }

    /// Adds a file to the project.
    pub fn file<B: AsRef<Path>>(mut self, path: B, body: &str) -> Self {
        self._file(path.as_ref(), body, false);
        self
    }

    /// Adds an executable file to the project.
    pub fn executable<B: AsRef<Path>>(mut self, path: B, body: &str) -> Self {
        self._file(path.as_ref(), body, true);
        self
    }

    fn _file(&mut self, path: &Path, body: &str, executable: bool) {
        let body = if path == Path::new("Cargo.toml") {
            ensure_workspace_root_manifest(body)
        } else {
            Cow::Borrowed(body)
        };
        self.files
            .push(FileBuilder::new(self.root.root().join(path), body.as_ref(), executable));
    }

    /// Adds a symlink to a file to the project.
    pub fn symlink<T: AsRef<Path>>(mut self, dst: T, src: T) -> Self {
        self.symlinks
            .push(SymlinkBuilder::new(self.root.root().join(dst), self.root.root().join(src)));
        self
    }

    /// Create a symlink to a directory
    pub fn symlink_dir<T: AsRef<Path>>(mut self, dst: T, src: T) -> Self {
        self.symlinks
            .push(SymlinkBuilder::new_dir(self.root.root().join(dst), self.root.root().join(src)));
        self
    }

    /// Disables automatic generation of a `Cargo.toml` when building the project.
    pub fn no_manifest(mut self) -> Self {
        self.no_manifest = true;
        self
    }

    /// Creates the project on disk.
    ///
    /// This is intentionally incremental: it prunes stale files from previous runs and rewrites
    /// only changed files, while preserving Cargo artifacts like `target/` and `Cargo.lock` to
    /// maximize caching across test runs.
    pub fn build(mut self) -> Project {
        let last_path_component =
            self.root.root().file_name().unwrap().to_string_lossy().to_string();

        if self.skip_rust_compilation(&last_path_component) {
            // Return the root directory without re-creating any files
            return self.root;
        }

        // Create the directory if missing
        self.root.root().mkdir_p();
        let manifest_path = self.root.root().join("Cargo.toml");
        if !self.no_manifest && self.files.iter().all(|fb| fb.path != manifest_path) {
            self._file(Path::new("Cargo.toml"), &basic_manifest("foo", "0.0.1"), false)
        }

        // Prune stale files from previous runs (but keep Cargo build artifacts).
        self.prune_root();

        let past = time::SystemTime::now() - Duration::new(1, 0);
        let ftime = filetime::FileTime::from_system_time(past);

        for file in self.files.iter() {
            let (path, changed) = file.mk();
            if changed && is_coarse_mtime() {
                // Place updated files 1 second in the past to avoid coarse mtime
                // collisions with build outputs (e.g. HFS on CI).
                filetime::set_file_times(&path, ftime, ftime).unwrap();
            }
        }

        for symlink in self.symlinks.iter_mut() {
            symlink.mk();
        }

        self.copy_workspace_lockfile();

        let ProjectBuilder { root, .. } = self;
        root
    }

    /// Seed generated standalone workspaces with the repository lockfile.
    ///
    /// Test projects use path dependencies into this repository, whose own dependencies are
    /// resolved in the workspace `Cargo.lock`. Since generated projects are intentionally made
    /// standalone workspaces, Cargo would otherwise try to resolve inherited registry
    /// dependencies online.
    fn copy_workspace_lockfile(&self) {
        let source = workspace_lockfile_path();
        let destination = self.root.root().join("Cargo.lock");

        let Ok(contents) = fs::read(&source) else {
            return;
        };

        if fs::read(&destination).ok().as_deref() == Some(contents.as_slice()) {
            return;
        }

        fs::write(&destination, contents).unwrap_or_else(|e| {
            panic!("could not copy {} to {}: {}", source.display(), destination.display(), e)
        });
    }

    fn rm_root(&self) {
        self.root.root().rm_rf()
    }

    /// Remove stale, non-generated files from prior runs while preserving Cargo artifacts.
    fn prune_root(&self) {
        use std::collections::HashSet;

        let root = self.root.root();
        let expected_files: HashSet<PathBuf> =
            self.files.iter().map(|file| file.output_path()).collect();
        let expected_symlinks: HashSet<PathBuf> =
            self.symlinks.iter().map(|link| link.src.clone()).collect();

        fn prune_dir(
            dir: &Path,
            expected_files: &HashSet<PathBuf>,
            expected_symlinks: &HashSet<PathBuf>,
        ) {
            let entries = match fs::read_dir(dir) {
                Ok(entries) => entries,
                Err(_) => return,
            };

            for entry in entries.flatten() {
                let path = entry.path();

                // Preserve Cargo build artifacts and lockfile to maximize caching.
                if path.file_name() == Some(std::ffi::OsStr::new("target"))
                    || path.file_name() == Some(std::ffi::OsStr::new("Cargo.lock"))
                {
                    continue;
                }

                let ft = match entry.file_type() {
                    Ok(ft) => ft,
                    Err(_) => continue,
                };

                if ft.is_dir() {
                    prune_dir(&path, expected_files, expected_symlinks);
                    // Best-effort cleanup of now-empty directories
                    let _ = fs::remove_dir(&path);
                    continue;
                }

                let is_expected =
                    expected_files.contains(&path) || expected_symlinks.contains(&path);
                if !is_expected {
                    let _ = fs::remove_file(&path);
                }
            }
        }

        prune_dir(root, &expected_files, &expected_symlinks);
    }

    fn skip_rust_compilation(&self, artifact_name: &str) -> bool {
        let computed_artifact_path_wuu = self
            .root()
            .join("target")
            .join("wasm32-unknown-unknown")
            .join("release")
            .join(artifact_name)
            .with_extension("wasm");
        let computed_artifact_path_ww = self
            .root()
            .join("target")
            .join("wasm32-wasip2")
            .join("release")
            .join(artifact_name)
            .with_extension("wasm");
        if std::env::var("SKIP_RUST").is_ok()
            && (computed_artifact_path_wuu.exists() || computed_artifact_path_ww.exists())
        {
            eprintln!("Skipping Rust compilation");
            true
        } else {
            false
        }
    }
}

/// Ensure a generated `Cargo.toml` is treated as its own workspace root.
///
/// When we place generated projects under this repository's `target/` directory, Cargo will
/// otherwise discover the repo's root workspace and reject the generated package as an unlisted
/// member. Adding an (empty) `[workspace]` table makes the generated package its own workspace.
fn ensure_workspace_root_manifest(manifest: &str) -> Cow<'_, str> {
    let has_workspace_table = manifest.lines().any(|line| line.trim() == "[workspace]");
    if has_workspace_table {
        return Cow::Borrowed(manifest);
    }

    let mut s = manifest.to_string();
    if !s.ends_with('\n') {
        s.push('\n');
    }
    s.push('\n');
    s.push_str("[workspace]\n");
    Cow::Owned(s)
}

fn workspace_lockfile_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("tests/support should live under the repository root")
        .join("Cargo.lock")
}

impl Project {
    /// Root of the project, ex: `/path/to/cargo/target/cit/t0/foo`
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Project's target dir, ex: `/path/to/cargo/target/cit/t0/foo/target`
    pub fn build_dir(&self) -> PathBuf {
        self.root().join("target")
    }

    /// Project's debug dir, ex: `/path/to/cargo/target/cit/t0/foo/target/debug`
    pub fn target_debug_dir(&self) -> PathBuf {
        self.build_dir().join("debug")
    }

    /// Project's release dir, ex: `/path/to/cargo/target/cit/t0/foo/target/release`
    pub fn target_release_dir(&self) -> PathBuf {
        self.build_dir().join("release")
    }

    /// Path to an example built as a library.
    /// `kind` should be one of: "lib", "rlib", "staticlib", "dylib", "proc-macro"
    /// ex: `/path/to/cargo/target/cit/t0/foo/target/debug/examples/libex.rlib`
    pub fn example_lib(&self, name: &str, kind: &str) -> PathBuf {
        self.target_debug_dir()
            .join("examples")
            .join(paths::get_lib_filename(name, kind))
    }

    /// Path to a debug binary.
    /// ex: `/path/to/cargo/target/cit/t0/foo/target/debug/foo`
    pub fn bin(&self, b: &str) -> PathBuf {
        self.build_dir().join("debug").join(format!("{}{}", b, env::consts::EXE_SUFFIX))
    }

    /// Path to a release binary.
    /// ex: `/path/to/cargo/target/cit/t0/foo/target/release/foo`
    pub fn release_bin(&self, b: &str) -> PathBuf {
        self.target_release_dir().join(format!("{}{}", b, env::consts::EXE_SUFFIX))
    }

    /// Path to a debug binary for a specific target triple.
    /// ex: `/path/to/cargo/target/cit/t0/foo/target/i686-apple-darwin/debug/foo`
    pub fn target_bin(&self, target: &str, b: &str) -> PathBuf {
        self.build_dir().join(target).join("debug").join(format!(
            "{}{}",
            b,
            env::consts::EXE_SUFFIX
        ))
    }

    /// Returns an iterator of paths matching the glob pattern, which is
    /// relative to the project root.
    pub fn glob<P: AsRef<Path>>(&self, pattern: P) -> glob::Paths {
        let pattern = self.root().join(pattern);
        glob::glob(pattern.to_str().expect("failed to convert pattern to str"))
            .expect("failed to glob")
    }

    /// Changes the contents of an existing file.
    pub fn change_file(&self, path: &str, body: &str) {
        let _ = FileBuilder::new(self.root().join(path), body, false).mk();
    }

    /// Returns the contents of `Cargo.lock`.
    pub fn read_lockfile(&self) -> String {
        self.read_file("Cargo.lock")
    }

    /// Returns the contents of a path in the project root
    pub fn read_file(&self, path: &str) -> String {
        let full = self.root().join(path);
        fs::read_to_string(&full)
            .unwrap_or_else(|e| panic!("could not read file {}: {}", full.display(), e))
    }

    /// Modifies `Cargo.toml` to remove all commented lines.
    pub fn uncomment_root_manifest(&self) {
        let contents = self.read_file("Cargo.toml").replace('#', "");
        fs::write(self.root().join("Cargo.toml"), contents).unwrap();
    }

    /// Creates a symlink within the project directory.
    pub fn symlink(&self, src: impl AsRef<Path>, dst: impl AsRef<Path>) {
        let src = self.root().join(src.as_ref());
        let dst = self.root().join(dst.as_ref());
        {
            if let Err(e) = os::unix::fs::symlink(&src, &dst) {
                panic!("failed to symlink {src:?} to {dst:?}: {e:?}");
            }
        }
    }
}

/// The Cargo target directory this test process builds under.
///
/// Derived from an absolute `CARGO_TARGET_DIR` when the process was started with one, otherwise
/// inferred from the test executable's location. Read **once**, before [`use_shared_build_dir`]
/// overwrites the variable, so both directories below are anchored to the same place whether the
/// suite is invoked as `cargo test` (no ambient variable) or through `cargo make` (which sets one,
/// `Makefile.toml`).
fn test_target_dir() -> PathBuf {
    static TARGET_DIR: OnceLock<PathBuf> = OnceLock::new();
    TARGET_DIR
        .get_or_init(|| {
            // Prefer an explicit override when provided (useful in CI).
            if let Ok(dir) = std::env::var("CARGO_TARGET_DIR") {
                let dir = PathBuf::from(dir);
                if dir.is_absolute() {
                    return dir;
                }
            }

            let exe = std::env::current_exe()
                .unwrap_or_else(|e| panic!("failed to determine test target directory: {e}"));

            // `cargo test` places the test binary at `<target_dir>/<profile>/deps/<bin>`.
            for ancestor in exe.ancestors() {
                if ancestor
                    .file_name()
                    .is_some_and(|file_name| file_name.eq_ignore_ascii_case("target"))
                    && ancestor.is_dir()
                {
                    return ancestor.to_path_buf();
                }
            }

            panic!("failed to determine test target directory from current_exe: {}", exe.display());
        })
        .clone()
}

/// The one build directory every nested `cargo` invocation in this suite shares.
///
/// # Why one directory
///
/// Each generated project is its own workspace, so with no `CARGO_TARGET_DIR` Cargo gives each one
/// a private `target/` and shares nothing. Every one of them builds `core`, `alloc`,
/// `compiler_builtins` and `panic_abort` from source (`-Z build-std`) plus the whole native Miden
/// stack as build dependencies, at `lto = true, codegen-units = 1` — around 900 MB and half a
/// minute of compilation to produce a few hundred bytes of Wasm. Across the suite that is the same
/// work tens of times over: at the time this was written, this was measured at 48,131 crate units
/// of which 1,627 are distinct.
///
/// Cargo already keys artifacts by a metadata hash that separates flag variants, so one directory
/// holds all of them side by side. It takes an exclusive lock on the build directory, so concurrent
/// nested builds serialize; that cost was measured before this was adopted rather than assumed. On
/// a 19-test slice driving 57 nested builds on ten cores: **1034 s and 33.6 GB unshared, versus
/// 66.9 s and 1.05 GB shared** — 15.5x on time and 31.8x on disk — with 56 build-directory lock
/// waits which, in total, cannot have exceeded the 67 s the whole run took.
///
/// The directory is a sibling of the generated projects rather than the workspace target directory
/// itself, so that nested-build output stays separable from the workspace's own and can be measured
/// (and deleted) on its own.
///
/// # Why it is worth keeping
///
/// At a few GB the cache survives between runs, without this, the total size of a warm cache was
/// exceeding 100GB, which made it necessary to frequently delete it to avoid consuming a lot of
/// disk space unnecessarily - this made test runs cold, exacerbating test run times due to the
/// need to rebuild the cache frequently.
pub fn shared_build_dir() -> PathBuf {
    test_target_dir().join("miden_test_shared")
}

/// Point this process's nested `cargo` invocations at [`shared_build_dir`].
///
/// The environment is the only means we have to reache them: the nested `cargo` is spawned by the
/// compiler itself (`midenc-compile`'s `run_cargo`), which inherits this process's environment,
/// and it selects the project with `--manifest-path` rather than by working directory — so a
/// `.cargo/config.toml` written beside the project would never be read.
///
/// Idempotent, and safe to call from any entry point that is about to cause a nested build. It must
/// be called by **every** such entry point, not just [`project`]: tests that compile a checked-in
/// fixture project never generate one, and before this they wrote a private `target/` into the
/// source tree (`tests/fixtures/components/*/target`, measured at ~1.7 GB apiece).
///
/// # Safety
///
/// `set_var` is unsafe because a concurrent `getenv` on another thread is a data race. This runs at
/// most once per process, from the first entry point reached, and before that entry point spawns
/// anything. The same reasoning is what permits the equivalent call in `tests/templates`, and the
/// suite's own runner (`cargo make test` → `nextest`) gives each test its own process.
pub fn use_shared_build_dir() {
    static INIT: OnceLock<()> = OnceLock::new();
    INIT.get_or_init(|| {
        let dir = shared_build_dir();
        std::fs::create_dir_all(&dir).unwrap_or_else(|e| {
            panic!("failed to create the shared build directory {}: {e}", dir.display())
        });
        unsafe { std::env::set_var("CARGO_TARGET_DIR", &dir) };
    });
}

/// Creates a [`ProjectBuilder`] for a generated Cargo project.
///
/// The project is located under the Cargo target directory to maximize reuse of build artifacts
/// across test runs.
///
/// The directory is derived from an absolute `CARGO_TARGET_DIR` when set, or inferred from the test
/// executable location.
#[track_caller]
pub fn project(proj_folder_name: &str) -> ProjectBuilder {
    /// Compute the directory under which generated Cargo projects should live.
    ///
    /// We keep these projects in the Cargo target directory so that Cargo build artifacts
    /// (which now live in [`shared_build_dir`]) can be reused across test runs.
    fn cargo_projects_root() -> PathBuf {
        let root = test_target_dir().join("miden_test_cargo_projects");
        // Anything built out of a generated project is a nested build, so redirect them before the
        // caller gets a chance to start one.
        use_shared_build_dir();
        root
    }

    /// Convert a call site into a stable directory name component.
    fn callsite_dir(file: &str, line: u32, column: u32) -> String {
        let mut s = String::new();
        for ch in file.chars() {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                s.push(ch);
            } else {
                s.push('_');
            }
        }
        write!(&mut s, "_{}_{}", line, column).unwrap();
        s
    }

    let loc = std::panic::Location::caller();
    let cargo_projects_root =
        cargo_projects_root().join(callsite_dir(loc.file(), loc.line(), loc.column()));
    let cargo_proj_path = cargo_projects_root.join(proj_folder_name);

    ProjectBuilder::new(cargo_proj_path)
}

/// This is the raw output from the process.
///
/// This is similar to `std::process::Output`, however the `status` is translated to the raw `code`.
/// This is necessary because `ProcessError` does not have access to the raw `ExitStatus` because
/// `ProcessError` needs to be serializable (for the Rustc cache), and `ExitStatus` does not provide
/// a constructor.
pub struct RawOutput {
    /// The exit code, if known
    pub code: Option<i32>,
    /// The contents of what was printed to stdout
    pub stdout: Vec<u8>,
    /// The contents of what was printed to stderr
    pub stderr: Vec<u8>,
}

/// Creates a basic package manifest for a crate named `name` of version `version`
pub fn basic_manifest(name: &str, version: &str) -> String {
    format!(
        r#"
        [package]
        name = "{name}"
        version = "{version}"
        authors = []
        edition = "2024"
    "#
    )
}

/// Creates a basic package manifest for a binary crate named `name`
pub fn basic_bin_manifest(name: &str) -> String {
    format!(
        r#"
        [package]
        name = "{name}"
        version = "0.5.0"
        authors = ["wycats@example.com"]
        edition = "2024"

        [[bin]]
        name = "{name}"
    "#
    )
}

/// Creates a basic package manifest for a library crate named `name`
pub fn basic_lib_manifest(name: &str) -> String {
    format!(
        r#"
        [package]
        name = "{name}"
        version = "0.5.0"
        authors = ["wycats@example.com"]
        edition = "2024"

        [lib]
        name = "{name}"
    "#
    )
}

struct RustcInfo {
    verbose_version: String,
    host: String,
}

impl RustcInfo {
    fn new() -> RustcInfo {
        let output = ProcessBuilder::new("rustc")
            .arg("-vV")
            .exec_with_output()
            .expect("rustc should exec");
        let verbose_version = String::from_utf8(output.stdout).expect("utf8 output");
        let host = verbose_version
            .lines()
            .filter_map(|line| line.strip_prefix("host: "))
            .next()
            .expect("verbose version has host: field")
            .to_string();
        RustcInfo {
            verbose_version,
            host,
        }
    }
}

fn rustc_info() -> &'static RustcInfo {
    static RUSTC_INFO: OnceLock<RustcInfo> = OnceLock::new();
    RUSTC_INFO.get_or_init(RustcInfo::new)
}

/// The rustc host such as `x86_64-unknown-linux-gnu`.
pub fn rustc_host() -> &'static str {
    &rustc_info().host
}

/// The host triple suitable for use in a cargo environment variable (uppercased).
pub fn rustc_host_env() -> String {
    rustc_host().to_uppercase().replace('-', "_")
}

/// Returns true if the current `rustc` is a nightly or not.
///
/// If `CARGO_TEST_DISABLE_NIGHTLY` is set, this returns false
pub fn is_nightly() -> bool {
    let vv = &rustc_info().verbose_version;
    // CARGO_TEST_DISABLE_NIGHTLY is set in rust-lang/rust's CI so that all
    // nightly-only tests are disabled there. Otherwise, it could make it
    // difficult to land changes which would need to be made simultaneously in
    // rust-lang/cargo and rust-lan/rust, which isn't possible.
    env::var("CARGO_TEST_DISABLE_NIGHTLY").is_err()
        && (vv.contains("-nightly") || vv.contains("-dev"))
}

/// Returns `true` if the local filesystem has low-resolution mtimes.
pub fn is_coarse_mtime() -> bool {
    // This should actually be a test that `$CARGO_TARGET_DIR` is on an HFS
    // filesystem, (or any filesystem with low-resolution mtimes). However,
    // that's tricky to detect, so for now just deal with CI.
    cfg!(target_os = "macos") && is_ci()
}
