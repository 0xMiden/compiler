use alloc::{collections::BTreeMap, format, sync::Arc};

use anyhow::anyhow;
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
        err: anyhow::Error,
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

        // Load the explicitly requested link libraries. Every Miden library, including the
        // core and protocol libraries, now comes from the sysroot above (or from an explicit
        // `-l`/`-L`); nothing is force-loaded here.
        for lib in options.link_libraries.iter() {
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
        let entries = match lib_dir.read_dir() {
            Ok(entries) => entries,
            // A sysroot with no `lib/` is an empty local registry, not a failure. `Options`
            // derives a sysroot from `MIDENUP_HOME`/`MIDENUP_TOOLCHAIN` whether or not a
            // toolchain is installed there, so this is the ordinary first-run path; failing
            // here would pre-empt `LinkLibrary::find`'s curated error, which names the paths
            // tried and points at midenup.
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(err) => {
                return Err(Report::msg(format!(
                    "cannot read from sysroot ({}): {err}",
                    lib_dir.display()
                )));
            }
        };

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
        // The commitment is hashed on every call, so it is computed once for both uses.
        let dependency_commitment = package.dependency_commitment();
        let version = miden_project::Version::new(package.version.clone(), dependency_commitment);
        log::trace!(target: "package-registry", "preparing to install package {}@{version}", package.name);
        if let Some(previous_digest) = self
            .packages
            .get(&package.name)
            .and_then(|versions| versions.get(&package.version))
            .and_then(PackageRecord::digest)
            .copied()
            && previous_digest != dependency_commitment
        {
            log::trace!(target: "package-registry", "package already installed: {}@{version}", package.name);
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

        log::trace!(target: "package-registry", "installed {}@{version}", package.name);

        self.artifacts
            .entry(package.name.clone())
            .or_default()
            .insert(version.clone(), package);

        Ok(version)
    }

    /// Every package this registry holds, in name order then version order.
    ///
    /// `artifacts` is keyed by an `FxHashMap`, whose iteration order is unspecified and can
    /// change between runs; sorting the package names first keeps this deterministic, which
    /// matters because [`ExportResolver::resolve_procedure`](midenc_package_interface::ExportResolver::resolve_procedure)'s
    /// "first match wins" must not depend on hash order.
    pub fn packages(&self) -> impl Iterator<Item = &Arc<Package>> {
        let mut names: alloc::vec::Vec<&PackageId> = self.artifacts.keys().collect();
        names.sort();
        names.into_iter().flat_map(move |name| {
            self.artifacts.get(name).into_iter().flat_map(|versions| versions.values())
        })
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
) -> anyhow::Result<std::path::PathBuf> {
    write_package_atomically_as(package, out_dir, None)
}

/// Publishes `package` atomically, using `file_name` when one is supplied.
#[cfg(any(test, feature = "std"))]
pub fn write_package_atomically_as(
    package: &Package,
    out_dir: &std::path::Path,
    file_name: Option<&str>,
) -> anyhow::Result<std::path::PathBuf> {
    let destination = match file_name {
        Some(file_name) => out_dir.join(file_name),
        None => out_dir
            .join(midenc_frontend_wasm_metadata::package_cache::package_file_name(&package.name)),
    };
    persist_atomically(&destination, |temp_path| {
        package
            .write_to_file(temp_path)
            .map_err(|err| anyhow!("failed to write package to file: {err}"))
    })?;
    Ok(destination)
}

/// Writes `bytes` to `path` through a temporary sibling and an atomic rename.
///
/// The byte-oriented door to the same publication mechanics as
/// [`write_package_atomically`], for the other files the compiler places into the shared
/// cache directory — the recorded dependency resolution, whose readers are the same
/// population as the packages'.
#[cfg(any(test, feature = "std"))]
pub fn write_file_atomically(path: &std::path::Path, bytes: &[u8]) -> anyhow::Result<()> {
    persist_atomically(path, |temp_path| {
        std::fs::write(temp_path, bytes).map_err(|err| anyhow!("failed to write to file: {err}"))
    })
}

/// Writes through a temporary sibling of `path` and renames it over the final name.
///
/// Temporary files default to mode 0o600, and the published file must stay readable by the
/// other build processes that share the cache directory, so the mode is widened to 0o666
/// (the process umask still applies).
#[cfg(any(test, feature = "std"))]
pub fn persist_atomically(
    path: &std::path::Path,
    write: impl FnOnce(&std::path::Path) -> anyhow::Result<()>,
) -> anyhow::Result<()> {
    use anyhow::Context;

    let directory = path
        .parent()
        .ok_or_else(|| anyhow!("path '{}' has no parent directory", path.display()))?;
    std::fs::create_dir_all(directory)
        .with_context(|| format!("failed to create directory '{}'", directory.display()))?;
    let mut builder = tempfile::Builder::new();
    builder.prefix(".").suffix(".tmp");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        builder.permissions(std::fs::Permissions::from_mode(0o666));
    }
    let temp_path = builder
        .tempfile_in(directory)
        .context("failed to create temp file")?
        .into_temp_path();
    write(&temp_path)?;
    temp_path
        .persist(path)
        .map_err(|err| err.error)
        .context("failed to persist temp file")?;
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
        // Artifacts are stored under a version that carries their dependency commitment, so the
        // stored digest is compared instead of hashing the package again on every lookup.
        let found = self
            .artifacts
            .get(package)
            .and_then(|versions| versions.get_key_value(&version.version));
        match found {
            Some((installed, _)) if version.digest != installed.digest => {
                Err(Report::msg(format!(
                    "cannot load {package}@{version}: a specific digest was requested, but \
                     differs from the available version"
                )))
            }
            Some((_, artifact)) => Ok(Arc::clone(artifact)),
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
pub(crate) mod tests {
    use alloc::boxed::Box;

    use tempfile::TempDir;

    use super::*;

    /// A minimal library package named `name`, assembled with the real assembler.
    ///
    /// The body's pushed immediate is derived from `name`'s bytes so that fixtures with
    /// different names always assemble to different MAST digests. A `nop` is elided by the
    /// assembler entirely (any number of them assembles identically), so it cannot be used for
    /// this; `push`/`drop` is a true no-op on the stack that still varies the assembled opcodes.
    /// Digests hash only the assembled opcodes, never the package or module name, so two
    /// fixtures with an identical body would otherwise collide and defeat every test that checks
    /// digest-conflict handling.
    pub(crate) fn fixture_package(name: &str) -> Arc<Package> {
        use miden_assembly_syntax::{
            ModuleParser,
            debuginfo::{DefaultSourceManager, SourceLanguage, SourceManager, Uri},
        };
        let source_manager: Arc<dyn SourceManager> = Arc::new(DefaultSourceManager::default());
        let root = name.replace('-', "_");
        let uri = Uri::from(root.clone().into_boxed_str());
        // Kept well under the field modulus (~2^64 - 2^32 + 1); the value itself is never read.
        let immediate = name
            .bytes()
            .fold(1u64, |acc, b| acc.wrapping_mul(31).wrapping_add(u64::from(b)))
            % 1_000_003
            + 1;
        let source = source_manager.load(
            SourceLanguage::Masm,
            uri,
            format!("pub proc id(x: felt) -> felt\n    push.{immediate}\n    drop\nend\n"),
        );
        let module = ModuleParser::new(None)
            .parse(
                Some(miden_assembly_syntax::ast::Path::new(&root)),
                source,
                source_manager.clone(),
            )
            .unwrap();
        let package = miden_assembly::Assembler::new(source_manager)
            .assemble_library(
                name,
                module,
                core::iter::empty::<Box<miden_assembly_syntax::ast::Module>>(),
            )
            .unwrap();
        Arc::from(package)
    }

    /// A second fixture whose manifest declares a dependency on `dep`, for the tests that need
    /// a package that requires another.
    pub(crate) fn fixture_package_depending_on(name: &str, dep: &Package) -> Arc<Package> {
        let mut package = (*fixture_package(name)).clone();
        package.manifest.add_dependency(dep.to_dependency()).unwrap();
        Arc::new(package)
    }

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
        let incumbent = fixture_package("alpha");
        let intruder = renamed(&fixture_package("beta"), &incumbent);
        assert_ne!(incumbent.dependency_commitment(), intruder.dependency_commitment());

        let cache_dir = tempfile::tempdir().unwrap();
        let mut registry = HybridPackageRegistry::empty();
        registry.filesystem_cache = Some(cache_dir.path().to_path_buf());
        registry.install_if_missing(Arc::clone(&incumbent)).unwrap();

        let cached = cache_dir.path().join(format!("{}.masp", incumbent.name));
        let before = std::fs::read(&cached).unwrap();

        let err = registry.install_if_missing(intruder).unwrap_err();
        assert!(matches!(err, InstallPackageError::AlreadyInstalledWithDifferentDigest { .. }));

        let after = std::fs::read(&cached).unwrap();
        assert_eq!(before, after, "a rejected install must not overwrite the cached artifact");

        let version = miden_project::Version::new(
            incumbent.version.clone(),
            incumbent.dependency_commitment(),
        );
        let loaded = registry.load_package(&incumbent.name, &version).unwrap();
        assert_eq!(loaded.dependency_commitment(), incumbent.dependency_commitment());
    }

    /// A fresh registry seeded from the sysroot must contain a dependency artifact that
    /// satisfies the exact-digest dependency recorded by the package requiring it.
    #[test]
    fn seeding_from_the_sysroot_provides_a_dependency_the_root_package_requires() {
        let sysroot = tempfile::tempdir().unwrap();
        let lib_dir = sysroot.path().join("lib");
        std::fs::create_dir_all(&lib_dir).unwrap();

        let dep = fixture_package("dep");
        let root = fixture_package_depending_on("root", &dep);
        dep.write_masp_file(&lib_dir).unwrap();
        root.write_masp_file(&lib_dir).unwrap();

        let registry =
            HybridPackageRegistry::new(&options(Some(sysroot.path().to_path_buf()))).unwrap();

        let recorded = root
            .manifest
            .dependencies()
            .find(|recorded| recorded.name == dep.name)
            .expect("root package should depend on the dep package");

        let version = miden_project::Version::new(recorded.version.clone(), recorded.digest);
        let loaded = registry.load_package(&recorded.name, &version).unwrap();
        assert_eq!(loaded.dependency_commitment(), recorded.digest);
    }

    /// A failed filesystem-cache write must surface as an error and must not leave a partial
    /// or temporary file behind.
    #[cfg(unix)]
    #[test]
    fn failed_cache_write_leaves_no_partial_file() {
        use std::os::unix::fs::PermissionsExt;

        let package = fixture_package("alpha");

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

    /// When the local registry on disk provides a same-version package with a different digest
    /// than one already installed, seeding from it must keep the already-installed package and
    /// must not fail the whole load.
    ///
    /// The registry no longer bundles anything on construction, so the "already installed"
    /// package that used to arrive automatically is installed explicitly here first.
    #[test]
    fn seeding_keeps_a_mismatched_local_registry_copy() {
        let bundled = fixture_package("alpha");
        let doctored = renamed(&fixture_package("beta"), &bundled);

        let sysroot = tempfile::tempdir().unwrap();
        let lib_dir = sysroot.path().join("lib");
        std::fs::create_dir_all(&lib_dir).unwrap();
        doctored.write_masp_file(&lib_dir).unwrap();

        let mut registry = HybridPackageRegistry::empty();
        registry.install_if_missing(Arc::clone(&bundled)).unwrap();
        registry
            .load_local_registry(&options(Some(sysroot.path().to_path_buf())))
            .unwrap();

        let version =
            miden_project::Version::new(bundled.version.clone(), bundled.dependency_commitment());
        let loaded = registry.load_package(&bundled.name, &version).unwrap();
        assert_eq!(
            loaded.dependency_commitment(),
            bundled.dependency_commitment(),
            "an already-installed package must survive a conflicting local registry copy"
        );
    }

    #[test]
    fn install_checks_conflicts_before_writing_and_rewrites_accepted_packages() {
        let temp = TempDir::new().unwrap();
        let cache = temp.path().join("cache");
        std::fs::create_dir_all(&cache).unwrap();

        let sysroot = temp.path().join("sysroot");
        let lib_dir = sysroot.join("lib");
        std::fs::create_dir_all(&lib_dir).unwrap();
        fixture_package("miden-core").write_masp_file(&lib_dir).unwrap();
        fixture_package("miden-tx-kernel").write_masp_file(&lib_dir).unwrap();

        let options = options(Some(sysroot));
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
        assert_ne!(conflict.dependency_commitment(), package.dependency_commitment());
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

    /// A sysroot with no `lib/` directory is an empty local registry, not a failure.
    ///
    /// `Options` derives a sysroot from `MIDENUP_HOME`/`MIDENUP_TOOLCHAIN` whether or not a
    /// toolchain is installed there, so this is the ordinary first-run path. Construction has to
    /// succeed for the error the user finally sees to be `LinkLibrary::find`'s, which names the
    /// paths tried and points at midenup.
    #[test]
    fn a_sysroot_without_a_lib_directory_is_an_empty_registry() {
        let temp = TempDir::new().unwrap();
        let options = options(Some(temp.path().to_path_buf()));

        let registry = HybridPackageRegistry::new_with_filesystem_cache(&options, None).unwrap();
        assert!(
            registry.packages().next().is_none(),
            "nothing is installed from an empty sysroot"
        );

        let err = alloc::format!("{}", crate::LinkLibrary::core().load(&options).unwrap_err());
        assert!(err.contains("midenup"), "{err}");
        assert!(!err.contains("cannot read from sysroot"), "{err}");
    }

    #[test]
    fn constructor_publishes_sysroot_packages_under_versioned_names() {
        let temp = TempDir::new().unwrap();
        let cache = temp.path().join("cache");
        let sysroot = temp.path().join("sysroot");
        std::fs::create_dir_all(sysroot.join("lib")).unwrap();
        fixture_package("miden-core").write_masp_file(sysroot.join("lib")).unwrap();

        let options = options(Some(sysroot));
        let core = crate::LinkLibrary::core().load(&options).unwrap();

        HybridPackageRegistry::new_with_filesystem_cache(&options, Some(cache.clone())).unwrap();

        let published =
            cache.join(midenc_frontend_wasm_metadata::package_cache::registry_package_file_name(
                &core.name,
                &core.version,
            ));
        assert!(
            published.is_file(),
            "a sysroot-resolved dependency must use the path recorded in dependency maps"
        );
    }

    #[test]
    fn constructor_publishes_preloaded_registry_packages_into_the_cache() {
        let temp = TempDir::new().unwrap();

        // A separate sysroot, used only to source a template package via the alias-resolving
        // load path; it is not the sysroot under test below.
        let source_sysroot = temp.path().join("source-sysroot");
        std::fs::create_dir_all(source_sysroot.join("lib")).unwrap();
        fixture_package("miden-core")
            .write_masp_file(source_sysroot.join("lib"))
            .unwrap();
        let mut registry_package =
            (*crate::LinkLibrary::core().load(&options(Some(source_sysroot))).unwrap()).clone();
        registry_package.name = "registry-component".into();
        let mut newer_registry_package = registry_package.clone();
        newer_registry_package.version.major += 1;

        let sysroot = temp.path().join("sysroot");
        let lib_dir = sysroot.join("lib");
        std::fs::create_dir_all(&lib_dir).unwrap();
        registry_package
            .write_to_file(lib_dir.join("registry-component-v1.masp"))
            .unwrap();
        newer_registry_package
            .write_to_file(lib_dir.join("registry-component-v2.masp"))
            .unwrap();

        let cache = temp.path().join("cache");
        let opts = options(Some(sysroot));
        let registry =
            HybridPackageRegistry::new_with_filesystem_cache(&opts, Some(cache.clone())).unwrap();

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
        assert_eq!(reloaded.dependency_commitment(), registry_package.dependency_commitment());
        assert_eq!(newer_reloaded.version, newer_registry_package.version);
        assert_eq!(
            newer_reloaded.dependency_commitment(),
            newer_registry_package.dependency_commitment()
        );
        assert!(
            registry
                .artifacts
                .get(&registry_package.name)
                .is_some_and(|versions| versions.contains_key(&registry_package.version)),
            "the published package must remain available through the in-memory registry"
        );
    }
}
