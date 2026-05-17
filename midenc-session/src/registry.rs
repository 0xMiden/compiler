use alloc::{collections::BTreeMap, format, sync::Arc};

#[cfg(feature = "std")]
use miden_assembly_syntax::Report;
use miden_assembly_syntax::diagnostics::{Diagnostic, miette};
use miden_core::LexicographicWord;
use miden_mast_package::Package;
use miden_package_registry::{
    PackageId, PackageIndex, PackageProvider, PackageRecord, PackageRegistry, PackageStore,
    PackageVersions,
};
use miden_project::VersionRequirement;

type FxHashMap<K, V> = hashbrown::HashMap<K, V, rustc_hash::FxBuildHasher>;

#[derive(Debug, thiserror::Error, Diagnostic)]
enum InstallPackageError {
    #[error("package {package}@{version} is already registered under a different digest")]
    AlreadyInstalledWithDifferentDigest {
        package: PackageId,
        version: miden_project::Version,
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
}

impl HybridPackageRegistry {
    /// Get an empty, uninitialized registry
    pub fn empty() -> Self {
        Self {
            packages: Default::default(),
            artifacts: Default::default(),
        }
    }

    /// Get a new instance of the registry, using the current compiler options
    #[cfg(any(test, feature = "std"))]
    pub fn new(options: &crate::Options) -> Result<Self, Report> {
        // Load system libraries
        let mut registry = if options.sysroot.is_some() {
            Self::from_local_registry(options)?
        } else {
            Self::empty()
        };

        // Load link libraries
        let core = crate::LinkLibrary::core();
        let protocol = crate::LinkLibrary::protocol();
        let implied_libraries = vec![&core, &protocol];
        let link_libraries = options.link_libraries.iter().chain(implied_libraries);
        for lib in link_libraries {
            let package = lib.load(options)?;
            match registry.install_if_missing(package) {
                Ok(_) => (),
                // Ignore duplicates when initializing the registry
                Err(InstallPackageError::AlreadyInstalledWithDifferentDigest { .. }) => (),
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
                        let prev_digest = prev.get().digest().copied().map(LexicographicWord::new);
                        let digest = LexicographicWord::new(package.digest());
                        if prev_digest.is_none_or(|prev_digest| prev_digest == digest) {
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
            Some(artifact) if version.digest != Some(LexicographicWord::new(artifact.digest())) => {
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

impl PackageStore for HybridPackageRegistry {
    type Error = Report;

    fn publish_package(
        &mut self,
        package: Arc<Package>,
    ) -> Result<miden_project::Version, Self::Error> {
        self.install_if_missing(package).map_err(Report::from)
    }
}
