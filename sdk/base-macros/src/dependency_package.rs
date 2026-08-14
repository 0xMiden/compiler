//! Locating and reading compiled Miden dependency packages (`.masp`) at macro-expansion time.
//!
//! A Miden path dependency is consumed through its compiled package: the `.masp` carries both the
//! dependency's embedded component WIT (read here) and its procedure roots (read by [`crate::fpi`]).
//!
//! Every dependency package comes from the build-owned package cache named by
//! `MIDENC_PACKAGE_CACHE`. A midenc-driven build compiles the dependencies, publishes them into
//! its per-build cache, and exports the variable to its nested cargo builds; the contract
//! `build.rs` does the same for plain `cargo build`/`cargo check` and IDE analysis through a
//! stable exported directory. The macros
//! never search the filesystem for packages themselves — the one exception is a dependency whose
//! manifest path names a `.masp` file directly, which is read from that explicit location.

use std::{
    env, fs,
    path::{Path, PathBuf},
    sync::Arc,
};

use miden_mast_package::Package;
use midenc_frontend_wasm_metadata::package_cache;
use proc_macro2::Span;
use syn::Error;

/// WIT source extracted from a compiled Miden dependency package.
pub(crate) struct DependencyWitSource {
    /// Manifest key used for this dependency.
    pub(crate) name: String,
    /// Canonical project root or precompiled package path.
    pub(crate) root: PathBuf,
    /// Path of the compiled `.masp` package the WIT was read from.
    pub(crate) package_path: PathBuf,
    /// The deserialized package, shared so later consumers (FPI procedure-root extraction) reuse
    /// the exact bytes this resolution read.
    pub(crate) package: Arc<Package>,
    /// The component WIT source: embedded in the package, or supplied by the dependency's `wit`
    /// manifest key when the package embeds none.
    pub(crate) wit: String,
    /// The `.wit` file the `wit` manifest key selected, when the WIT is not embedded.
    ///
    /// Recorded so consumers can register the file as a build input: it is the only source of
    /// the dependency's interface in that flow, and an edit to it must re-run the expansion.
    pub(crate) wit_override_path: Option<PathBuf>,
}

/// The result of resolving every declared Miden dependency's component WIT.
pub(crate) struct DependencyWitSources {
    /// Dependencies whose WIT resolved, in declaration order.
    pub(crate) sources: Vec<DependencyWitSource>,
    /// Dependencies that resolved to a package without component WIT (and no `wit` override).
    ///
    /// Not an error here: a link-only dependency — for example a MASM library — is never
    /// referenced by an SDK macro and needs no WIT. A macro that does reference one of these
    /// names is diagnosed at its lookup site with the recorded reason.
    pub(crate) skipped: Vec<SkippedDependency>,
    /// The compiler-written artifact map this resolution consumed, when one was present.
    ///
    /// Recorded so consumers can register the file as a build input.
    pub(crate) artifact_map_path: Option<PathBuf>,
}

// Manual impl: required by `expect_err` in tests, without requiring `Package: Debug` (which
// would dump the whole MAST forest).
impl core::fmt::Debug for DependencyWitSources {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("DependencyWitSources")
            .field("sources", &self.sources.iter().map(|source| &source.name).collect::<Vec<_>>())
            .field("skipped", &self.skipped.iter().map(|skipped| &skipped.name).collect::<Vec<_>>())
            .finish_non_exhaustive()
    }
}

/// A declared dependency that resolved, but without component WIT to consume.
#[derive(Debug)]
pub(crate) struct SkippedDependency {
    /// Manifest key of the dependency.
    pub(crate) name: String,
    /// Why its WIT is unavailable, phrased for a reference-site diagnostic.
    pub(crate) reason: String,
}

/// Reads the WIT of every Miden dependency's compiled package.
///
/// Resolution follows the compiler: when the package cache carries the artifact map the
/// compiler wrote for this consumer (`miden-deps/<consumer>.deps.toml`), every declared
/// dependency — whatever its source scheme — is read from the exact artifact the compiler
/// selected. Without a map (a hand-assembled cache), only path dependencies resolve, through
/// the legacy name probing.
///
/// Embedded WIT is authoritative. A dependency whose package embeds none may supply it manually
/// through the `package.metadata.miden.dependencies.<name>.wit` key in `miden-project.toml` — the
/// escape hatch for packages produced by toolchains that do not embed WIT. Setting the key for a
/// package that embeds WIT is an error, and a package with neither is skipped, not rejected:
/// only dependencies a macro references need WIT.
pub(crate) fn collect_dependency_wit_sources(
    manifest_dir: &Path,
    package: &miden_project::Package,
) -> Result<DependencyWitSources, Error> {
    let error_span = Span::call_site();
    let artifact_map = load_dependency_artifact_map(package)?;
    let mut collected = DependencyWitSources {
        sources: Vec::new(),
        skipped: Vec::new(),
        artifact_map_path: artifact_map.as_ref().map(|map| map.map_path.clone()),
    };

    for dependency in package.dependencies() {
        let name = dependency.name().as_ref();
        let resolved = match &artifact_map {
            // A `wit = false` entry is a link-only package (for example a base library or a
            // MASM-only dependency): the compiler recorded that there is no component WIT to
            // read, so its (potentially large) package is never deserialized here. Unless the
            // manifest supplies a `wit` override — the escape hatch for exactly such packages
            // — in which case the package is read for its procedure roots and the override
            // provides the interface, like on every other path.
            Some(map) => match map.resolve(name, error_span)? {
                MapResolution::Resolved(root, resolved) => Some((root, resolved)),
                MapResolution::LinkOnly(path) => {
                    if dependency_wit_override(package, name)?.is_some() {
                        let resolved = ResolvedDependencyPackage {
                            path: path.clone(),
                            package: read_package(&path)?,
                        };
                        Some((path, resolved))
                    } else {
                        collected.skipped.push(SkippedDependency {
                            name: name.to_string(),
                            reason: "the compiler recorded its package as embedding no component \
                                     WIT; it is consumed at link time only. If the package should \
                                     supply an interface, set \
                                     package.metadata.miden.dependencies.<name>.wit to a WIT file \
                                     describing it"
                                .to_string(),
                        });
                        continue;
                    }
                }
            },
            None => match dependency.scheme() {
                miden_project::DependencyVersionScheme::Path { path, .. } => {
                    Some(legacy_resolve_path_dependency(manifest_dir, name, path.path())?)
                }
                // Without the compiler's artifact map these schemes cannot be resolved at
                // expansion time; a macro reference to one of these names is diagnosed at
                // the lookup site instead.
                miden_project::DependencyVersionScheme::Workspace { .. }
                | miden_project::DependencyVersionScheme::WorkspacePath { .. }
                | miden_project::DependencyVersionScheme::Git { .. }
                | miden_project::DependencyVersionScheme::Registry(_) => None,
            },
        };
        let Some((dependency_root, resolved)) = resolved else {
            continue;
        };

        let wit_override = dependency_wit_override(package, name)?;
        let (wit, wit_override_path) =
            match (package_wit(&resolved.package, &resolved.path)?, wit_override) {
                (Some(_), Some(_)) => {
                    return Err(Error::new(
                        error_span,
                        format!(
                            "dependency '{name}': package '{}' embeds component WIT, but \
                             miden-project.toml also sets \
                             package.metadata.miden.dependencies.{name}.wit; remove the `wit` key \
                             — embedded WIT is authoritative",
                            resolved.path.display(),
                        ),
                    ));
                }
                (Some(wit), None) => (wit, None),
                (None, Some(wit_override)) => {
                    let (wit, override_path) =
                        read_wit_override(&wit_override, manifest_dir, name)?;
                    (wit, Some(override_path))
                }
                (None, None) => {
                    collected.skipped.push(SkippedDependency {
                        name: name.to_string(),
                        reason: missing_embedded_wit_message(&resolved.path, name),
                    });
                    continue;
                }
            };
        let source = DependencyWitSource {
            name: name.to_string(),
            root: dependency_root,
            package_path: resolved.path,
            package: resolved.package,
            wit,
            wit_override_path,
        };
        // The dependency's WIT must be self-contained apart from the bundled SDK WIT.
        // Validated here — the single point every macro path shares — so a package is
        // accepted or rejected identically by the `#[component]`-family macros and by bare
        // `generate!()` consumers, whose resolver would otherwise tolerate imports of
        // sibling dependency packages.
        if let Err(details) = crate::wit_world::parse_dependency_wit_source(&source.wit) {
            return Err(Error::new(
                error_span,
                crate::wit_world::dependency_wit_error_message(&source, &details),
            ));
        }
        collected.sources.push(source);
    }

    Ok(collected)
}

/// Resolves a path dependency without a compiler-written artifact map.
///
/// The legacy flow for hand-assembled caches: the dependency root is canonicalized, a root
/// that is itself a `.masp` file is read in place, and anything else is probed in the cache
/// by likely package-name stems. A root naming the project's manifest file is normalized to
/// the project directory first — both spellings are accepted by the compiler's resolver.
fn legacy_resolve_path_dependency(
    manifest_dir: &Path,
    dependency_name: &str,
    path: &str,
) -> Result<(PathBuf, ResolvedDependencyPackage), Error> {
    let error_span = Span::call_site();
    let absolute_path = manifest_dir.join(path);
    let mut dependency_root = fs::canonicalize(&absolute_path).map_err(|err| {
        Error::new(
            error_span,
            format!(
                "failed to canonicalize dependency '{dependency_name}' path '{}': {err}",
                absolute_path.display()
            ),
        )
    })?;
    if dependency_root.is_file()
        && dependency_root.file_name().is_some_and(|file_name| {
            file_name.eq_ignore_ascii_case("miden-project.toml")
                || file_name.eq_ignore_ascii_case("Cargo.toml")
        })
        && let Some(parent) = dependency_root.parent()
    {
        dependency_root = parent.to_path_buf();
    }
    let resolved = resolve_dependency_package(dependency_name, &dependency_root)?;
    Ok((dependency_root, resolved))
}

/// Describes a declared dependency whose scheme the macros cannot resolve, for diagnostics.
///
/// Returns `None` when the dependency is undeclared or uses a supported scheme.
pub(crate) fn unsupported_dependency_scheme(
    package: &miden_project::Package,
    dependency_name: &str,
) -> Option<&'static str> {
    let dependency = package
        .dependencies()
        .iter()
        .find(|dependency| dependency.name().as_ref() == dependency_name)?;
    match dependency.scheme() {
        miden_project::DependencyVersionScheme::Workspace { .. } => Some("workspace"),
        miden_project::DependencyVersionScheme::WorkspacePath { .. } => Some("workspace path"),
        miden_project::DependencyVersionScheme::Git { .. } => Some("git"),
        miden_project::DependencyVersionScheme::Path { .. }
        | miden_project::DependencyVersionScheme::Registry(_) => None,
    }
}

/// Returns the raw WIT override path from `package.metadata.miden.dependencies.<name>.wit`.
///
/// A malformed shape on the way to the key is an error rather than an absent key: silently
/// ignoring it would degrade to the "does not embed component WIT" diagnostic (or bypass the
/// embedded-WIT conflict error) while the user believes their key is in effect.
fn dependency_wit_override(
    package: &miden_project::Package,
    dependency_name: &str,
) -> Result<Option<String>, Error> {
    let Some(dependencies) =
        package.metadata().get("miden").and_then(|meta| meta.get("dependencies"))
    else {
        return Ok(None);
    };
    let dependencies = dependencies.as_table().ok_or_else(|| {
        Error::new(
            Span::call_site(),
            "invalid miden-project.toml configuration: expected \
             package.metadata.miden.dependencies to be a table",
        )
    })?;
    let Some(config) = dependencies.get(dependency_name) else {
        return Ok(None);
    };
    let config = config.as_table().ok_or_else(|| {
        Error::new(
            Span::call_site(),
            format!(
                "invalid miden-project.toml configuration: expected \
                 package.metadata.miden.dependencies.{dependency_name} to be a table"
            ),
        )
    })?;
    let Some(wit_value) = config.get("wit") else {
        return Ok(None);
    };
    let wit_path = wit_value.as_str().ok_or_else(|| {
        Error::new(
            Span::call_site(),
            format!(
                "invalid miden-project.toml configuration: expected \
                 package.metadata.miden.dependencies.{dependency_name}.wit to be a string"
            ),
        )
    })?;
    Ok(Some(wit_path.to_string()))
}

/// Reads a dependency's manually provided WIT from a `.wit` file or a directory containing
/// exactly one top-level `.wit` file, returning the WIT text and the selected file's path.
///
/// The override is validated like embedded WIT: it must resolve against the bundled SDK WIT alone
/// and export an interface, so every macro flow gets the accurate diagnostic at the source.
fn read_wit_override(
    wit_path: &str,
    manifest_dir: &Path,
    dependency_name: &str,
) -> Result<(String, PathBuf), Error> {
    let error_span = Span::call_site();
    let raw_path = Path::new(wit_path);
    let absolute_path = if raw_path.is_absolute() {
        raw_path.to_path_buf()
    } else {
        manifest_dir.join(raw_path)
    };
    let path = fs::canonicalize(&absolute_path).map_err(|err| {
        Error::new(
            error_span,
            format!(
                "failed to resolve the WIT override for dependency '{dependency_name}' from \
                 package.metadata.miden.dependencies.{dependency_name}.wit = '{wit_path}': '{}': \
                 {err}",
                absolute_path.display()
            ),
        )
    })?;

    let file = if path.is_dir() {
        let mut wit_files = crate::util::wit_files_in_dir(&path)?;
        match wit_files.len() {
            1 => wit_files.remove(0),
            count => {
                return Err(Error::new(
                    error_span,
                    format!(
                        "the WIT override directory '{}' for dependency '{dependency_name}' \
                         contains {count} `.wit` files; point \
                         package.metadata.miden.dependencies.{dependency_name}.wit at a single \
                         self-contained `.wit` file",
                        path.display()
                    ),
                ));
            }
        }
    } else {
        path.to_path_buf()
    };

    let wit = fs::read_to_string(&file).map_err(|err| {
        Error::new(
            error_span,
            format!(
                "failed to read the WIT override '{}' for dependency '{dependency_name}': {err}",
                file.display()
            ),
        )
    })?;
    crate::wit_world::parse_dependency_wit_source(&wit).map_err(|details| {
        Error::new(
            error_span,
            format!(
                "invalid WIT override for dependency '{dependency_name}' at '{}': {details}. The \
                 override must be self-contained apart from the bundled SDK WIT (`miden:base`) \
                 and export an interface.",
                file.display()
            ),
        )
    })?;
    Ok((wit, file))
}

/// Formats the diagnostic for a dependency package that embeds no WIT and has no override.
fn missing_embedded_wit_message(package_path: &Path, dependency_name: &str) -> String {
    format!(
        "dependency package '{}' does not embed component WIT (missing package section \
         '{wit_section}'); it was likely built with an older Miden toolchain. Rebuild the \
         dependency with the current `cargo miden build`, or provide the WIT manually via \
         package.metadata.miden.dependencies.{dependency_name}.wit in miden-project.toml. For \
         manually authored components (a hand-written `wit/` directory with a bare \
         `miden::generate!()`), the WIT is embedded only when the `wit/` directory contains \
         exactly one `.wit` file that is self-contained and exports an interface.",
        package_path.display(),
        wit_section = midenc_frontend_wasm_metadata::PACKAGE_WIT_SECTION_ID,
    )
}

/// Deserialized package reads, keyed by path and revalidated by content identity.
type PackageReadCache = std::collections::HashMap<PathBuf, (ContentIdentity, Arc<Package>)>;

/// The content identity of a package file: its length and a hash of its bytes.
///
/// File metadata is not identity: the adopted package cache keeps stable paths that are
/// replaced atomically, and a same-length replacement can carry a colliding or coarse
/// modification timestamp. The hash is not cryptographic — the cache defends against
/// accidental staleness in a long-lived proc-macro host, not against an adversary.
type ContentIdentity = (u64, u64);

/// Computes the [`ContentIdentity`] of package bytes.
fn content_identity(bytes: &[u8]) -> ContentIdentity {
    use std::hash::{Hash, Hasher};

    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut hasher);
    (bytes.len() as u64, hasher.finish())
}

thread_local! {
    /// One deserialized package per path, revalidated by content identity.
    ///
    /// A single expansion resolves the same dependency package from several entry points, and
    /// deserializing a MAST forest is expensive. The file is read on every call — the read is
    /// what makes one call see one consistent snapshot of an atomically replaced file — and
    /// the memo skips only the deserialization when the bytes are unchanged. The host process
    /// may outlive many builds (rust-analyzer keeps proc-macro servers running), so entries
    /// are never trusted by path or file metadata alone.
    static PACKAGE_READS: core::cell::RefCell<PackageReadCache> =
        core::cell::RefCell::new(std::collections::HashMap::new());
}

/// Reads and deserializes a compiled Miden package, reusing the previous deserialization of
/// unchanged bytes.
fn read_package(package_path: &Path) -> Result<Arc<Package>, Error> {
    let error_span = Span::call_site();
    let package_bytes = fs::read(package_path).map_err(|err| {
        Error::new(
            error_span,
            format!("failed to read dependency package '{}': {err}", package_path.display()),
        )
    })?;
    let identity = content_identity(&package_bytes);
    if let Some(package) = PACKAGE_READS.with(|reads| {
        reads.borrow().get(package_path).and_then(|(cached_identity, package)| {
            (*cached_identity == identity).then(|| package.clone())
        })
    }) {
        return Ok(package);
    }
    let package =
        Package::read_from_bytes_unchecked(&package_bytes)
            .map(Arc::new)
            .map_err(|err| {
                Error::new(
                    error_span,
                    format!(
                        "failed to deserialize dependency package '{}': {err}. The package may \
                         have been produced by a different Miden toolchain version; rebuild the \
                         dependency with the current `cargo miden build`.",
                        package_path.display()
                    ),
                )
            })?;
    PACKAGE_READS.with(|reads| {
        reads
            .borrow_mut()
            .insert(package_path.to_path_buf(), (identity, package.clone()));
    });
    Ok(package)
}

/// Extracts the component WIT embedded in a compiled Miden package.
///
/// Returns `Ok(None)` when the package has no WIT section; a section that is present but not
/// valid UTF-8 is an error (the package claims its own WIT, so nothing may substitute it).
fn package_wit(package: &Package, package_path: &Path) -> Result<Option<String>, Error> {
    let Some(wit_bytes) = midenc_frontend_wasm_metadata::package_wit(package) else {
        return Ok(None);
    };

    String::from_utf8(wit_bytes.to_vec()).map(Some).map_err(|err| {
        Error::new(
            Span::call_site(),
            format!(
                "dependency package '{}' contains an invalid component WIT section (not UTF-8): \
                 {err}",
                package_path.display()
            ),
        )
    })
}

/// The per-consumer artifact map the compiler writes into the package cache.
///
/// `miden-deps/<consumer>.deps.toml` records, for every declared dependency of the consumer
/// project, the artifact the compiler's resolution selected: a `package` file name inside the
/// cache directory, or an absolute `path` for a preassembled `.masp` consumed in place. When
/// the map is present it is authoritative — the macros follow the compiler instead of
/// re-deriving resolution from the manifests.
struct DependencyArtifactMap {
    /// The map file itself, tracked as a build input by consumers.
    map_path: PathBuf,
    /// The cache directory `package` entries are relative to.
    cache_dir: PathBuf,
    /// Declared dependency name → selected artifact.
    entries: std::collections::BTreeMap<String, ArtifactMapEntry>,
}

/// One artifact selection in a [`DependencyArtifactMap`].
/// One artifact selection recorded in the map.
struct ArtifactMapEntry {
    /// Where the selected artifact lives.
    location: ArtifactLocation,
    /// Whether the artifact embeds component WIT, when the compiler recorded it.
    ///
    /// `Some(false)` lets the macros skip a link-only package without deserializing it. An
    /// absent key (a map from an older writer) means unknown, and the package is read to
    /// find out.
    embeds_wit: Option<bool>,
}

enum ArtifactLocation {
    /// A file name inside the cache directory.
    CacheFile(String),
    /// An absolute path outside the cache (a preassembled dependency consumed in place).
    Path(PathBuf),
}

/// The outcome of resolving one declared dependency through the map.
enum MapResolution {
    /// The dependency's package was located and deserialized.
    Resolved(PathBuf, ResolvedDependencyPackage),
    /// The compiler recorded the package as embedding no component WIT; it was not read.
    ///
    /// The located path is carried so a `wit` manifest override — the escape hatch for
    /// exactly such packages — can still read the package for its procedure roots.
    LinkOnly(PathBuf),
}

impl DependencyArtifactMap {
    /// Resolves a declared dependency through the map.
    fn resolve(&self, dependency_name: &str, error_span: Span) -> Result<MapResolution, Error> {
        let Some(entry) = self.entries.get(dependency_name) else {
            return Err(Error::new(
                error_span,
                format!(
                    "dependency '{dependency_name}' is not recorded in the compiler's dependency \
                     manifest '{}'; the staged package cache is out of date with \
                     miden-project.toml — rebuild with `cargo miden build`, or re-run the check \
                     so the contract build script re-stages the packages",
                    self.map_path.display(),
                ),
            ));
        };
        let path = match &entry.location {
            ArtifactLocation::CacheFile(file) => self.cache_dir.join(file),
            ArtifactLocation::Path(path) => path.clone(),
        };
        if entry.embeds_wit == Some(false) {
            return Ok(MapResolution::LinkOnly(path));
        }
        let package = read_package(&path)?;
        Ok(MapResolution::Resolved(
            path.clone(),
            ResolvedDependencyPackage { path, package },
        ))
    }
}

/// Loads the artifact map the compiler wrote for `package`, when one exists.
///
/// `Ok(None)` when no package cache is configured, or the cache carries no map for this
/// consumer — the hand-assembled caches of tests and user tooling. A present map that cannot
/// be read or parsed is an error: the compiler wrote it, so corruption is a real problem.
fn load_dependency_artifact_map(
    package: &miden_project::Package,
) -> Result<Option<DependencyArtifactMap>, Error> {
    let error_span = Span::call_site();
    let Some(cache_dir) = package_cache_dir() else {
        return Ok(None);
    };
    let consumer = package.name().to_string();
    if consumer.is_empty() {
        return Ok(None);
    }
    let map_path = cache_dir
        .join(package_cache::DEPENDENCY_MANIFEST_DIR)
        .join(package_cache::dependency_map_file_name(&consumer));
    let contents = match fs::read_to_string(&map_path) {
        Ok(contents) => contents,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => {
            return Err(Error::new(
                error_span,
                format!(
                    "failed to read the compiler's dependency manifest '{}': {err}",
                    map_path.display()
                ),
            ));
        }
    };
    let table = contents.parse::<toml::Table>().map_err(|err| {
        Error::new(
            error_span,
            format!(
                "failed to parse the compiler's dependency manifest '{}': {err}",
                map_path.display()
            ),
        )
    })?;
    let map_error = |message: String| Error::new(error_span, message);
    let schema = table.get("schema").and_then(toml::Value::as_integer);
    if schema != Some(package_cache::DEPENDENCY_MAP_SCHEMA) {
        return Err(map_error(format!(
            "the compiler's dependency manifest '{}' has an unsupported schema {schema:?}; the \
             package cache was staged by an incompatible toolchain — rebuild with the current \
             `cargo miden build`",
            map_path.display(),
        )));
    }
    let mut entries = std::collections::BTreeMap::new();
    if let Some(dependencies) = table.get("dependencies") {
        let dependencies = dependencies.as_table().ok_or_else(|| {
            map_error(format!(
                "the compiler's dependency manifest '{}' has a malformed `dependencies` table",
                map_path.display(),
            ))
        })?;
        for (name, value) in dependencies {
            let entry = value.as_table().and_then(|entry| {
                let location =
                    if let Some(file) = entry.get("package").and_then(toml::Value::as_str) {
                        Some(ArtifactLocation::CacheFile(file.to_string()))
                    } else {
                        entry
                            .get("path")
                            .and_then(toml::Value::as_str)
                            .map(|path| ArtifactLocation::Path(PathBuf::from(path)))
                    };
                location.map(|location| ArtifactMapEntry {
                    location,
                    embeds_wit: entry.get("wit").and_then(toml::Value::as_bool),
                })
            });
            let Some(entry) = entry else {
                return Err(map_error(format!(
                    "the compiler's dependency manifest '{}' has a malformed entry for dependency \
                     '{name}'",
                    map_path.display(),
                )));
            };
            entries.insert(name.clone(), entry);
        }
    }
    Ok(Some(DependencyArtifactMap {
        map_path,
        cache_dir,
        entries,
    }))
}

/// A located and deserialized dependency package.
struct ResolvedDependencyPackage {
    /// Path of the `.masp` file the package was read from.
    path: PathBuf,
    /// The deserialized package.
    package: Arc<Package>,
}

// Manual impl: required by `expect_err` in tests, without requiring `Package: Debug` (which
// would dump the whole MAST forest).
impl core::fmt::Debug for ResolvedDependencyPackage {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ResolvedDependencyPackage")
            .field("path", &self.path)
            .finish_non_exhaustive()
    }
}

/// Finds and reads the `.masp` package artifact for the dependency named `name` rooted at `root`.
///
/// A `root` that is itself a `.masp` file is the manifest's explicit choice and is read from
/// that location (the manifest key need not equal the prebuilt package's id). Every other
/// dependency package is read from the `MIDENC_PACKAGE_CACHE` directory under its package name,
/// trying the hyphen/underscore stem spellings the cache writers use. The cache belongs to one
/// build — a unique per-build directory, or the stable directory a build exported — so the
/// package found under the dependency's name is trusted as-is; id, version, and digest
/// verification belong to the compiler's project resolution. Without a configured cache the
/// dependency cannot be resolved at all.
fn resolve_dependency_package(name: &str, root: &Path) -> Result<ResolvedDependencyPackage, Error> {
    if root.is_file() {
        // Only a `.masp` file is an explicit package choice; any other file here is a
        // mis-declared path (manifest files are normalized to their directory upstream).
        if !root
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case(Package::EXTENSION))
        {
            return Err(Error::new(
                Span::call_site(),
                format!(
                    "dependency '{name}': path '{}' names a file that is not a `.{}` package",
                    root.display(),
                    Package::EXTENSION,
                ),
            ));
        }
        return Ok(ResolvedDependencyPackage {
            path: root.to_path_buf(),
            package: read_package(root)?,
        });
    }

    let Some(filesystem_cache_dir) = package_cache_dir() else {
        return Err(Error::new(Span::call_site(), missing_package_cache_message(name, root)));
    };

    let package_stems = dependency_package_stems(name, root);
    for stem in &package_stems {
        let candidate = filesystem_cache_dir.join(package_cache::package_file_name(stem));
        if candidate.is_file() {
            return Ok(ResolvedDependencyPackage {
                package: read_package(&candidate)?,
                path: candidate,
            });
        }
    }

    Err(Error::new(
        Span::call_site(),
        missing_cached_dependency_package_message(
            name,
            root,
            &package_stems,
            &filesystem_cache_dir,
        ),
    ))
}

/// Returns the package cache directory of this expansion, when one is configured.
fn package_cache_dir() -> Option<PathBuf> {
    #[cfg(test)]
    if let Some(overridden) = TEST_PACKAGE_CACHE_DIR.with(|dir| dir.borrow().clone()) {
        return overridden;
    }
    // An empty value counts as unset at every boundary of the variable, matching the
    // compiler's adoption check and the contract build script's guard. The value is
    // absolutized like the compiler's adoption does — and because the resolved package
    // paths are emitted into `include_bytes!` tracking constants, which rustc resolves
    // against the containing source file rather than the working directory.
    env::var_os(package_cache::PACKAGE_CACHE_ENV)
        .filter(|value| !value.is_empty())
        .map(|value| {
            let path = PathBuf::from(value);
            std::path::absolute(&path).unwrap_or(path)
        })
}

#[cfg(test)]
thread_local! {
    /// Test override for the package cache directory.
    ///
    /// The process environment is global, so parallel unit tests cannot use it to point each
    /// expansion at its own fixture cache. `Some(None)` simulates an unset variable.
    static TEST_PACKAGE_CACHE_DIR: core::cell::RefCell<Option<Option<PathBuf>>> =
        const { core::cell::RefCell::new(None) };
}

/// Runs `run` with the package cache directory overridden for the current thread.
///
/// `None` simulates a build without a configured cache. The override is reset even when `run`
/// panics, so an assertion failure cannot leak it into another test on the same thread.
#[cfg(test)]
pub(crate) fn with_test_package_cache_dir<R>(
    cache_dir: Option<&Path>,
    run: impl FnOnce() -> R,
) -> R {
    struct ResetOnDrop;
    impl Drop for ResetOnDrop {
        fn drop(&mut self) {
            TEST_PACKAGE_CACHE_DIR.with(|dir| {
                *dir.borrow_mut() = None;
            });
        }
    }

    TEST_PACKAGE_CACHE_DIR.with(|dir| {
        *dir.borrow_mut() = Some(cache_dir.map(Path::to_path_buf));
    });
    let _reset = ResetOnDrop;
    run()
}

/// Formats the diagnostic for a dependency package missing from the build-owned package cache.
fn missing_cached_dependency_package_message(
    name: &str,
    root: &Path,
    package_stems: &[String],
    filesystem_cache_dir: &Path,
) -> String {
    let expected_files = package_stems
        .iter()
        .map(|stem| format!("'{stem}.masp'"))
        .collect::<Vec<_>>()
        .join(", ");

    format!(
        "could not find a built `.masp` package for Miden dependency '{name}' (root '{}'). The \
         SDK macros need the dependency package during Rust macro expansion to read its embedded \
         WIT and procedure roots. Expected one of these package names: {expected_files}. Searched \
         {} directory '{}'. The cache is populated by a midenc-driven build (`cargo miden \
         build`), and by the contract `build.rs` for plain cargo builds; rebuild through either \
         so the dependency package is available during macro expansion.",
        root.display(),
        package_cache::PACKAGE_CACHE_ENV,
        filesystem_cache_dir.display(),
    )
}

/// Formats the diagnostic for an expansion without a configured package cache.
fn missing_package_cache_message(name: &str, root: &Path) -> String {
    format!(
        "the Miden package cache is not configured ({} is not set), so the compiled package for \
         Miden dependency '{name}' (root '{}') cannot be resolved during Rust macro expansion. \
         Build through `cargo miden build`, which exports the variable to its nested builds, or \
         add the contract `build.rs` from a generated template so plain `cargo build`/`cargo \
         check` and IDE analysis populate and export the cache.",
        package_cache::PACKAGE_CACHE_ENV,
        root.display(),
    )
}

/// Returns likely `.masp` filename stems for a dependency, most authoritative first.
///
/// The cache writer names each file after the *Miden* package name — `miden-project.toml`'s
/// `[package].name` — so that stem is probed first. The Cargo package name, the manifest
/// dependency key, and the directory name are fallbacks for prebuilt artifacts named under
/// older conventions.
fn dependency_package_stems(name: &str, root: &Path) -> Vec<String> {
    let mut stems = Vec::new();

    if let Some(package_name) = manifest_package_name(&root.join("miden-project.toml")) {
        push_dependency_stem(&mut stems, &package_name);
    }

    if let Some(package_name) = manifest_package_name(&root.join("Cargo.toml")) {
        push_dependency_stem(&mut stems, &package_name);
    }

    if let Some(name) = name.split([':', '/']).next_back() {
        push_dependency_stem(&mut stems, name);
    }

    if let Some(name) = root.file_name().and_then(|name| name.to_str()) {
        push_dependency_stem(&mut stems, name);
    }

    stems
}

/// Reads a TOML manifest's `[package].name`.
fn manifest_package_name(manifest_path: &Path) -> Option<String> {
    let manifest = fs::read_to_string(manifest_path).ok()?;
    let manifest = manifest.parse::<toml::Table>().ok()?;
    manifest
        .get("package")
        .and_then(toml::Value::as_table)
        .and_then(|package| package.get("name"))
        .and_then(toml::Value::as_str)
        .map(ToOwned::to_owned)
}

/// Adds Miden package stem candidates if they have not already been added.
fn push_dependency_stem(stems: &mut Vec<String>, name: &str) {
    if !name.is_empty() && !stems.iter().any(|existing| existing == name) {
        stems.push(name.to_owned());
    }

    let normalized = name.replace('-', "_");
    if !normalized.is_empty() && !stems.iter().any(|existing| existing == &normalized) {
        stems.push(normalized);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::write_masp_fixture;

    /// Creates a unique fixture root under the temp dir.
    fn fixture_root(name: &str) -> PathBuf {
        let root =
            env::temp_dir().join(format!("midenc-dep-package-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn dependency_stem_preserves_package_filename_before_legacy_alias() {
        let mut stems = Vec::new();

        push_dependency_stem(&mut stems, "no-arg-account");

        assert_eq!(stems, ["no-arg-account", "no_arg_account"]);
    }

    #[test]
    fn probes_the_miden_package_name_stem_first() {
        // The cache writer names files after miden-project.toml's `[package].name`; a dependency
        // whose Miden name differs from its Cargo name, manifest key, and directory must still
        // resolve.
        let temp_root = fixture_root("miden-name-stem");
        let cache_dir = temp_root.join("package-cache");
        let dependency_root = temp_root.join("dirname");
        std::fs::create_dir_all(&dependency_root).unwrap();
        std::fs::write(
            dependency_root.join("miden-project.toml"),
            "[package]\nname = \"miden-name\"\nversion = \"1.0.0\"\n\n[lib]\npath = \
             \"src/lib.rs\"\n",
        )
        .unwrap();
        std::fs::write(dependency_root.join("Cargo.toml"), "[package]\nname = \"cargo_name\"\n")
            .unwrap();
        let package_path = cache_dir.join("miden-name.masp");
        write_masp_fixture(&package_path, "miden-name", None);

        let resolved = with_test_package_cache_dir(Some(&cache_dir), || {
            resolve_dependency_package("dep-key", &dependency_root)
        })
        .unwrap();

        assert_eq!(resolved.path, package_path);

        std::fs::remove_dir_all(temp_root).unwrap();
    }

    #[test]
    fn reuses_the_read_of_an_unchanged_package_and_rereads_after_a_rewrite() {
        let temp_root = fixture_root("read-memo");
        let package_path = temp_root.join("dep.masp");
        write_masp_fixture(&package_path, "first-name", None);

        let first = read_package(&package_path).unwrap();
        let again = read_package(&package_path).unwrap();
        assert!(Arc::ptr_eq(&first, &again), "an unchanged file must reuse the previous read");

        // The rewritten package carries a longer id, so the file length changes even when the
        // modification time granularity is coarse.
        write_masp_fixture(&package_path, "second-longer-name", None);
        let rewritten = read_package(&package_path).unwrap();
        assert_eq!(
            rewritten.name.to_string(),
            "second-longer-name",
            "a rewritten file must be re-read"
        );

        std::fs::remove_dir_all(temp_root).unwrap();
    }

    #[test]
    fn unsupported_dependency_scheme_names_only_unsupported_schemes() {
        let workspace_path = miden_project::Dependency::new(
            miden_assembly_syntax::debuginfo::Span::unknown(Arc::<str>::from("ws-dep")),
            miden_project::DependencyVersionScheme::WorkspacePath {
                path: miden_assembly_syntax::debuginfo::Span::unknown(miden_project::Uri::new(
                    "sibling",
                )),
                version: None,
            },
            miden_project::Linkage::Dynamic,
        );
        let path = miden_project::Dependency::new(
            miden_assembly_syntax::debuginfo::Span::unknown(Arc::<str>::from("path-dep")),
            miden_project::DependencyVersionScheme::Path {
                path: miden_assembly_syntax::debuginfo::Span::unknown(miden_project::Uri::new(
                    "sibling",
                )),
                version: None,
            },
            miden_project::Linkage::Dynamic,
        );
        let target = miden_project::Target::new(
            miden_project::TargetType::Library,
            "default",
            miden_assembly_syntax::ast::Path::new("empty"),
            miden_project::Uri::new("lib/src.rs"),
        );
        let package = miden_project::Package::new("consumer", target)
            .with_dependencies([workspace_path, path]);

        assert_eq!(unsupported_dependency_scheme(&package, "ws-dep"), Some("workspace path"));
        assert_eq!(unsupported_dependency_scheme(&package, "path-dep"), None);
        assert_eq!(unsupported_dependency_scheme(&package, "undeclared"), None);
    }

    #[test]
    fn resolves_the_dependency_package_from_the_cache_by_stem() {
        let temp_root = fixture_root("cache-hit");
        let cache_dir = temp_root.join("package-cache");
        let dependency_root = temp_root.join("dep-fixture");
        std::fs::create_dir_all(&dependency_root).unwrap();
        // The underscore spelling exercises the stem aliases: the hyphen probe misses first.
        let package_path = cache_dir.join("dep_fixture.masp");
        write_masp_fixture(&package_path, "dep-fixture", None);

        let resolved = with_test_package_cache_dir(Some(&cache_dir), || {
            resolve_dependency_package("dep-fixture", &dependency_root)
        })
        .unwrap();

        assert_eq!(resolved.path, package_path);

        std::fs::remove_dir_all(temp_root).unwrap();
    }

    #[test]
    fn explicit_package_file_dependency_bypasses_the_cache() {
        let temp_root = fixture_root("explicit-file");
        let package_path = temp_root.join("prebuilt/renamed.masp");
        write_masp_fixture(&package_path, "dep-fixture", None);

        let resolved = with_test_package_cache_dir(None, || {
            resolve_dependency_package("dep-fixture", &package_path)
        })
        .unwrap();

        assert_eq!(resolved.path, package_path);

        std::fs::remove_dir_all(temp_root).unwrap();
    }

    #[test]
    fn missing_package_cache_reports_actionable_error() {
        let temp_root = fixture_root("no-cache");
        let dependency_root = temp_root.join("dep-fixture");
        std::fs::create_dir_all(&dependency_root).unwrap();

        let error = with_test_package_cache_dir(None, || {
            resolve_dependency_package("dep-fixture", &dependency_root)
        })
        .expect_err("resolution without a configured cache must fail");
        let message = error.to_string();

        assert!(
            message.contains("MIDENC_PACKAGE_CACHE is not set"),
            "unexpected error: {message}"
        );
        assert!(message.contains("cargo miden build"), "unexpected error: {message}");
        assert!(message.contains("build.rs"), "unexpected error: {message}");

        std::fs::remove_dir_all(temp_root).unwrap();
    }

    #[test]
    fn missing_cached_dependency_package_reports_the_cache_contract() {
        let temp_root = fixture_root("cache-miss");
        let cache_dir = temp_root.join("package-cache");
        std::fs::create_dir_all(&cache_dir).unwrap();
        let dependency_root = temp_root.join("dep-fixture");
        std::fs::create_dir_all(&dependency_root).unwrap();

        let error = with_test_package_cache_dir(Some(&cache_dir), || {
            resolve_dependency_package("dep-fixture", &dependency_root)
        })
        .expect_err("an empty cache must fail resolution");
        let message = error.to_string();

        assert!(
            message.contains("could not find a built `.masp` package"),
            "unexpected error: {message}"
        );
        assert!(message.contains("'dep-fixture.masp'"), "unexpected error: {message}");
        assert!(message.contains("'dep_fixture.masp'"), "unexpected error: {message}");
        assert!(
            message.contains(&cache_dir.display().to_string()),
            "unexpected error: {message}"
        );
        assert!(message.contains("cargo miden build"), "unexpected error: {message}");
        assert!(message.contains("build.rs"), "unexpected error: {message}");
        assert!(!message.contains("target/miden/<profile>"), "unexpected error: {message}");

        std::fs::remove_dir_all(temp_root).unwrap();
    }

    #[test]
    fn corrupt_dependency_package_reports_rebuild_hint() {
        let temp_root = fixture_root("corrupt");
        let cache_dir = temp_root.join("package-cache");
        std::fs::create_dir_all(&cache_dir).unwrap();
        let dependency_root = temp_root.join("dep-fixture");
        std::fs::create_dir_all(&dependency_root).unwrap();
        std::fs::write(cache_dir.join("dep-fixture.masp"), b"garbage").unwrap();

        let error = with_test_package_cache_dir(Some(&cache_dir), || {
            resolve_dependency_package("dep-fixture", &dependency_root)
        })
        .expect_err("a corrupt dependency package must fail resolution");
        let message = error.to_string();

        assert!(message.contains("failed to deserialize"), "unexpected error: {message}");
        assert!(
            message.contains("different Miden toolchain version"),
            "unexpected error: {message}"
        );
        assert!(message.contains("cargo miden build"), "unexpected error: {message}");

        std::fs::remove_dir_all(temp_root).unwrap();
    }

    /// Builds a consumer package named `consumer` with one workspace-scheme dependency.
    ///
    /// The workspace scheme is unresolvable at expansion time without the compiler's artifact
    /// map, which makes it the sharpest probe of map-based resolution.
    fn consumer_with_workspace_dependency(dependency_name: &str) -> Box<miden_project::Package> {
        use miden_assembly_syntax::{ast, debuginfo::Span as MidenSpan};

        let target = miden_project::Target::new(
            miden_project::TargetType::Library,
            "default",
            ast::Path::new("empty"),
            miden_project::Uri::new("lib/src.rs"),
        );
        let dependency = miden_project::Dependency::new(
            MidenSpan::unknown(Arc::<str>::from(dependency_name)),
            miden_project::DependencyVersionScheme::Workspace {
                member: MidenSpan::unknown(miden_project::Uri::new(dependency_name)),
                version: None,
            },
            miden_project::Linkage::Dynamic,
        );
        miden_project::Package::new("consumer", target).with_dependencies([dependency])
    }

    fn consumer_with_registry_dependency(dependency_name: &str) -> Box<miden_project::Package> {
        use miden_assembly_syntax::{ast, debuginfo::Span as MidenSpan};

        let target = miden_project::Target::new(
            miden_project::TargetType::Library,
            "default",
            ast::Path::new("empty"),
            miden_project::Uri::new("lib/src.rs"),
        );
        let dependency = miden_project::Dependency::new(
            MidenSpan::unknown(Arc::<str>::from(dependency_name)),
            miden_project::DependencyVersionScheme::Registry(
                miden_project::VersionRequirement::Semantic(MidenSpan::unknown(
                    "^0.1.0".parse().unwrap(),
                )),
            ),
            miden_project::Linkage::Dynamic,
        );
        miden_project::Package::new("consumer", target).with_dependencies([dependency])
    }

    const MAPPED_DEP_WIT: &str = "package miden:mapped-dep@0.1.0;\n\ninterface api {\n  get: \
                                  func() -> u64;\n}\n\nworld w {\n  export api;\n}\n";

    #[test]
    fn artifact_map_resolves_any_dependency_scheme() {
        // The compiler records the artifact it selected for every declared dependency; with the
        // map present the macros follow it, so even schemes they cannot resolve themselves —
        // here a workspace member — read the exact selected package.
        let temp_root = fixture_root("artifact-map");
        let cache_dir = temp_root.join("package-cache");
        write_masp_fixture(&cache_dir.join("mapped-dep.masp"), "mapped-dep", Some(MAPPED_DEP_WIT));
        let map_dir = cache_dir.join("miden-deps");
        std::fs::create_dir_all(&map_dir).unwrap();
        std::fs::write(
            map_dir.join("consumer.deps.toml"),
            "schema = 1\n\n[dependencies]\nthe-dep = { package = \"mapped-dep.masp\", version = \
             \"0.1.0\" }\n",
        )
        .unwrap();
        let package = consumer_with_workspace_dependency("the-dep");

        let collected = with_test_package_cache_dir(Some(&cache_dir), || {
            collect_dependency_wit_sources(&temp_root, &package)
        })
        .expect("the map must resolve the workspace dependency");

        assert_eq!(collected.sources.len(), 1);
        assert_eq!(collected.sources[0].name, "the-dep");
        assert_eq!(collected.sources[0].package_path, cache_dir.join("mapped-dep.masp"));
        assert_eq!(
            collected.artifact_map_path.as_deref(),
            Some(map_dir.join("consumer.deps.toml").as_path()),
            "the consumed map must be reported for build-input tracking"
        );

        std::fs::remove_dir_all(temp_root).unwrap();
    }

    #[test]
    fn artifact_map_resolves_a_registry_component() {
        // The registry scheme is generic and can select an assembled component; with the
        // compiler's map present it resolves exactly like every other scheme.
        let temp_root = fixture_root("artifact-map-registry");
        let cache_dir = temp_root.join("package-cache");
        write_masp_fixture(&cache_dir.join("mapped-dep.masp"), "mapped-dep", Some(MAPPED_DEP_WIT));
        let map_dir = cache_dir.join("miden-deps");
        std::fs::create_dir_all(&map_dir).unwrap();
        std::fs::write(
            map_dir.join("consumer.deps.toml"),
            "schema = 1\n\n[dependencies]\nthe-dep = { package = \"mapped-dep.masp\", version = \
             \"0.1.0\" }\n",
        )
        .unwrap();
        let package = consumer_with_registry_dependency("the-dep");

        let collected = with_test_package_cache_dir(Some(&cache_dir), || {
            collect_dependency_wit_sources(&temp_root, &package)
        })
        .expect("the map must resolve the registry dependency");

        assert_eq!(collected.sources.len(), 1);
        assert_eq!(collected.sources[0].name, "the-dep");
        assert_eq!(collected.sources[0].package_path, cache_dir.join("mapped-dep.masp"));

        std::fs::remove_dir_all(temp_root).unwrap();
    }

    /// `package.metadata.miden.dependencies.<name>.wit = <path>` as a [`miden_project`]
    /// metadata set.
    fn wit_override_metadata(dependency_name: &str, wit_path: &str) -> miden_project::MetadataSet {
        use miden_assembly_syntax::debuginfo::Span as MidenSpan;
        use toml::{Value, value::Table};

        let mut dependency_config = Table::new();
        dependency_config.insert("wit".to_string(), Value::String(wit_path.to_string()));
        let mut dependencies = Table::new();
        dependencies.insert(dependency_name.to_string(), Value::Table(dependency_config));
        let mut miden_metadata = miden_project::Metadata::default();
        miden_metadata.insert(
            MidenSpan::unknown(Arc::<str>::from("dependencies")),
            MidenSpan::unknown(Value::Table(dependencies)),
        );
        let mut metadata = miden_project::MetadataSet::default();
        metadata.insert(MidenSpan::unknown(Arc::<str>::from("miden")), miden_metadata);
        metadata
    }

    #[test]
    fn recorded_link_only_entry_honors_the_wit_override() {
        // `wit = false` records that the package embeds no component WIT — which is exactly
        // the case the `wit` manifest key exists for. The override must win over the fast
        // path: the package is read for its procedure roots, and the key supplies the
        // interface.
        let temp_root = fixture_root("link-only-wit-override");
        let cache_dir = temp_root.join("package-cache");
        write_masp_fixture(&cache_dir.join("foreign-dep.masp"), "foreign-dep", None);
        let override_path = temp_root.join("foreign-dep.wit");
        std::fs::write(&override_path, MAPPED_DEP_WIT).unwrap();
        let map_dir = cache_dir.join("miden-deps");
        std::fs::create_dir_all(&map_dir).unwrap();
        std::fs::write(
            map_dir.join("consumer.deps.toml"),
            "schema = 1\n\n[dependencies]\nthe-dep = { package = \"foreign-dep.masp\", version = \
             \"0.1.0\", wit = false }\n",
        )
        .unwrap();
        let package = consumer_with_registry_dependency("the-dep")
            .with_metadata(wit_override_metadata("the-dep", &override_path.to_string_lossy()));

        let collected = with_test_package_cache_dir(Some(&cache_dir), || {
            collect_dependency_wit_sources(&temp_root, &package)
        })
        .expect("the wit override must resolve the recorded link-only dependency");

        assert_eq!(collected.sources.len(), 1);
        assert_eq!(collected.sources[0].name, "the-dep");
        assert_eq!(collected.sources[0].package_path, cache_dir.join("foreign-dep.masp"));
        assert_eq!(
            collected.sources[0].wit_override_path.as_deref(),
            // The override flow canonicalizes; canonicalize the expectation too so macOS's
            // `/var` symlink does not fail the comparison.
            Some(override_path.canonicalize().unwrap().as_path()),
            "the override file must be reported as a build input"
        );
        assert!(collected.skipped.is_empty());

        std::fs::remove_dir_all(temp_root).unwrap();
    }

    #[test]
    fn recorded_link_only_entries_are_skipped_without_reading_the_package() {
        // `wit = false` is the compiler's record that the package embeds no component WIT.
        // The package file deliberately does not exist in the fixture cache: a skip that
        // tried to read it would fail, proving link-only packages are never deserialized.
        let temp_root = fixture_root("link-only-fast-path");
        let cache_dir = temp_root.join("package-cache");
        let map_dir = cache_dir.join("miden-deps");
        std::fs::create_dir_all(&map_dir).unwrap();
        std::fs::write(
            map_dir.join("consumer.deps.toml"),
            "schema = 1\n\n[dependencies]\nmiden-core = { package = \"miden-core.masp\", version \
             = \"0.1.0\", wit = false }\n",
        )
        .unwrap();
        let package = consumer_with_registry_dependency("miden-core");

        let collected = with_test_package_cache_dir(Some(&cache_dir), || {
            collect_dependency_wit_sources(&temp_root, &package)
        })
        .expect("a recorded link-only package must be skipped without being read");

        assert!(collected.sources.is_empty());
        assert_eq!(collected.skipped.len(), 1);
        assert_eq!(collected.skipped[0].name, "miden-core");
        assert!(collected.skipped[0].reason.contains("link time"));

        std::fs::remove_dir_all(temp_root).unwrap();
    }

    #[test]
    fn artifact_map_missing_entry_reports_staging_drift() {
        // A present map is authoritative: a declared dependency the compiler did not record
        // means the staged cache predates the manifest edit, which re-staging fixes.
        let temp_root = fixture_root("artifact-map-drift");
        let cache_dir = temp_root.join("package-cache");
        let map_dir = cache_dir.join("miden-deps");
        std::fs::create_dir_all(&map_dir).unwrap();
        std::fs::write(map_dir.join("consumer.deps.toml"), "schema = 1\n\n[dependencies]\n")
            .unwrap();
        let package = consumer_with_workspace_dependency("the-dep");

        let error = with_test_package_cache_dir(Some(&cache_dir), || {
            collect_dependency_wit_sources(&temp_root, &package)
        })
        .expect_err("a declared dependency absent from the map must fail");
        let message = error.to_string();

        assert!(
            message.contains("not recorded in the compiler's dependency manifest"),
            "unexpected error: {message}"
        );
        assert!(message.contains("cargo miden build"), "unexpected error: {message}");

        std::fs::remove_dir_all(temp_root).unwrap();
    }
}
