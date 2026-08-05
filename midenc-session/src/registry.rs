use alloc::{collections::BTreeMap, format, sync::Arc};

#[cfg(feature = "std")]
use miden_assembly_syntax::Report;
use miden_assembly_syntax::diagnostics::{Diagnostic, miette};
use miden_mast_package::Package;
use miden_package_registry::{
    PackageCache, PackageId, PackageIndex, PackageProvider, PackageRecord, PackageRegistry,
    PackageStore, PackageVersions,
};
use miden_project::VersionRequirement;

type FxHashMap<K, V> = hashbrown::HashMap<K, V, rustc_hash::FxBuildHasher>;

#[derive(Debug, thiserror::Error, Diagnostic)]
#[non_exhaustive]
enum InstallPackageError {
    #[error("package {package}@{version} is already registered under a different digest")]
    AlreadyInstalledWithDifferentDigest {
        package: PackageId,
        version: miden_project::Version,
    },
    #[cfg(any(test, feature = "std"))]
    #[error("failed to write {package} to filesystem cache: {err}")]
    FilesystemCacheInsertion {
        package: PackageId,
        err: std::io::Error,
    },
}

/// The in-memory package registry used by the compiler
///
/// This is initialized per-session, or on an as-needed basis.
///
/// It can be constructed in various ways, but the recommended way to use it is
/// [HybridPackageRegistry::new], which loads packages from the local filesystem registry (if
/// available), and adds in any libraries requested explicitly via `-l`.
pub struct HybridPackageRegistry {
    packages: FxHashMap<PackageId, PackageVersions>,
    artifacts: FxHashMap<PackageId, BTreeMap<miden_package_registry::Version, Arc<Package>>>,
    #[cfg(any(test, feature = "std"))]
    filesystem_cache: Option<std::path::PathBuf>,
    #[cfg(any(test, feature = "std"))]
    filesystem_cache_lock: Option<std::fs::File>,
}

impl HybridPackageRegistry {
    #[cfg(any(test, feature = "std"))]
    pub fn filesystem_cache_dir(&self) -> Option<&std::path::Path> {
        self.filesystem_cache.as_deref()
    }

    /// Get an empty, uninitialized registry
    pub fn empty() -> Self {
        Self {
            packages: Default::default(),
            artifacts: Default::default(),
            filesystem_cache: None,
            filesystem_cache_lock: None,
        }
    }

    /// Get a new instance of the registry, using the current compiler options
    #[cfg(any(test, feature = "std"))]
    pub fn new(options: &crate::Options) -> Result<Self, Report> {
        Self::new_with_filesystem_cache(options, None)
    }

    /// Get a new instance of the registry, using the current compiler options and an optional
    /// filesystem cache directory.
    ///
    /// A cache path in the owned `miden/packages/<fingerprint>` layout is created and locked for
    /// the registry's lifetime. During construction, dead sibling fingerprint directories and
    /// legacy flat `.masp` entries are pruned as defense in depth; FPI expansions track the cache
    /// path themselves for correctness. Any other path is created but deliberately neither locked
    /// nor used to sweep its parent.
    ///
    /// Cleanup is best-effort and its failures are reported only through the `package-registry`
    /// log target. Package insertion failures are still returned to the caller.
    #[cfg(any(test, feature = "std"))]
    pub fn new_with_filesystem_cache(
        options: &crate::Options,
        filesystem_cache: Option<std::path::PathBuf>,
    ) -> Result<Self, Report> {
        use alloc::string::ToString;

        let filesystem_cache_lock = filesystem_cache.as_deref().and_then(prepare_filesystem_cache);

        // Load system libraries
        let mut registry = if options.sysroot.is_some() {
            Self::from_local_registry(options)?
        } else {
            Self::empty()
        };
        registry.filesystem_cache = filesystem_cache;
        registry.filesystem_cache_lock = filesystem_cache_lock;

        // Load link libraries
        let core = crate::LinkLibrary::core();
        let tx_kernel = crate::LinkLibrary::tx_kernel();
        let protocol = crate::LinkLibrary::protocol();
        let implied_libraries = vec![&core, &tx_kernel, &protocol]
            .into_iter()
            .filter(|ll| !options.link_libraries.iter().any(|oll| oll.name == ll.name));
        let link_libraries = options.link_libraries.iter().chain(implied_libraries);
        for lib in link_libraries {
            let package = lib.load(options)?;
            match registry.install_if_missing(package) {
                Ok(_) => (),
                // Ignore duplicates when initializing the registry
                Err(InstallPackageError::AlreadyInstalledWithDifferentDigest { .. }) => (),
                Err(err) => return Err(Report::msg(err.to_string())),
            }
        }

        Ok(registry)
    }

    /// Get a new instance of the registry, using the current compiler options
    #[cfg(not(any(test, feature = "std")))]
    pub fn new(options: &crate::Options) -> Result<Self, Report> {
        Ok(Self::empty())
    }

    /// Get a new instance of the registry seeded with packages available in the local filesystem-
    /// based package store.
    ///
    /// This returns an error if `--sysroot` was not provided/set.
    #[cfg(any(test, feature = "std"))]
    pub fn from_local_registry(options: &crate::Options) -> Result<Self, Report> {
        use alloc::string::ToString;

        let Some(sysroot) = options.sysroot.as_deref() else {
            return Err(Report::msg(
                "unable to load packages from local registry: --sysroot was not provided",
            ));
        };

        let lib_dir = sysroot.join("lib");
        let entries = lib_dir.read_dir().map_err(|err| {
            Report::msg(format!("cannot read from sysroot ({}): {err}", lib_dir.display()))
        })?;

        let mut registry = Self::empty();
        for entry in entries {
            let Ok(entry) = entry else {
                continue;
            };
            let path = entry.path();
            if path.extension().is_none_or(|ext| !ext.eq_ignore_ascii_case(Package::EXTENSION)) {
                continue;
            }

            let package = crate::libs::load_package_from_path(&path)?;
            match registry.install_if_missing(package) {
                Ok(_) => (),
                // Ignore duplicates when initializing the registry
                Err(InstallPackageError::AlreadyInstalledWithDifferentDigest { .. }) => (),
                Err(err) => return Err(Report::msg(err.to_string())),
            }
        }

        Ok(registry)
    }

    fn install_if_missing(
        &mut self,
        package: Arc<Package>,
    ) -> Result<miden_project::Version, InstallPackageError> {
        let version = miden_project::Version::new(package.version.clone(), package.digest());
        log::trace!(target: "package-registry", "preparing to install package {}@{version}", &package.name);
        if let Some(previous_digest) = self
            .packages
            .get(&package.name)
            .and_then(|versions| versions.get(&package.version))
            .and_then(PackageRecord::digest)
            .copied()
            && previous_digest != package.digest()
        {
            log::trace!(target: "package-registry", "package already installed: {}@{version}", &package.name);
            return Err(InstallPackageError::AlreadyInstalledWithDifferentDigest {
                package: package.name.clone(),
                version,
            });
        }

        #[cfg(any(test, feature = "std"))]
        if let Some(filesystem_cache) = self.filesystem_cache.as_deref() {
            package.write_masp_file(filesystem_cache).map_err(|err| {
                InstallPackageError::FilesystemCacheInsertion {
                    package: package.name.clone(),
                    err,
                }
            })?;
        }

        let record = PackageRecord::new(
            version.clone(),
            package.manifest.dependencies().map(|dep| {
                (
                    dep.name.clone(),
                    VersionRequirement::Exact(miden_project::Version::new(
                        dep.version.clone(),
                        dep.digest,
                    )),
                )
            }),
        );
        self.packages
            .entry(package.name.clone())
            .or_default()
            .insert(package.version.clone(), record);

        log::trace!(target: "package-registry", "installed {}@{version}", &package.name);

        self.artifacts
            .entry(package.name.clone())
            .or_default()
            .insert(version.clone(), package);

        Ok(version)
    }
}

/// The extension of a sibling lock file that keeps a fingerprint directory live.
#[cfg(any(test, feature = "std"))]
const BUILD_LOCK_EXTENSION: &str = "lock";

/// Creates and locks the current cache directory, then removes dead stale entries owned by
/// `midenc`.
///
/// Deletion is defense in depth. The primary invalidation is in the FPI expansion itself: it
/// records `option_env!("MIDENC_PACKAGE_CACHE")`, whose value carries the fingerprinted cache
/// path, so Cargo re-expands consumers whenever the fingerprint rotates — even when a stale
/// directory survives here. Pruning still removes the `include_bytes!` targets of expansions
/// made by pre-fingerprint macro versions (the legacy flat files), keeps the parent directory
/// bounded, and takes dead caches out of circulation promptly.
///
/// A build holds a shared `packages/<fingerprint>.lock` lock for its registry's lifetime. Pruning
/// requires the corresponding exclusive lock and holds it while deleting the sibling fingerprint
/// directory, so every live same-input build protects that directory. The lock lives outside the
/// deletable directory and is acquired before that directory is created, closing both prior
/// create-before-lock and unlock-before-delete windows.
///
/// After deletion the pruner closes and removes the now-orphaned lock file. A builder can acquire
/// that file between close and unlink; this residual unlink race is accepted because pruning is
/// hygiene-level defense in depth, while `option_env!("MIDENC_PACKAGE_CACHE")` in FPI expansions
/// provides the correctness boundary. Legacy flat `.masp` files have no lock and retain the
/// accepted one-time race with a pre-fingerprint compiler. Cleanup remains best-effort so it
/// cannot obscure the current build's own diagnostics; package writes still report their failures
/// normally.
#[cfg(any(test, feature = "std"))]
fn prepare_filesystem_cache(filesystem_cache: &std::path::Path) -> Option<std::fs::File> {
    if !is_owned_filesystem_cache_path(filesystem_cache) {
        if let Err(err) = std::fs::create_dir_all(filesystem_cache) {
            log::warn!(
                target: "package-registry",
                "failed to create filesystem package cache '{}': {err}; skipping cache preparation",
                filesystem_cache.display()
            );
            return None;
        }
        log::debug!(
            target: "package-registry",
            "filesystem package cache '{}' is outside the owned miden/packages/<fingerprint> layout; skipping locking and parent pruning",
            filesystem_cache.display()
        );
        return None;
    }

    let parent = filesystem_cache
        .parent()
        .expect("an owned filesystem cache path always has a packages parent");
    if let Err(err) = std::fs::create_dir_all(parent) {
        log::warn!(
            target: "package-registry",
            "failed to create filesystem package cache parent '{}': {err}; skipping cache preparation",
            parent.display()
        );
        return None;
    }
    let filesystem_cache_lock = acquire_filesystem_cache_lock(filesystem_cache)?;
    if let Err(err) = std::fs::create_dir_all(filesystem_cache) {
        log::warn!(
            target: "package-registry",
            "failed to create filesystem package cache '{}': {err}; skipping cache preparation",
            filesystem_cache.display()
        );
        return None;
    }

    let entries = match std::fs::read_dir(parent) {
        Ok(entries) => entries,
        Err(err) => {
            log::debug!(
                target: "package-registry",
                "failed to inspect filesystem package cache '{}': {err}",
                parent.display()
            );
            return Some(filesystem_cache_lock);
        }
    };
    let current_lock_path = filesystem_cache_lock_path(filesystem_cache);

    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(err) => {
                log::debug!(
                    target: "package-registry",
                    "failed to inspect an entry in filesystem package cache '{}': {err}",
                    parent.display()
                );
                continue;
            }
        };
        let path = entry.path();
        if path == filesystem_cache || path == current_lock_path {
            continue;
        }
        let file_type = match entry.file_type() {
            Ok(file_type) => file_type,
            Err(err) => {
                log::debug!(
                    target: "package-registry",
                    "failed to inspect filesystem package cache entry '{}': {err}",
                    path.display()
                );
                continue;
            }
        };

        let is_stale_fingerprint =
            file_type.is_dir() && is_package_cache_fingerprint(&entry.file_name());
        let is_legacy_package = file_type.is_file()
            && path
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case(Package::EXTENSION));
        if is_stale_fingerprint {
            prune_stale_fingerprint(&path, parent);
        } else if is_legacy_package {
            if let Err(err) = std::fs::remove_file(&path) {
                warn_prune_failure(&path, parent, &err);
            }
        } else if file_type.is_file()
            && let Some(fingerprint) = package_cache_fingerprint_from_lock(&entry.file_name())
            && !parent.join(fingerprint).is_dir()
        {
            prune_orphaned_fingerprint_lock(&path, parent);
        }
    }

    Some(filesystem_cache_lock)
}

/// Opens the current fingerprint's sibling lock file and tries to hold a shared builder lock.
#[cfg(any(test, feature = "std"))]
fn acquire_filesystem_cache_lock(filesystem_cache: &std::path::Path) -> Option<std::fs::File> {
    use std::fs::{OpenOptions, TryLockError};

    let lock_path = filesystem_cache_lock_path(filesystem_cache);
    let lock = match OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lock_path)
    {
        Ok(lock) => lock,
        Err(err) => {
            log::warn!(
                target: "package-registry",
                "failed to open filesystem package cache liveness lock '{}': {err}",
                lock_path.display()
            );
            return None;
        }
    };

    match lock.try_lock_shared() {
        Ok(()) => Some(lock),
        Err(TryLockError::WouldBlock) => {
            log::debug!(
                target: "package-registry",
                "filesystem package cache '{}' is being pruned; skipping cache preparation",
                filesystem_cache.display()
            );
            None
        }
        Err(TryLockError::Error(err)) => {
            log::warn!(
                target: "package-registry",
                "failed to lock filesystem package cache '{}': {err}",
                filesystem_cache.display()
            );
            None
        }
    }
}

/// Deletes a stale fingerprint directory while holding its exclusive sibling lock.
#[cfg(any(test, feature = "std"))]
fn prune_stale_fingerprint(fingerprint_dir: &std::path::Path, parent: &std::path::Path) {
    use std::fs::{OpenOptions, TryLockError};

    let lock_path = filesystem_cache_lock_path(fingerprint_dir);
    let lock = match OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lock_path)
    {
        Ok(lock) => lock,
        Err(err) => {
            log::warn!(
                target: "package-registry",
                "cannot verify liveness of stale filesystem package cache '{}': {err}; skipping deletion",
                fingerprint_dir.display()
            );
            return;
        }
    };

    match lock.try_lock() {
        Ok(()) => {
            if let Err(err) = std::fs::remove_dir_all(fingerprint_dir) {
                warn_prune_failure(fingerprint_dir, parent, &err);
                return;
            }
            drop(lock);
            remove_orphaned_lock_file(&lock_path, fingerprint_dir, parent);
        }
        Err(TryLockError::WouldBlock) => {
            log::debug!(
                target: "package-registry",
                "skipping live filesystem package cache '{}' during stale-cache pruning",
                fingerprint_dir.display()
            )
        }
        Err(TryLockError::Error(err)) => {
            log::warn!(
                target: "package-registry",
                "cannot verify liveness of stale filesystem package cache '{}': {err}; skipping deletion",
                fingerprint_dir.display()
            )
        }
    }
}

/// Deletes an orphaned sibling lock after verifying that no builder holds it.
#[cfg(any(test, feature = "std"))]
fn prune_orphaned_fingerprint_lock(lock_path: &std::path::Path, parent: &std::path::Path) {
    use std::{fs::TryLockError, io::ErrorKind};

    let lock = match std::fs::File::open(lock_path) {
        Ok(lock) => lock,
        Err(err) if err.kind() == ErrorKind::NotFound => return,
        Err(err) => {
            log::warn!(
                target: "package-registry",
                "cannot verify liveness of orphaned filesystem package cache lock '{}': {err}; skipping deletion",
                lock_path.display()
            );
            return;
        }
    };
    match lock.try_lock() {
        Ok(()) => {
            drop(lock);
            let fingerprint_dir = lock_path.with_extension("");
            remove_orphaned_lock_file(lock_path, &fingerprint_dir, parent);
        }
        Err(TryLockError::WouldBlock) => {
            log::debug!(
                target: "package-registry",
                "skipping live filesystem package cache lock '{}' during orphan cleanup",
                lock_path.display()
            )
        }
        Err(TryLockError::Error(err)) => {
            log::warn!(
                target: "package-registry",
                "cannot verify liveness of orphaned filesystem package cache lock '{}': {err}; skipping deletion",
                lock_path.display()
            )
        }
    }
}

/// Removes an unlocked lock file if its fingerprint directory remains absent.
#[cfg(any(test, feature = "std"))]
fn remove_orphaned_lock_file(
    lock_path: &std::path::Path,
    fingerprint_dir: &std::path::Path,
    parent: &std::path::Path,
) {
    use std::io::ErrorKind;

    if fingerprint_dir.exists() {
        return;
    }
    if let Err(err) = std::fs::remove_file(lock_path)
        && err.kind() != ErrorKind::NotFound
    {
        warn_prune_failure(lock_path, parent, &err);
    }
}

/// Logs a best-effort cleanup failure with the exact directory a user can remove.
#[cfg(any(test, feature = "std"))]
fn warn_prune_failure(path: &std::path::Path, parent: &std::path::Path, err: &std::io::Error) {
    log::warn!(
        target: "package-registry",
        "failed to prune stale filesystem package cache entry '{}': {err}; stale cache entries may survive; delete '{}' manually",
        path.display(),
        parent.display()
    );
}

/// Returns the sibling lock path associated with a fingerprint directory.
#[cfg(any(test, feature = "std"))]
fn filesystem_cache_lock_path(filesystem_cache: &std::path::Path) -> std::path::PathBuf {
    filesystem_cache.with_extension(BUILD_LOCK_EXTENSION)
}

/// Returns true when a path is owned by the `miden/packages/<fingerprint>` cache layout.
#[cfg(any(test, feature = "std"))]
fn is_owned_filesystem_cache_path(filesystem_cache: &std::path::Path) -> bool {
    use std::ffi::OsStr;

    filesystem_cache.file_name().is_some_and(is_package_cache_fingerprint)
        && filesystem_cache
            .parent()
            .and_then(std::path::Path::file_name)
            .is_some_and(|name| name == OsStr::new("packages"))
        && filesystem_cache
            .parent()
            .and_then(std::path::Path::parent)
            .and_then(std::path::Path::file_name)
            .is_some_and(|name| name == OsStr::new("miden"))
}

/// Extracts a fingerprint from an owned sibling `<fingerprint>.lock` filename.
#[cfg(any(test, feature = "std"))]
fn package_cache_fingerprint_from_lock(name: &std::ffi::OsStr) -> Option<&str> {
    let fingerprint = name.to_str()?.strip_suffix(".lock")?;
    crate::package_cache::is_fingerprint(fingerprint).then_some(fingerprint)
}

/// Returns true when `name` has the cache fingerprint format owned by `midenc`.
#[cfg(any(test, feature = "std"))]
fn is_package_cache_fingerprint(name: &std::ffi::OsStr) -> bool {
    name.to_str().is_some_and(crate::package_cache::is_fingerprint)
}

impl HybridPackageRegistry {
    fn insert_record(&mut self, id: PackageId, record: PackageRecord) {
        self.packages
            .entry(id)
            .or_default()
            .insert(record.semantic_version().clone(), record);
    }
}

impl PackageRegistry for HybridPackageRegistry {
    fn available_versions(&self, package: &PackageId) -> Option<&PackageVersions> {
        self.packages.get(package)
    }
}

impl PackageIndex for HybridPackageRegistry {
    type Error = Report;

    fn register(&mut self, name: PackageId, record: PackageRecord) -> Result<(), Self::Error> {
        if self.is_semver_available(&name, record.semantic_version()) {
            return Err(Report::msg(format!(
                "cannot register {name}: version {} is already registered",
                record.semantic_version()
            )));
        }
        self.insert_record(name, record);
        Ok(())
    }
}

impl PackageProvider for HybridPackageRegistry {
    fn load_package(
        &self,
        package: &PackageId,
        version: &miden_project::Version,
    ) -> Result<Arc<Package>, Report> {
        let found = self.artifacts.get(package).and_then(|versions| versions.get(&version.version));
        match found {
            Some(artifact) if version.digest != Some(artifact.digest()) => {
                Err(Report::msg(format!(
                    "cannot load {package}@{version}: a specific digest was requested, but \
                     differs from the available version"
                )))
            }
            Some(artifact) => Ok(Arc::clone(artifact)),
            None => Err(Report::msg(format!(
                "cannot load {package}@{version}: no such package available",
            ))),
        }
    }
}

impl PackageCache for HybridPackageRegistry {
    type Error = Report;

    fn cache_package(
        &mut self,
        package: Arc<Package>,
    ) -> Result<miden_project::Version, Self::Error> {
        self.install_if_missing(package).map_err(Report::from)
    }
}

impl PackageStore for HybridPackageRegistry {
    fn publish_package(
        &mut self,
        package: Arc<Package>,
    ) -> Result<miden_project::Version, Self::Error> {
        self.install_if_missing(package).map_err(Report::from)
    }
}

#[cfg(test)]
mod tests {
    use std::fs::OpenOptions;

    use tempfile::TempDir;

    use super::*;

    #[test]
    fn install_checks_conflicts_before_writing_and_rewrites_accepted_packages() {
        let temp = TempDir::new().unwrap();
        let cache = temp.path().join("cache");
        std::fs::create_dir_all(&cache).unwrap();

        let options = crate::Options::default();
        let package = crate::LinkLibrary::core().load(&options).unwrap();
        let package_name: &str = &package.name;
        let cached_package = cache.join(package_name).with_extension(Package::EXTENSION);
        let mut registry = HybridPackageRegistry::empty();
        registry.filesystem_cache = Some(cache);

        registry.install_if_missing(Arc::clone(&package)).unwrap();
        std::fs::write(&cached_package, b"damaged").unwrap();
        registry.install_if_missing(Arc::clone(&package)).unwrap();
        assert_ne!(
            std::fs::read(&cached_package).unwrap(),
            b"damaged",
            "an accepted same-digest install must repair the cached package"
        );

        let mut conflict = (*crate::LinkLibrary::tx_kernel().load(&options).unwrap()).clone();
        conflict.name = package.name.clone();
        conflict.version = package.version.clone();
        assert_ne!(conflict.digest(), package.digest());
        std::fs::write(&cached_package, b"keep-on-conflict").unwrap();

        assert!(matches!(
            registry.install_if_missing(Arc::new(conflict)),
            Err(InstallPackageError::AlreadyInstalledWithDifferentDigest { .. })
        ));
        assert_eq!(
            std::fs::read(cached_package).unwrap(),
            b"keep-on-conflict",
            "a rejected install must not touch the cached package"
        );
    }

    #[test]
    fn creating_a_filesystem_cache_prunes_only_stale_owned_entries() {
        let temp = TempDir::new().unwrap();
        let parent = temp.path().join("miden").join("packages");
        let current = parent.join("fedcba9876543210");
        let stale = parent.join("0123456789abcdef");
        let unrelated_directory = parent.join("not-a-midenc-cache");
        let uppercase_directory = parent.join("ABCDEF0123456789");
        let legacy_package = parent.join("legacy.masp");
        let uppercase_legacy_package = parent.join("uppercase.MASP");
        let orphan_lock = parent.join("1111111111111111.lock");
        let live_precreation_lock_path = parent.join("2222222222222222.lock");
        let unrelated_file = parent.join("keep.txt");

        for directory in [&current, &stale, &unrelated_directory, &uppercase_directory] {
            std::fs::create_dir_all(directory).unwrap();
        }
        let current_marker = current.join("keep");
        std::fs::write(&current_marker, b"current").unwrap();
        std::fs::write(stale.join("old.masp"), b"stale").unwrap();
        std::fs::write(&legacy_package, b"legacy").unwrap();
        std::fs::write(&uppercase_legacy_package, b"legacy").unwrap();
        std::fs::write(&orphan_lock, b"").unwrap();
        let live_precreation_lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&live_precreation_lock_path)
            .unwrap();
        live_precreation_lock.try_lock_shared().unwrap();
        std::fs::write(&unrelated_file, b"unrelated").unwrap();

        let current_lock =
            prepare_filesystem_cache(&current).expect("current cache must be locked");

        assert!(current_marker.exists(), "the current cache must remain intact");
        assert!(!stale.exists(), "a stale fingerprint directory must be removed");
        assert!(!filesystem_cache_lock_path(&stale).exists(), "the stale lock must be removed");
        assert!(!legacy_package.exists(), "a legacy flat package must be removed");
        assert!(
            !uppercase_legacy_package.exists(),
            "legacy package extensions must be matched case-insensitively"
        );
        assert!(!orphan_lock.exists(), "an unlocked orphan fingerprint lock must be removed");
        assert!(
            live_precreation_lock_path.exists(),
            "a lock held before its directory is created must survive orphan cleanup"
        );
        assert!(unrelated_directory.exists(), "unowned directories must be retained");
        assert!(uppercase_directory.exists(), "non-lowercase directories must be retained");
        assert!(unrelated_file.exists(), "unowned files must be retained");

        drop(live_precreation_lock);
        drop(current_lock);
    }

    #[test]
    fn constructor_prepares_and_locks_the_filesystem_cache() {
        let temp = TempDir::new().unwrap();
        let current = temp.path().join("miden").join("packages").join("fedcba9876543210");

        let registry = HybridPackageRegistry::new_with_filesystem_cache(
            &crate::Options::default(),
            Some(current.clone()),
        )
        .unwrap();

        assert_eq!(registry.filesystem_cache_dir(), Some(current.as_path()));
        assert!(current.is_dir());
        assert!(registry.filesystem_cache_lock.is_some());
        let contender = OpenOptions::new()
            .read(true)
            .write(true)
            .open(filesystem_cache_lock_path(&current))
            .unwrap();
        contender.try_lock_shared().unwrap();
        let stale_checker = OpenOptions::new()
            .read(true)
            .write(true)
            .open(filesystem_cache_lock_path(&current))
            .unwrap();
        assert!(matches!(stale_checker.try_lock(), Err(std::fs::TryLockError::WouldBlock)));
    }

    #[test]
    fn live_stale_fingerprint_survives_until_its_lock_is_released() {
        let temp = TempDir::new().unwrap();
        let parent = temp.path().join("miden").join("packages");
        let current = parent.join("fedcba9876543210");
        let stale = parent.join("0123456789abcdef");
        std::fs::create_dir_all(&stale).unwrap();

        let stale_lock_path = filesystem_cache_lock_path(&stale);
        let stale_lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&stale_lock_path)
            .unwrap();
        stale_lock.try_lock_shared().unwrap();

        let current_lock =
            prepare_filesystem_cache(&current).expect("current cache must be locked");
        assert!(filesystem_cache_lock_path(&current).exists());
        let current_contender = OpenOptions::new()
            .read(true)
            .write(true)
            .open(filesystem_cache_lock_path(&current))
            .unwrap();
        current_contender.try_lock_shared().unwrap();
        assert!(stale.exists(), "a live sibling cache must not be pruned");

        drop(stale_lock);
        let second_lock =
            prepare_filesystem_cache(&current).expect("same-input builders share the lock");
        assert!(!stale.exists(), "the stale cache must be pruned after its build exits");
        assert!(!stale_lock_path.exists(), "the stale sibling lock must be removed");

        drop(second_lock);
        drop(current_lock);
    }

    #[test]
    fn same_fingerprint_contender_remains_live_after_first_builder_exits() {
        let temp = TempDir::new().unwrap();
        let parent = temp.path().join("miden").join("packages");
        let shared = parent.join("fedcba9876543210");
        let different = parent.join("0123456789abcdef");

        let first = prepare_filesystem_cache(&shared).expect("first builder must lock the cache");
        let contender =
            prepare_filesystem_cache(&shared).expect("same-input contender must share the lock");
        drop(first);

        let different_lock = prepare_filesystem_cache(&different)
            .expect("different-input builder must lock its cache");

        assert!(shared.exists(), "the live contender's cache must not be pruned");

        drop(different_lock);
        drop(contender);
    }

    #[test]
    fn cache_creation_failure_does_not_sweep_siblings() {
        let temp = TempDir::new().unwrap();
        let parent = temp.path().join("miden").join("packages");
        let current = parent.join("fedcba9876543210");
        let stale = parent.join("0123456789abcdef");
        std::fs::create_dir_all(&stale).unwrap();
        std::fs::write(&current, b"not a directory").unwrap();

        let lock = prepare_filesystem_cache(&current);

        assert!(lock.is_none());
        assert!(stale.exists(), "siblings must survive when the current cache cannot be created");
    }

    #[test]
    fn arbitrary_cache_path_cannot_sweep_its_parent() {
        let temp = TempDir::new().unwrap();
        let parent = temp.path().join("arbitrary-parent");
        let current = parent.join("cache");
        let fingerprint_sibling = parent.join("0123456789abcdef");
        let package_sibling = parent.join("unrelated.masp");
        std::fs::create_dir_all(&fingerprint_sibling).unwrap();
        std::fs::write(&package_sibling, b"unrelated").unwrap();

        let lock = prepare_filesystem_cache(&current);

        assert!(lock.is_none());
        assert!(current.is_dir(), "an arbitrary cache path is still created");
        assert!(!filesystem_cache_lock_path(&current).exists());
        assert!(fingerprint_sibling.exists());
        assert!(package_sibling.exists());
    }

    #[test]
    fn fingerprint_name_outside_owned_layout_cannot_sweep_its_parent() {
        let temp = TempDir::new().unwrap();
        let parent = temp.path().join("shared");
        let current = parent.join("fedcba9876543210");
        let fingerprint_sibling = parent.join("0123456789abcdef");
        let package_sibling = parent.join("unrelated.masp");
        std::fs::create_dir_all(&fingerprint_sibling).unwrap();
        std::fs::write(&package_sibling, b"unrelated").unwrap();

        let lock = prepare_filesystem_cache(&current);

        assert!(lock.is_none());
        assert!(current.is_dir(), "an out-of-layout cache path is still created");
        assert!(!filesystem_cache_lock_path(&current).exists());
        assert!(fingerprint_sibling.exists());
        assert!(package_sibling.exists());
    }
}
