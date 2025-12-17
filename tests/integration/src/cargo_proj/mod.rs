//! Cargo project generation in-place for testing.
//! Based on cargo-test-support v0.2.0 crate code.

#![allow(dead_code)]

use std::{
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

#[track_caller]
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

    fn mk(&mut self) {
        if self.executable {
            let mut path = self.path.clone().into_os_string();
            write!(path, "{}", env::consts::EXE_SUFFIX).unwrap();
            self.path = path.into();
        }

        self.dirname().mkdir_p();
        fs::write(&self.path, &self.body)
            .unwrap_or_else(|e| panic!("could not create file {}: {}", self.path.display(), e));

        #[cfg(unix)]
        if self.executable {
            use std::os::unix::fs::PermissionsExt;

            let mut perms = fs::metadata(&self.path).unwrap().permissions();
            let mode = perms.mode();
            perms.set_mode(mode | 0o111);
            fs::set_permissions(&self.path, perms).unwrap();
        }
    }

    fn dirname(&self) -> &Path {
        self.path.parent().unwrap()
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
/// See [`ProjectBuilder`] or [`Project::from_template`] to get started.
pub struct Project {
    root: PathBuf,
}

/// Create a project to run tests against
///
/// The project can be constructed programmatically or from the filesystem with
/// [`Project::from_template`]
#[must_use]
pub struct ProjectBuilder {
    root: Project,
    files: Vec<FileBuilder>,
    symlinks: Vec<SymlinkBuilder>,
    no_manifest: bool,
}

impl ProjectBuilder {
    /// Root of the project, ex: `/path/to/cargo/target/cit/t0/foo`
    pub fn root(&self) -> PathBuf {
        self.root.root()
    }

    /// Project's debug dir, ex: `/path/to/cargo/target/cit/t0/foo/target/debug`
    pub fn target_debug_dir(&self) -> PathBuf {
        self.root.target_debug_dir()
    }

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
        self.files.push(FileBuilder::new(self.root.root().join(path), body, executable));
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

    pub fn no_manifest(mut self) -> Self {
        self.no_manifest = true;
        self
    }

    /// Creates the project.
    pub fn build(mut self) -> Project {
        let last_path_component =
            self.root.root().file_name().unwrap().to_string_lossy().to_string();

        if self.skip_rust_compilation(&last_path_component) {
            // Return the root directory without re-creating any files
            return self.root;
        }

        // First, clean the directory if it already exists
        self.rm_root();

        // Create the empty directory
        self.root.root().mkdir_p();

        let manifest_path = self.root.root().join("Cargo.toml");
        if !self.no_manifest && self.files.iter().all(|fb| fb.path != manifest_path) {
            self._file(Path::new("Cargo.toml"), &basic_manifest("foo", "0.0.1"), false)
        }

        let past = time::SystemTime::now() - Duration::new(1, 0);
        let ftime = filetime::FileTime::from_system_time(past);

        for file in self.files.iter_mut() {
            file.mk();
            if is_coarse_mtime() {
                // Place the entire project 1 second in the past to ensure
                // that if cargo is called multiple times, the 2nd call will
                // see targets as "fresh". Without this, if cargo finishes in
                // under 1 second, the second call will see the mtime of
                // source == mtime of output and consider it dirty.
                filetime::set_file_times(&file.path, ftime, ftime).unwrap();
            }
        }

        for symlink in self.symlinks.iter_mut() {
            symlink.mk();
        }

        let ProjectBuilder { root, .. } = self;
        root
    }

    fn rm_root(&self) {
        self.root.root().rm_rf()
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

impl Project {
    /// Root of the project, ex: `/path/to/cargo/target/cit/t0/foo`
    pub fn root(&self) -> PathBuf {
        self.root.clone()
    }

    /// Project's target dir, ex: `/path/to/cargo/target/cit/t0/foo/target`
    pub fn build_dir(&self) -> PathBuf {
        self.root().join("target")
    }

    /// Project's debug dir, ex: `/path/to/cargo/target/cit/t0/foo/target/debug`
    pub fn target_debug_dir(&self) -> PathBuf {
        self.build_dir().join("debug")
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
        self.build_dir()
            .join("release")
            .join(format!("{}{}", b, env::consts::EXE_SUFFIX))
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
        FileBuilder::new(self.root().join(path), body, false).mk()
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

// Generates a project layout
pub fn project(proj_folder_name: &str) -> ProjectBuilder {
    let temp_dir = &std::env::temp_dir();
    let cargo_projects_root = temp_dir.join("miden_test_cargo_projects");
    let cargo_proj_path = cargo_projects_root.join(proj_folder_name);
    // dbg!(&cargo_proj_path);
    ProjectBuilder::new(cargo_proj_path)
}

/// This is the raw output from the process.
///
/// This is similar to `std::process::Output`, however the `status` is
/// translated to the raw `code`. This is necessary because `ProcessError`
/// does not have access to the raw `ExitStatus` because `ProcessError` needs
/// to be serializable (for the Rustc cache), and `ExitStatus` does not
/// provide a constructor.
pub struct RawOutput {
    pub code: Option<i32>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

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
