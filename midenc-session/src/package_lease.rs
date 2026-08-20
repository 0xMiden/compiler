//! The per-build package-cache directory: a unique lease, or an adopted caller directory.
//!
//! The compiler and the nested builds it spawns exchange compiled dependency packages
//! through the filesystem: proc macros run inside `rustc` processes and read the `.masp`
//! bytes of the packages the assembler has already published. The directory that carries
//! this exchange is derived once for the root compilation and shared — by `Arc` — across
//! the root [`Session`] and all of its clones. It has two modes:
//!
//! - **Leased** (the default): the compiler creates a directory with a globally unique
//!   name atomically when the root compilation starts, and deletes it when the last owner
//!   drops. Uniqueness per build is the whole correctness story: no two builds ever
//!   observe the same directory, so stale packages cannot leak between builds, and no
//!   fingerprinting, locking, or pruning of a shared namespace is needed.
//! - **Adopted**: when the calling process already exported `MIDENC_PACKAGE_CACHE`, the
//!   compiler uses that directory as the package cache and leaves it in place. The caller
//!   owns the directory's location and lifetime. This is how packages outlive the
//!   compiler process: a contract `build.rs` stages the cache in its own `OUT_DIR`, runs
//!   the nested `cargo miden build` with the variable set, and re-exports the same value
//!   to the outer `rustc` whose macro expansions read the packages after the compiler
//!   exited.
//!
//! Consumers track the path itself (`option_env!("MIDENC_PACKAGE_CACHE")`) and the
//! package bytes (`include_bytes!`), so a fresh lease re-expands every consumer, and an
//! adopted stable directory re-expands them only when package contents change.
//!
//! Nested dependency builds receive the root's directory through their build environment
//! (`MIDENC_PACKAGE_CACHE`) or as an explicit parameter. They must never create a
//! directory of their own: a dependency with a private directory could not see its
//! already-assembled transitive dependencies. A nested compiler invocation that sees the
//! variable adopts the outer build's directory through the same detection, which is
//! exactly the sharing the parameter threading provides in-process.
//!
//! Lease deletion runs after the pipeline and all of its cargo/rustc children have
//! exited. A killed process leaves the directory behind; remnants under `target/` are
//! harmless — nothing ever reads a foreign lease — and `cargo clean` removes them. Do not
//! add a sweeper for them: a sweeper needs ownership heuristics and liveness protocols,
//! which is the complexity the lease design exists to avoid.
//!
//! [`Session`]: crate::Session

use alloc::string::String;
use std::path::{Path, PathBuf};

/// The shared cell owning a session family's package-cache lease.
///
/// Cloned into every consumer whose lifetime must keep the leased directory alive — the
/// session's clones, and the package registry that publishes into the directory.
pub(crate) type SharedPackageCacheLease =
    alloc::sync::Arc<std::sync::OnceLock<Result<PackageCacheLease, String>>>;

/// The environment variable that names the package cache.
///
/// The compiler reads it to adopt a caller-provided directory, and sets it for the
/// nested cargo builds it spawns. The spelling is shared with the SDK macros through the
/// contract crate.
pub(crate) use midenc_frontend_wasm_metadata::package_cache::PACKAGE_CACHE_ENV;

/// The per-build package-cache directory.
///
/// Created by the root compilation session; see the module documentation for the two
/// modes and their lifecycles.
#[derive(Debug)]
pub(crate) enum PackageCacheLease {
    /// A directory this build created; dropping it deletes the directory recursively,
    /// best-effort.
    Leased(tempfile::TempDir),
    /// A directory the calling process provided through `MIDENC_PACKAGE_CACHE`; never
    /// deleted here.
    Adopted(PathBuf),
}

impl PackageCacheLease {
    /// Derives the package-cache directory for a root build using `target_dir`.
    ///
    /// Adopts the directory named by `MIDENC_PACKAGE_CACHE` when the variable is set
    /// (an empty value counts as unset), and mints a unique lease under
    /// [`package_cache_parent`] otherwise.
    ///
    /// Fails closed: when the directory cannot be created, the error is reported and the
    /// build must stop, rather than continue with no package exchange and fail later
    /// inside a macro expansion with a confusing missing-package diagnostic.
    ///
    /// The error is a `String` so the caller can memoize it in a shared cell and report
    /// it identically on every access.
    pub(crate) fn create(target_dir: &Path) -> Result<Self, String> {
        Self::from_env_value(std::env::var_os(PACKAGE_CACHE_ENV), target_dir)
    }

    /// [`Self::create`] with the environment read separated out, for tests.
    fn from_env_value(
        env_value: Option<std::ffi::OsString>,
        target_dir: &Path,
    ) -> Result<Self, String> {
        match env_value {
            Some(value) if !value.is_empty() => Self::adopt(PathBuf::from(value)),
            _ => Self::lease(target_dir),
        }
    }

    /// Adopts a caller-provided directory: created if needed, never deleted here.
    fn adopt(dir: PathBuf) -> Result<Self, String> {
        // A relative caller value is anchored to this process's working directory, so
        // the registry and the nested builds — which run in other directories — agree
        // on one location.
        let dir = std::path::absolute(&dir).unwrap_or(dir);
        std::fs::create_dir_all(&dir).map_err(|err| {
            format!(
                "cannot create the package cache '{}' named by {PACKAGE_CACHE_ENV}: {err}",
                dir.display()
            )
        })?;
        log::debug!(
            target: "package-cache",
            "adopted the caller-provided package cache '{}'",
            dir.display()
        );
        Ok(Self::Adopted(dir))
    }

    /// Mints a unique lease directory under [`package_cache_parent`] of `target_dir`.
    fn lease(target_dir: &Path) -> Result<Self, String> {
        let parent = package_cache_parent(target_dir);
        std::fs::create_dir_all(&parent).map_err(|err| {
            format!("cannot create the package cache parent '{}': {err}", parent.display())
        })?;
        let dir = tempfile::Builder::new().prefix("build-").tempdir_in(&parent).map_err(|err| {
            format!("cannot create a package cache under '{}': {err}", parent.display())
        })?;
        log::debug!(
            target: "package-cache",
            "created package cache '{}' for this build",
            dir.path().display()
        );
        Ok(Self::Leased(dir))
    }

    /// The package-cache directory.
    pub(crate) fn path(&self) -> &Path {
        match self {
            Self::Leased(dir) => dir.path(),
            Self::Adopted(dir) => dir.as_path(),
        }
    }
}

/// `<target-dir>/packages` — the parent directory of every minted lease.
///
/// Anchored at the session's configured target directory (`<cwd>/target/miden` by default),
/// so a caller-supplied `--target-dir` — a writable location for a read-only checkout, for
/// example — is honored. Only the root build derives this path; every nested participant
/// receives it through `MIDENC_PACKAGE_CACHE`, so agreement needs no fixed anchor. The
/// unique lease name below the parent does the isolation.
pub(crate) fn package_cache_parent(target_dir: &Path) -> PathBuf {
    target_dir.join("packages")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn leases_are_unique_and_deleted_on_drop() {
        let temp = tempfile::TempDir::new().unwrap();
        let first = PackageCacheLease::from_env_value(None, temp.path()).unwrap();
        let second = PackageCacheLease::from_env_value(None, temp.path()).unwrap();

        assert_ne!(first.path(), second.path(), "two leases must never share a directory");
        assert_eq!(first.path().parent().unwrap(), package_cache_parent(temp.path()));
        assert!(first.path().is_dir());
        assert!(second.path().is_dir());

        let first_path = first.path().to_path_buf();
        drop(first);
        assert!(!first_path.exists(), "dropping the lease must delete its directory");
        assert!(second.path().is_dir(), "an unrelated lease must survive");
    }

    #[test]
    fn an_env_named_directory_is_adopted_and_never_deleted() {
        let temp = tempfile::TempDir::new().unwrap();
        let caller_dir = temp.path().join("caller-cache");

        let adopted = PackageCacheLease::from_env_value(
            Some(caller_dir.clone().into_os_string()),
            temp.path(),
        )
        .unwrap();

        assert_eq!(adopted.path(), caller_dir.as_path(), "the caller names the directory");
        assert!(caller_dir.is_dir(), "adoption must create the directory");
        assert!(
            !package_cache_parent(temp.path()).exists(),
            "adoption must not mint a lease under the project"
        );

        drop(adopted);
        assert!(caller_dir.is_dir(), "an adopted directory is the caller's; never delete it");
    }

    #[test]
    fn an_empty_env_value_counts_as_unset() {
        let temp = tempfile::TempDir::new().unwrap();
        let lease = PackageCacheLease::from_env_value(Some(std::ffi::OsString::new()), temp.path())
            .unwrap();
        assert_eq!(lease.path().parent().unwrap(), package_cache_parent(temp.path()));
    }

    #[test]
    fn creation_fails_closed_on_an_unwritable_parent() {
        let temp = tempfile::TempDir::new().unwrap();
        // Occupy the parent path with a file, so the directory cannot be created.
        let target_dir = temp.path().join("target-dir");
        std::fs::create_dir_all(&target_dir).unwrap();
        std::fs::write(package_cache_parent(&target_dir), b"not a directory").unwrap();

        let error = PackageCacheLease::from_env_value(None, &target_dir).unwrap_err();
        assert!(error.contains("package cache"), "unexpected error text: {error}");
    }
}
