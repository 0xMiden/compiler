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

        registry.seed_core_precompiles()?;

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

    /// Seeds the registry with the `miden-precompiles` artifact that the installed core package
    /// requires.
    ///
    /// The core package records its `miden-precompiles` dependency with an exact digest, so an
    /// artifact with the right name and version but a different digest can never satisfy it.
    /// This inspects the core package that actually won installation (the bundled one, or a
    /// same-version copy from the local registry), and installs the bundled precompiles package
    /// only when it is the artifact that dependency resolution will look for. When neither the
    /// local registry nor the bundled copy can satisfy the dependency, a warning names the
    /// mismatch instead of leaving it to fail later, far from the cause.
    #[cfg(any(test, feature = "std"))]
    fn seed_core_precompiles(&mut self) -> Result<(), Report> {
        use alloc::string::ToString;

        let core_library = miden_core_lib::CoreLibrary::default();
        let bundled_core = core_library.package();
        let bundled_precompiles = core_library.precompiles_package();

        let installed_core = self
            .artifacts
            .get(&bundled_core.name)
            .and_then(|versions| versions.get(&bundled_core.version));
        let Some(required) = installed_core.and_then(|core| {
            core.manifest
                .dependencies()
                .find(|dep| dep.name == bundled_precompiles.name)
                .map(|dep| (dep.version.clone(), dep.digest))
        }) else {
            // No installed core package, or one without a precompiles dependency: nothing to
            // seed.
            return Ok(());
        };
        let (required_version, required_digest) = required;

        let satisfied = self
            .artifacts
            .get(&bundled_precompiles.name)
            .and_then(|versions| versions.get(&required_version))
            .is_some_and(|artifact| artifact.digest() == required_digest);
        if satisfied {
            return Ok(());
        }

        if bundled_precompiles.version == required_version
            && bundled_precompiles.digest() == required_digest
        {
            match self.install_if_missing(bundled_precompiles) {
                Ok(_) => (),
                Err(InstallPackageError::AlreadyInstalledWithDifferentDigest {
                    package,
                    version,
                }) => {
                    log::warn!(
                        target: "package-registry",
                        "the local registry provides {package}@{version} with a digest that does \
                         not satisfy the installed core package's dependency; core-library code \
                         may fail to resolve it"
                    );
                }
                Err(err) => return Err(Report::msg(err.to_string())),
            }
        } else {
            log::warn!(
                target: "package-registry",
                "the installed {} package requires {}@{} with digest {}, which neither the local \
                 registry nor the bundled core library provides",
                &bundled_core.name,
                &bundled_precompiles.name,
                required_version,
                required_digest,
            );
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
            package.write_masp_file(filesystem_cache).map_err(|err| {
                InstallPackageError::FilesystemCacheInsertion {
                    package: package.name.clone(),
                    err,
                }
            })?;
        }

        Ok(version)
    }
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
