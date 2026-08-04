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
    /// A cache path whose final component is a `midenc` fingerprint is created and locked for the
    /// registry's lifetime. During construction, dead sibling fingerprint directories and legacy
    /// flat `.masp` entries are pruned. A path that does not satisfy the fingerprint format is
    /// created but deliberately neither locked nor used to sweep its parent.
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
            if path.extension().is_none_or(|ext| !ext.eq_ignore_ascii_case("masp")) {
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
        use alloc::collections::btree_map::Entry as BTreeMapEntry;

        use hashbrown::hash_map::Entry;

        #[cfg(any(test, feature = "std"))]
        if let Some(filesystem_cache) = self.filesystem_cache.as_deref() {
            package.write_masp_file(filesystem_cache).map_err(|err| {
                InstallPackageError::FilesystemCacheInsertion {
                    package: package.name.clone(),
                    err,
                }
            })?;
        }

        let version = miden_project::Version::new(package.version.clone(), package.digest());
        log::trace!(target: "package-registry", "preparing to install package {}@{version}", &package.name);
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
        match self.packages.entry(package.name.clone()) {
            Entry::Occupied(mut entry) => {
                let versions = entry.get_mut();
                match versions.entry(package.version.clone()) {
                    BTreeMapEntry::Occupied(mut prev) => {
                        let prev_digest = prev.get().digest().copied();
                        if prev_digest.is_none_or(|prev_digest| prev_digest == package.digest()) {
                            prev.insert(record);
                        } else {
                            log::trace!(target: "package-registry", "package already installed: {}@{version}", &package.name);
                            return Err(InstallPackageError::AlreadyInstalledWithDifferentDigest {
                                package: package.name.clone(),
                                version,
                            });
                        }
                    }
                    BTreeMapEntry::Vacant(entry) => {
                        entry.insert(record);
                    }
                }
            }
            Entry::Vacant(entry) => {
                entry.insert([(package.version.clone(), record)].into_iter().collect());
            }
        }

        log::trace!(target: "package-registry", "installed {}@{version}", &package.name);

        self.artifacts
            .entry(package.name.clone())
            .or_default()
            .insert(version.clone(), package);

        Ok(version)
    }
}

/// The filename used to keep a fingerprint directory live while its build registry exists.
#[cfg(any(test, feature = "std"))]
const BUILD_LOCK_FILENAME: &str = ".build-lock";

/// Creates and locks the current cache directory, then removes dead stale entries owned by
/// `midenc`.
///
/// Deletion is correctness-critical rather than housekeeping. The FPI macro leaves an
/// `include_bytes!` reference to the package path in its expansion; if an old target survives,
/// Cargo can reuse that expansion and preserve stale procedure roots. Removing the old target
/// forces re-expansion.
///
/// Each build tries to hold an exclusive [BUILD_LOCK_FILENAME] lock for its registry's lifetime.
/// A sibling fingerprint is dead when its lock file is absent or can be locked, and live when the
/// lock would block. The acquired stale lock is closed before deletion for Windows compatibility,
/// leaving a microscopic accepted race in which another process can re-lock the file before
/// `remove_dir_all`. Legacy flat `.masp` files have no lock and retain the accepted one-time race
/// with a pre-fingerprint compiler. Cleanup remains best-effort so it cannot obscure the current
/// build's own diagnostics; package writes still report their failures normally.
#[cfg(any(test, feature = "std"))]
fn prepare_filesystem_cache(filesystem_cache: &std::path::Path) -> Option<std::fs::File> {
    if let Err(err) = std::fs::create_dir_all(filesystem_cache) {
        log::debug!(
            target: "package-registry",
            "failed to create filesystem package cache '{}': {err}",
            filesystem_cache.display()
        );
    }
    if !filesystem_cache.file_name().is_some_and(is_package_cache_fingerprint) {
        log::debug!(
            target: "package-registry",
            "filesystem package cache '{}' is not fingerprint-named; skipping locking and parent pruning",
            filesystem_cache.display()
        );
        return None;
    }
    let filesystem_cache_lock = acquire_filesystem_cache_lock(filesystem_cache);

    let Some(parent) = filesystem_cache.parent() else {
        return filesystem_cache_lock;
    };
    let entries = match std::fs::read_dir(parent) {
        Ok(entries) => entries,
        Err(err) => {
            log::debug!(
                target: "package-registry",
                "failed to inspect filesystem package cache '{}': {err}",
                parent.display()
            );
            return filesystem_cache_lock;
        }
    };

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
        if path == filesystem_cache {
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
            && path.extension().and_then(|extension| extension.to_str()) == Some("masp");
        if !is_stale_fingerprint && !is_legacy_package {
            continue;
        }

        let result = if is_stale_fingerprint {
            if !stale_fingerprint_can_be_pruned(&path) {
                continue;
            }
            std::fs::remove_dir_all(&path)
        } else {
            std::fs::remove_file(&path)
        };
        if let Err(err) = result {
            log::warn!(
                target: "package-registry",
                "failed to prune stale filesystem package cache entry '{}': {err}; stale macro expansions may survive; delete target/miden/packages manually",
                path.display()
            );
        }
    }

    filesystem_cache_lock
}

/// Opens the current fingerprint's lock file and tries to hold it for the registry lifetime.
#[cfg(any(test, feature = "std"))]
fn acquire_filesystem_cache_lock(filesystem_cache: &std::path::Path) -> Option<std::fs::File> {
    use std::fs::{OpenOptions, TryLockError};

    let lock_path = filesystem_cache.join(BUILD_LOCK_FILENAME);
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

    match lock.try_lock() {
        Ok(()) => Some(lock),
        Err(TryLockError::WouldBlock) => {
            log::debug!(
                target: "package-registry",
                "filesystem package cache '{}' is already protected by an identical-input build",
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

/// Returns true when a stale fingerprint directory is not protected by a live build.
#[cfg(any(test, feature = "std"))]
fn stale_fingerprint_can_be_pruned(fingerprint_dir: &std::path::Path) -> bool {
    use std::{
        fs::{File, TryLockError},
        io::ErrorKind,
    };

    let lock_path = fingerprint_dir.join(BUILD_LOCK_FILENAME);
    let lock = match File::open(&lock_path) {
        Ok(lock) => lock,
        Err(err) if err.kind() == ErrorKind::NotFound => return true,
        Err(err) => {
            log::warn!(
                target: "package-registry",
                "cannot verify liveness of stale filesystem package cache '{}': {err}; skipping deletion",
                fingerprint_dir.display()
            );
            return false;
        }
    };

    match lock.try_lock() {
        Ok(()) => {
            if let Err(err) = lock.unlock() {
                log::debug!(
                    target: "package-registry",
                    "failed to explicitly unlock stale filesystem package cache '{}': {err}; closing the lock file",
                    fingerprint_dir.display()
                );
            }
            drop(lock);
            true
        }
        Err(TryLockError::WouldBlock) => {
            log::debug!(
                target: "package-registry",
                "skipping live filesystem package cache '{}' during stale-cache pruning",
                fingerprint_dir.display()
            );
            false
        }
        Err(TryLockError::Error(err)) => {
            log::warn!(
                target: "package-registry",
                "cannot verify liveness of stale filesystem package cache '{}': {err}; skipping deletion",
                fingerprint_dir.display()
            );
            false
        }
    }
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
    fn creating_a_filesystem_cache_prunes_only_stale_owned_entries() {
        let temp = TempDir::new().unwrap();
        let parent = temp.path().join("packages");
        let current = parent.join("fedcba9876543210");
        let stale = parent.join("0123456789abcdef");
        let unrelated_directory = parent.join("not-a-midenc-cache");
        let uppercase_directory = parent.join("ABCDEF0123456789");
        let legacy_package = parent.join("legacy.masp");
        let unrelated_file = parent.join("keep.txt");

        for directory in [&current, &stale, &unrelated_directory, &uppercase_directory] {
            std::fs::create_dir_all(directory).unwrap();
        }
        let current_marker = current.join("keep");
        std::fs::write(&current_marker, b"current").unwrap();
        std::fs::write(stale.join("old.masp"), b"stale").unwrap();
        std::fs::write(&legacy_package, b"legacy").unwrap();
        std::fs::write(&unrelated_file, b"unrelated").unwrap();

        let current_lock =
            prepare_filesystem_cache(&current).expect("current cache must be locked");

        assert!(current_marker.exists(), "the current cache must remain intact");
        assert!(!stale.exists(), "a stale fingerprint directory must be removed");
        assert!(!legacy_package.exists(), "a legacy flat package must be removed");
        assert!(unrelated_directory.exists(), "unowned directories must be retained");
        assert!(uppercase_directory.exists(), "non-lowercase directories must be retained");
        assert!(unrelated_file.exists(), "unowned files must be retained");

        drop(current_lock);
    }

    #[test]
    fn constructor_prepares_and_locks_the_filesystem_cache() {
        let temp = TempDir::new().unwrap();
        let current = temp.path().join("packages").join("fedcba9876543210");

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
            .open(current.join(BUILD_LOCK_FILENAME))
            .unwrap();
        assert!(matches!(contender.try_lock(), Err(std::fs::TryLockError::WouldBlock)));
    }

    #[test]
    fn live_stale_fingerprint_survives_until_its_lock_is_released() {
        let temp = TempDir::new().unwrap();
        let parent = temp.path().join("packages");
        let current = parent.join("fedcba9876543210");
        let stale = parent.join("0123456789abcdef");
        std::fs::create_dir_all(&stale).unwrap();

        let stale_lock_path = stale.join(BUILD_LOCK_FILENAME);
        let stale_lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&stale_lock_path)
            .unwrap();
        stale_lock.try_lock().unwrap();

        let current_lock =
            prepare_filesystem_cache(&current).expect("current cache must be locked");
        assert!(current.join(BUILD_LOCK_FILENAME).exists());
        let current_contender = OpenOptions::new()
            .read(true)
            .write(true)
            .open(current.join(BUILD_LOCK_FILENAME))
            .unwrap();
        assert!(matches!(current_contender.try_lock(), Err(std::fs::TryLockError::WouldBlock)));
        assert!(stale.exists(), "a live sibling cache must not be pruned");

        drop(stale_lock);
        let second_lock = prepare_filesystem_cache(&current);
        assert!(second_lock.is_none(), "the first current-cache lock is still held");
        assert!(!stale.exists(), "the stale cache must be pruned after its build exits");

        drop(current_lock);
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
        assert!(!current.join(BUILD_LOCK_FILENAME).exists());
        assert!(fingerprint_sibling.exists());
        assert!(package_sibling.exists());
    }
}
