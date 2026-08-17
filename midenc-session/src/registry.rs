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

        registry.seed_bundled_dependencies()?;

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

    /// Seeds the registry with bundled packages that installed packages depend on.
    ///
    /// Package manifests record their dependencies with exact digests, so an artifact with the
    /// right name and version but a different digest can never satisfy one. This walks the
    /// manifest dependencies of every installed package, including the packages it installs
    /// along the way, and installs the bundled package that matches a missing dependency
    /// exactly. When neither the local registry nor a bundled package can satisfy a dependency,
    /// a warning names the mismatch instead of leaving it to fail later, far from the cause.
    #[cfg(any(test, feature = "std"))]
    fn seed_bundled_dependencies(&mut self) -> Result<(), Report> {
        use alloc::{string::ToString, vec::Vec};

        // The bundled packages that seeding can provide.
        let bundled = miden_core_lib::CoreLibrary::default().packages();

        let mut worklist: Vec<Arc<Package>> = self
            .artifacts
            .values()
            .flat_map(|versions| versions.values())
            .cloned()
            .collect();
        while let Some(package) = worklist.pop() {
            for dep in package.manifest.dependencies() {
                let required = miden_project::Version::new(dep.version.clone(), dep.digest);
                if self.load_package(&dep.name, &required).is_ok() {
                    continue;
                }

                // A same-version artifact that load_package rejected differs by digest. The
                // local copy stays installed; the warning names both digests.
                let incumbent = self
                    .artifacts
                    .get(&dep.name)
                    .and_then(|versions| versions.get(&dep.version))
                    .map(|artifact| artifact.digest());
                if let Some(incumbent_digest) = incumbent {
                    log::warn!(
                        target: "package-registry",
                        "the local registry provides {}@{} with digest {incumbent_digest}, which \
                         does not satisfy the digest {} that {}@{} requires; dependent code may \
                         fail to resolve it",
                        &dep.name,
                        &dep.version,
                        &dep.digest,
                        &package.name,
                        &package.version,
                    );
                    continue;
                }

                let provider = bundled.iter().find(|provider| {
                    provider.name == dep.name
                        && provider.version == dep.version
                        && provider.digest() == dep.digest
                });
                let Some(provider) = provider else {
                    log::warn!(
                        target: "package-registry",
                        "the installed {}@{} package requires {}@{} with digest {}, which \
                         neither the local registry nor the bundled packages provides",
                        &package.name,
                        &package.version,
                        &dep.name,
                        &dep.version,
                        &dep.digest,
                    );
                    continue;
                };

                match self.install_if_missing(Arc::clone(provider)) {
                    // The dependencies of the new package need the same check.
                    Ok(_) => worklist.push(Arc::clone(provider)),
                    Err(err) => return Err(Report::msg(err.to_string())),
                }
            }
        }

        Ok(())
    }

    fn install_if_missing(
        &mut self,
        package: Arc<Package>,
    ) -> Result<miden_project::Version, InstallPackageError> {
        use alloc::collections::btree_map::Entry as BTreeMapEntry;

        use hashbrown::hash_map::Entry;

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
            .insert(version.clone(), Arc::clone(&package));

        // Mirror the artifact into the filesystem cache only after the in-memory install
        // succeeded: a same-version package with a different digest must not overwrite the
        // incumbent's cached file.
        #[cfg(any(test, feature = "std"))]
        if let Some(filesystem_cache) = self.filesystem_cache.as_deref() {
            write_masp_file_atomically(&package, filesystem_cache).map_err(|err| {
                InstallPackageError::FilesystemCacheInsertion {
                    package: package.name.clone(),
                    err,
                }
            })?;
        }

        Ok(version)
    }
}

/// Writes `package` to `dir` as `<name>.masp` through a temporary file and an atomic rename.
///
/// A direct write that fails part-way (full disk, crash) leaves a truncated file that poisons
/// every later build that reads the cache directory. The temporary file name carries the
/// process id, so concurrent compiler processes that share a cache directory do not corrupt
/// each other's writes.
#[cfg(any(test, feature = "std"))]
fn write_masp_file_atomically(package: &Package, dir: &std::path::Path) -> std::io::Result<()> {
    use miden_core::serde::Serializable;

    std::fs::create_dir_all(dir)?;
    let package_name: &str = &package.name;
    let final_path = dir.join(package_name).with_extension(Package::EXTENSION);
    let temp_path =
        final_path.with_extension(format!("{}.{}.tmp", Package::EXTENSION, std::process::id()));
    std::fs::write(&temp_path, package.to_bytes())?;
    std::fs::rename(&temp_path, &final_path).inspect_err(|_| {
        let _ = std::fs::remove_file(&temp_path);
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

    /// Seeding must walk every installed package, not only the bundled core version: a core
    /// package at another version must still get its precompiles dependency installed.
    #[test]
    fn seeding_covers_every_installed_version_of_a_package() {
        let core_library = miden_core_lib::CoreLibrary::default();
        let mut other_core = core_library.package().as_ref().clone();
        other_core.version.major += 1;

        let mut registry = HybridPackageRegistry::empty();
        registry.install_if_missing(Arc::new(other_core.clone())).unwrap();
        registry.seed_bundled_dependencies().unwrap();

        let precompiles_name = core_library.precompiles_package().name.clone();
        let dep = other_core
            .manifest
            .dependencies()
            .find(|dep| dep.name == precompiles_name)
            .expect("core package should depend on the precompiles package");
        let version = miden_project::Version::new(dep.version.clone(), dep.digest);
        let loaded = registry
            .load_package(&dep.name, &version)
            .expect("seeding must satisfy the dependency of a non-bundled core version");
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
}
