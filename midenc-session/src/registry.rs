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

        // Configure publication before loading any packages. Registry-resolved packages are
        // part of the dependency artifact exchange just like packages assembled from source;
        // loading the sysroot first would leave those artifacts only in memory while the
        // compiler records cache-local paths for them.
        let mut registry = Self::empty();
        registry.filesystem_cache = filesystem_cache;
        if options.sysroot.is_some() {
            registry.load_local_registry(options)?;
        }

        // Load link libraries. The precompiles library is implied because the core library
        // depends on it; the project assembler resolves and verifies the dependency itself.
        let core = crate::LinkLibrary::core();
        let precompiles = crate::LinkLibrary::precompiles();
        let tx_kernel = crate::LinkLibrary::tx_kernel();
        let protocol = crate::LinkLibrary::protocol();
        let implied_libraries = vec![&core, &precompiles, &tx_kernel, &protocol]
            .into_iter()
            .filter(|ll| !options.link_libraries.iter().any(|oll| oll.name == ll.name));
        let link_libraries = options.link_libraries.iter().chain(implied_libraries);
        for lib in link_libraries {
            let package = lib.load(options)?;
            let file_name =
                midenc_frontend_wasm_metadata::package_cache::registry_package_file_name(
                    &package.name,
                    &package.version,
                );
            match registry.install_if_missing_as(package, Some(&file_name)) {
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
        let mut registry = Self::empty();
        registry.load_local_registry(options)?;
        Ok(registry)
    }

    /// Loads packages from the configured local registry into this registry.
    ///
    /// Unlike [`Self::from_local_registry`], this preserves the receiver's configured
    /// filesystem cache, so callers that publish an artifact exchange can configure it before
    /// any sysroot package is installed.
    #[cfg(any(test, feature = "std"))]
    fn load_local_registry(&mut self, options: &crate::Options) -> Result<(), Report> {
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

        for entry in entries {
            let Ok(entry) = entry else {
                continue;
            };
            let path = entry.path();
            if path.extension().is_none_or(|ext| !ext.eq_ignore_ascii_case(Package::EXTENSION)) {
                continue;
            }

            let package = crate::libs::load_package_from_path(&path)?;
            let file_name =
                midenc_frontend_wasm_metadata::package_cache::registry_package_file_name(
                    &package.name,
                    &package.version,
                );
            match self.install_if_missing_as(package, Some(&file_name)) {
                Ok(_) => (),
                // Ignore duplicates when initializing the registry
                Err(InstallPackageError::AlreadyInstalledWithDifferentDigest { .. }) => (),
                Err(err) => return Err(Report::msg(err.to_string())),
            }
        }

        Ok(())
    }

    fn install_if_missing(
        &mut self,
        package: Arc<Package>,
    ) -> Result<miden_project::Version, InstallPackageError> {
        self.install_if_missing_as(package, None)
    }

    /// Installs `package`, optionally overriding the filename used in the filesystem exchange.
    ///
    /// Compiler-built packages use the historical name-only path. Registry packages supply a
    /// version-qualified name because several versions can be resident and eagerly published at
    /// once.
    fn install_if_missing_as(
        &mut self,
        package: Arc<Package>,
        published_file_name: Option<&str>,
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

        // Publish into the filesystem cache before mutating the in-memory registry, so a
        // failed write leaves both untouched and the two can never disagree about what is
        // installed. The incumbent's cached file is protected by the digest conflict check
        // above, which returns before reaching here.
        #[cfg(any(test, feature = "std"))]
        if let Some(filesystem_cache) = self.filesystem_cache.as_deref() {
            write_package_atomically_as(&package, filesystem_cache, published_file_name).map_err(
                |err| InstallPackageError::FilesystemCacheInsertion {
                    package: package.name.clone(),
                    err,
                },
            )?;
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

/// Publishes `package` as `<out_dir>/<package name>.masp`, atomically, and returns that path.
///
/// The package is serialized to a temporary file in the same directory and then renamed over
/// the final path. Compiled packages are read concurrently by other build processes — e.g. the
/// `#[account(..)]` proc macro of a dependent crate deserializes a dependency's `.masp` while a
/// parallel build of that dependency may be rewriting it — and the rename guarantees a reader
/// only ever observes a complete artifact.
#[cfg(any(test, feature = "std"))]
pub fn write_package_atomically(
    package: &Package,
    out_dir: &std::path::Path,
) -> std::io::Result<std::path::PathBuf> {
    write_package_atomically_as(package, out_dir, None)
}

/// Publishes `package` atomically, using `file_name` when one is supplied.
#[cfg(any(test, feature = "std"))]
fn write_package_atomically_as(
    package: &Package,
    out_dir: &std::path::Path,
    file_name: Option<&str>,
) -> std::io::Result<std::path::PathBuf> {
    std::fs::create_dir_all(out_dir)?;
    let destination = match file_name {
        Some(file_name) => out_dir.join(file_name),
        None => out_dir
            .join(midenc_frontend_wasm_metadata::package_cache::package_file_name(&package.name)),
    };
    persist_atomically(&destination, |temp_path| package.write_to_file(temp_path))?;
    Ok(destination)
}

/// Writes `bytes` to `path` through a temporary sibling and an atomic rename.
///
/// The byte-oriented door to the same publication mechanics as
/// [`write_package_atomically`], for the other files the compiler places into the shared
/// cache directory — the recorded dependency resolution, whose readers are the same
/// population as the packages'.
#[cfg(any(test, feature = "std"))]
pub fn write_file_atomically(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    persist_atomically(path, |temp_path| std::fs::write(temp_path, bytes))
}

/// Writes through a temporary sibling of `path` and renames it over the final name.
///
/// Temporary files default to mode 0o600, and the published file must stay readable by the
/// other build processes that share the cache directory, so the mode is widened to 0o666
/// (the process umask still applies).
#[cfg(any(test, feature = "std"))]
fn persist_atomically(
    path: &std::path::Path,
    write: impl FnOnce(&std::path::Path) -> std::io::Result<()>,
) -> std::io::Result<()> {
    let directory = path.parent().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("path '{}' has no parent directory", path.display()),
        )
    })?;
    let mut builder = tempfile::Builder::new();
    builder.prefix(".").suffix(".tmp");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        builder.permissions(std::fs::Permissions::from_mode(0o666));
    }
    let temp_path = builder.tempfile_in(directory)?.into_temp_path();
    write(&temp_path)?;
    temp_path.persist(path).map_err(|err| err.error)?;
    Ok(())
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

    /// Returns a copy of `package` renamed to the name and version of `like`.
    ///
    /// The copy keeps the content digest of `package`, so it collides with `like` on
    /// name and version while carrying a different digest.
    fn renamed(package: &Package, like: &Package) -> Arc<Package> {
        let mut copy = package.clone();
        copy.name = like.name.clone();
        copy.version = like.version.clone();
        Arc::new(copy)
    }

    fn options(sysroot: Option<std::path::PathBuf>) -> crate::Options {
        let dir = std::env::temp_dir();
        crate::Options::new(None, None, dir.clone(), dir, None, sysroot)
    }

    /// A same-version install with a different digest must be rejected without
    /// overwriting the incumbent's artifact, in memory or in the filesystem cache.
    #[test]
    fn rejected_install_preserves_the_cached_artifact() {
        let core_library = miden_core_lib::CoreLibrary::default();
        let incumbent = core_library.precompiles_package();
        let intruder = renamed(&core_library.package(), &incumbent);
        assert_ne!(incumbent.digest(), intruder.digest());

        let cache_dir = tempfile::tempdir().unwrap();
        let mut registry = HybridPackageRegistry::empty();
        registry.filesystem_cache = Some(cache_dir.path().to_path_buf());
        registry.install_if_missing(Arc::clone(&incumbent)).unwrap();

        let cached = cache_dir.path().join(format!("{}.masp", &incumbent.name));
        let before = std::fs::read(&cached).unwrap();

        let err = registry.install_if_missing(intruder).unwrap_err();
        assert!(matches!(err, InstallPackageError::AlreadyInstalledWithDifferentDigest { .. }));

        let after = std::fs::read(&cached).unwrap();
        assert_eq!(before, after, "a rejected install must not overwrite the cached artifact");

        let version = miden_project::Version::new(incumbent.version.clone(), incumbent.digest());
        let loaded = registry.load_package(&incumbent.name, &version).unwrap();
        assert_eq!(loaded.digest(), incumbent.digest());
    }

    /// A fresh registry must contain a precompiles artifact that satisfies the exact-digest
    /// dependency recorded by the installed core package.
    #[test]
    fn seeding_provides_the_precompiles_artifact_the_core_package_requires() {
        let registry = HybridPackageRegistry::new(&options(None)).unwrap();

        let core_library = miden_core_lib::CoreLibrary::default();
        let core = core_library.package();
        let precompiles_name = core_library.precompiles_package().name.clone();
        let dep = core
            .manifest
            .dependencies()
            .find(|dep| dep.name == precompiles_name)
            .expect("core package should depend on the precompiles package");

        let version = miden_project::Version::new(dep.version.clone(), dep.digest);
        let loaded = registry.load_package(&dep.name, &version).unwrap();
        assert_eq!(loaded.digest(), dep.digest);
    }

    /// A failed filesystem-cache write must surface as an error and must not leave a partial
    /// or temporary file behind.
    #[cfg(unix)]
    #[test]
    fn failed_cache_write_leaves_no_partial_file() {
        use std::os::unix::fs::PermissionsExt;

        let package = miden_core_lib::CoreLibrary::default().precompiles_package();

        let cache_dir = tempfile::tempdir().unwrap();
        let mut permissions = std::fs::metadata(cache_dir.path()).unwrap().permissions();
        permissions.set_mode(0o555);
        std::fs::set_permissions(cache_dir.path(), permissions).unwrap();

        // Root ignores the mode bits; when the read-only precondition cannot be expressed,
        // there is nothing to test.
        let probe = cache_dir.path().join("probe");
        if std::fs::write(&probe, b"").is_ok() {
            std::fs::remove_file(&probe).unwrap();
            return;
        }

        let mut registry = HybridPackageRegistry::empty();
        registry.filesystem_cache = Some(cache_dir.path().to_path_buf());
        let err = registry.install_if_missing(package).unwrap_err();
        assert!(matches!(err, InstallPackageError::FilesystemCacheInsertion { .. }));

        let leftovers = std::fs::read_dir(cache_dir.path()).unwrap().count();
        assert_eq!(leftovers, 0, "a failed cache write must not leave files behind");
    }

    /// When the local registry provides a same-version precompiles package with a different
    /// digest, seeding must keep the local copy and initialization must still succeed.
    #[test]
    fn seeding_keeps_a_mismatched_local_registry_copy() {
        let core_library = miden_core_lib::CoreLibrary::default();
        let bundled_precompiles = core_library.precompiles_package();
        let doctored = renamed(&core_library.package(), &bundled_precompiles);

        let sysroot = tempfile::tempdir().unwrap();
        let lib_dir = sysroot.path().join("lib");
        std::fs::create_dir_all(&lib_dir).unwrap();
        doctored.write_masp_file(&lib_dir).unwrap();

        let registry =
            HybridPackageRegistry::new(&options(Some(sysroot.path().to_path_buf()))).unwrap();

        let version = miden_project::Version::new(doctored.version.clone(), doctored.digest());
        let loaded = registry.load_package(&doctored.name, &version).unwrap();
        assert_eq!(
            loaded.digest(),
            doctored.digest(),
            "the local registry copy must survive the bundled seeding"
        );
    }

    #[test]
    fn install_checks_conflicts_before_writing_and_rewrites_accepted_packages() {
        let temp = TempDir::new().unwrap();
        let cache = temp.path().join("cache");
        std::fs::create_dir_all(&cache).unwrap();

        let options = crate::Options::default();
        let package = crate::LinkLibrary::core().load(&options).unwrap();
        let package_name: &str = &package.name;
        let cached_package = cache
            .join(midenc_frontend_wasm_metadata::package_cache::package_file_name(package_name));
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
                .ends_with(".tmp")),
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
                .ends_with(".tmp")),
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

    #[test]
    fn constructor_publishes_embedded_registry_packages_under_versioned_names() {
        let temp = TempDir::new().unwrap();
        let cache = temp.path().join("cache");
        let options = crate::Options::default();
        let core = crate::LinkLibrary::core().load(&options).unwrap();

        HybridPackageRegistry::new_with_filesystem_cache(&options, Some(cache.clone())).unwrap();

        let published =
            cache.join(midenc_frontend_wasm_metadata::package_cache::registry_package_file_name(
                &core.name,
                &core.version,
            ));
        assert!(
            published.is_file(),
            "an embedded registry dependency must use the path recorded in dependency maps"
        );
    }

    #[test]
    fn constructor_publishes_preloaded_registry_packages_into_the_cache() {
        let temp = TempDir::new().unwrap();
        let sysroot = temp.path().join("sysroot");
        let lib_dir = sysroot.join("lib");
        std::fs::create_dir_all(&lib_dir).unwrap();

        let mut registry_package =
            (*crate::LinkLibrary::core().load(&crate::Options::default()).unwrap()).clone();
        registry_package.name = "registry-component".into();
        let mut newer_registry_package = registry_package.clone();
        newer_registry_package.version.major += 1;
        registry_package
            .write_to_file(lib_dir.join("registry-component-v1.masp"))
            .unwrap();
        newer_registry_package
            .write_to_file(lib_dir.join("registry-component-v2.masp"))
            .unwrap();

        let cache = temp.path().join("cache");
        let options = crate::Options {
            sysroot: Some(sysroot),
            ..crate::Options::default()
        };
        let registry =
            HybridPackageRegistry::new_with_filesystem_cache(&options, Some(cache.clone()))
                .unwrap();

        let published =
            cache.join(midenc_frontend_wasm_metadata::package_cache::registry_package_file_name(
                &registry_package.name,
                &registry_package.version,
            ));
        let newer_published =
            cache.join(midenc_frontend_wasm_metadata::package_cache::registry_package_file_name(
                &newer_registry_package.name,
                &newer_registry_package.version,
            ));
        assert!(published.is_file(), "a registry package must be present in the exchange");
        assert!(
            newer_published.is_file(),
            "each registry version must have a distinct exchange path"
        );
        let reloaded = crate::libs::load_package_from_path(&published).unwrap();
        let newer_reloaded = crate::libs::load_package_from_path(&newer_published).unwrap();
        assert_eq!(reloaded.name, registry_package.name);
        assert_eq!(reloaded.digest(), registry_package.digest());
        assert_eq!(newer_reloaded.version, newer_registry_package.version);
        assert_eq!(newer_reloaded.digest(), newer_registry_package.digest());
        assert!(
            registry
                .artifacts
                .get(&registry_package.name)
                .is_some_and(|versions| versions.contains_key(&registry_package.version)),
            "the published package must remain available through the in-memory registry"
        );
    }
}
