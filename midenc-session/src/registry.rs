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
        }
    }

    /// Get a new instance of the registry, using the current compiler options
    #[cfg(any(test, feature = "std"))]
    pub fn new(options: &crate::Options) -> Result<Self, Report> {
        Self::new_with_filesystem_cache(options, None)
    }

    /// Get a new instance of the registry, using the current compiler options and an optional
    /// filesystem cache directory
    #[cfg(any(test, feature = "std"))]
    pub fn new_with_filesystem_cache(
        options: &crate::Options,
        filesystem_cache: Option<std::path::PathBuf>,
    ) -> Result<Self, Report> {
        use alloc::string::ToString;

        if let Some(filesystem_cache) = filesystem_cache.as_deref() {
            prepare_filesystem_cache(filesystem_cache);
        }

        // Load system libraries
        let mut registry = if options.sysroot.is_some() {
            Self::from_local_registry(options)?
        } else {
            Self::empty()
        };
        registry.filesystem_cache = filesystem_cache;

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

/// Creates the current cache directory and removes stale cache entries owned by `midenc`.
///
/// Cleanup is best-effort because an inability to prune an old build must not obscure the
/// diagnostic from the current build. Package writes still report their own failures normally.
#[cfg(any(test, feature = "std"))]
fn prepare_filesystem_cache(filesystem_cache: &std::path::Path) {
    if let Err(err) = std::fs::create_dir_all(filesystem_cache) {
        log::debug!(
            target: "package-registry",
            "failed to create filesystem package cache '{}': {err}",
            filesystem_cache.display()
        );
    }

    let Some(parent) = filesystem_cache.parent() else {
        return;
    };
    let entries = match std::fs::read_dir(parent) {
        Ok(entries) => entries,
        Err(err) => {
            log::debug!(
                target: "package-registry",
                "failed to inspect filesystem package cache '{}': {err}",
                parent.display()
            );
            return;
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
            std::fs::remove_dir_all(&path)
        } else {
            std::fs::remove_file(&path)
        };
        if let Err(err) = result {
            log::debug!(
                target: "package-registry",
                "failed to prune stale filesystem package cache entry '{}': {err}",
                path.display()
            );
        }
    }
}

/// Returns true when `name` has the cache fingerprint format owned by `midenc`.
#[cfg(any(test, feature = "std"))]
fn is_package_cache_fingerprint(name: &std::ffi::OsStr) -> bool {
    name.to_str().is_some_and(|name| {
        name.len() == 16
            && name.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    })
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

        let registry = HybridPackageRegistry::new_with_filesystem_cache(
            &crate::Options::default(),
            Some(current.clone()),
        )
        .unwrap();

        assert_eq!(registry.filesystem_cache_dir(), Some(current.as_path()));
        assert!(current_marker.exists(), "the current cache must remain intact");
        assert!(!stale.exists(), "a stale fingerprint directory must be removed");
        assert!(!legacy_package.exists(), "a legacy flat package must be removed");
        assert!(unrelated_directory.exists(), "unowned directories must be retained");
        assert!(uppercase_directory.exists(), "non-lowercase directories must be retained");
        assert!(unrelated_file.exists(), "unowned files must be retained");
    }
}
