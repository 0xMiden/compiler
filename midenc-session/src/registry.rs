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
    /// Keeps the owning session's package-cache lease alive for this registry's lifetime.
    ///
    /// Never read: the field exists so a leased cache directory cannot be deleted while a
    /// registry that publishes into it is still live, even after every `Session` clone is
    /// dropped.
    #[cfg(feature = "std")]
    _filesystem_cache_lease: Option<crate::package_lease::SharedPackageCacheLease>,
}

impl HybridPackageRegistry {
    #[cfg(any(test, feature = "std"))]
    pub fn filesystem_cache_dir(&self) -> Option<&std::path::Path> {
        self.filesystem_cache.as_deref()
    }

    /// Keeps the session's package-cache lease alive for this registry's lifetime.
    ///
    /// Called by [`crate::Session::package_registry`] after construction; see the
    /// `_filesystem_cache_lease` field for why.
    #[cfg(feature = "std")]
    pub(crate) fn retain_session_package_cache(
        &mut self,
        lease: crate::package_lease::SharedPackageCacheLease,
    ) {
        self._filesystem_cache_lease = Some(lease);
    }

    /// Get an empty, uninitialized registry
    pub fn empty() -> Self {
        Self {
            packages: Default::default(),
            artifacts: Default::default(),
            filesystem_cache: None,
            #[cfg(feature = "std")]
            _filesystem_cache_lease: None,
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
    /// The directory — typically the session's per-build package-exchange lease — is created
    /// when possible, and every package installed into the registry is published into it. A
    /// caller-supplied path is used exactly as given: nothing beside it is ever touched, and
    /// its lifetime belongs to the caller.
    ///
    /// A creation failure keeps the cache configured, so the first package publication
    /// reports the concrete filesystem error to the caller instead of silently compiling
    /// without a package exchange.
    #[cfg(any(test, feature = "std"))]
    pub fn new_with_filesystem_cache(
        options: &crate::Options,
        filesystem_cache: Option<std::path::PathBuf>,
    ) -> Result<Self, Report> {
        if let Some(filesystem_cache) = filesystem_cache.as_deref()
            && let Err(err) = std::fs::create_dir_all(filesystem_cache)
        {
            log::warn!(
                target: "package-registry",
                "failed to create filesystem package cache '{}': {err}; keeping the cache configured so package publication reports the failure",
                filesystem_cache.display()
            );
        }
        Self::construct(options, filesystem_cache)
    }

    /// Builds the registry with system libraries, link libraries, and the given cache state.
    #[cfg(any(test, feature = "std"))]
    fn construct(
        options: &crate::Options,
        filesystem_cache: Option<std::path::PathBuf>,
    ) -> Result<Self, Report> {
        use alloc::string::ToString;

        // Load system libraries
        let mut registry = if options.sysroot.is_some() {
            Self::from_local_registry(options)?
        } else {
            Self::empty()
        };
        registry.filesystem_cache = filesystem_cache;
        #[cfg(feature = "std")]
        {
            registry._filesystem_cache_lease = None;
        }

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
            write_package_atomically(&package, filesystem_cache).map_err(|err| {
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

/// Publishes `package` into `filesystem_cache` with an atomic replacement of the final path.
#[cfg(any(test, feature = "std"))]
fn write_package_atomically(
    package: &Package,
    filesystem_cache: &std::path::Path,
) -> std::io::Result<()> {
    use std::{
        ffi::OsString,
        io::{Error, ErrorKind},
        sync::atomic::{AtomicU64, Ordering},
    };

    static NEXT_TEMP_ID: AtomicU64 = AtomicU64::new(0);

    let package_name: &str = &package.name;
    let destination = filesystem_cache.join(package_name).with_extension(Package::EXTENSION);
    let destination_name = destination.file_name().ok_or_else(|| {
        Error::new(
            ErrorKind::InvalidInput,
            format!("package cache destination '{}' has no file name", destination.display()),
        )
    })?;
    let mut temp_name = OsString::from(".");
    temp_name.push(destination_name);
    temp_name.push(format!(
        ".tmp-{}-{}",
        std::process::id(),
        NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed)
    ));
    let temp_path = destination.with_file_name(temp_name);

    let result = package
        .write_to_file(&temp_path)
        .and_then(|()| std::fs::rename(&temp_path, &destination));
    if result.is_err() {
        let _ = std::fs::remove_file(&temp_path);
    }
    result
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
        assert!(
            std::fs::read_dir(cached_package.parent().unwrap()).unwrap().all(|entry| !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .contains(".tmp-")),
            "successful publication must not leave its temporary file behind"
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
            std::fs::read(&cached_package).unwrap(),
            b"keep-on-conflict",
            "a rejected install must not touch the cached package"
        );

        std::fs::remove_file(&cached_package).unwrap();
        std::fs::create_dir(&cached_package).unwrap();
        assert!(matches!(
            registry.install_if_missing(Arc::clone(&package)),
            Err(InstallPackageError::FilesystemCacheInsertion { .. })
        ));
        assert!(
            std::fs::read_dir(cached_package.parent().unwrap()).unwrap().all(|entry| !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .contains(".tmp-")),
            "failed publication must clean up its temporary file"
        );
    }

    #[test]
    fn constructor_creates_the_cache_and_leaves_its_siblings_alone() {
        let temp = TempDir::new().unwrap();
        let parent = temp.path().join("miden").join("packages");
        let current = parent.join("build-current");
        let sibling = parent.join("build-sibling");
        std::fs::create_dir_all(&sibling).unwrap();

        let registry = HybridPackageRegistry::new_with_filesystem_cache(
            &crate::Options::default(),
            Some(current.clone()),
        )
        .unwrap();

        assert_eq!(registry.filesystem_cache_dir(), Some(current.as_path()));
        assert!(current.is_dir(), "the configured cache directory is created");
        assert!(sibling.exists(), "a sibling directory must never be swept");
    }
}
