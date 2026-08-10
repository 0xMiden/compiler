//! Locating and reading compiled Miden dependency packages (`.masp`) at macro-expansion time.
//!
//! A Miden path dependency is consumed through its compiled package: the `.masp` carries both the
//! dependency's embedded component WIT (read here) and its procedure roots (read by [`crate::fpi`]).
//!
//! Every dependency package comes from the build-owned package cache named by
//! `MIDENC_PACKAGE_CACHE`. A midenc-driven build compiles the dependencies, publishes them into
//! the fingerprinted cache, and exports the variable to its nested cargo builds; the contract
//! `build.rs` does the same for plain `cargo build`/`cargo check` and IDE analysis. The macros
//! never search the filesystem for packages themselves — the one exception is a dependency whose
//! manifest path names a `.masp` file directly, which is read from that explicit location.

use std::{
    env, fs,
    path::{Path, PathBuf},
    sync::Arc,
};

use miden_mast_package::{Package, SectionId};
use midenc_frontend_wasm_metadata::PACKAGE_WIT_SECTION_ID;
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

/// Reads the WIT of every Miden path dependency's compiled package.
///
/// Embedded WIT is authoritative. A dependency whose package embeds none may supply it manually
/// through the `package.metadata.miden.dependencies.<name>.wit` key in `miden-project.toml` — the
/// escape hatch for packages produced by toolchains that do not embed WIT. Setting the key for a
/// package that embeds WIT is an error.
pub(crate) fn collect_dependency_wit_sources(
    manifest_dir: &Path,
    package: &miden_project::Package,
) -> Result<Vec<DependencyWitSource>, Error> {
    let error_span = Span::call_site();
    let mut sources = Vec::new();

    for dependency in package.dependencies() {
        match dependency.scheme() {
            miden_project::DependencyVersionScheme::Path { path, .. } => {
                let absolute_path = manifest_dir.join(path.path());
                let dependency_root = fs::canonicalize(&absolute_path).map_err(|err| {
                    Error::new(
                        error_span,
                        format!(
                            "failed to canonicalize dependency '{}' path '{}': {err}",
                            dependency.name(),
                            absolute_path.display()
                        ),
                    )
                })?;
                let resolved =
                    resolve_dependency_package(dependency.name().as_ref(), &dependency_root)?;
                let wit_override = dependency_wit_override(package, dependency.name().as_ref())?;
                let (wit, wit_override_path) =
                    match (package_wit(&resolved.package, &resolved.path)?, wit_override) {
                        (Some(_), Some(_)) => {
                            return Err(Error::new(
                                error_span,
                                format!(
                                    "dependency '{}': package '{}' embeds component WIT, but \
                                     miden-project.toml also sets \
                                     package.metadata.miden.dependencies.{}.wit; remove the `wit` \
                                     key — embedded WIT is authoritative",
                                    dependency.name(),
                                    resolved.path.display(),
                                    dependency.name(),
                                ),
                            ));
                        }
                        (Some(wit), None) => (wit, None),
                        (None, Some(wit_override)) => {
                            let (wit, override_path) = read_wit_override(
                                &wit_override,
                                manifest_dir,
                                dependency.name().as_ref(),
                            )?;
                            (wit, Some(override_path))
                        }
                        (None, None) => {
                            return Err(Error::new(
                                error_span,
                                missing_embedded_wit_message(
                                    &resolved.path,
                                    dependency.name().as_ref(),
                                ),
                            ));
                        }
                    };
                sources.push(DependencyWitSource {
                    name: dependency.name().to_string(),
                    root: dependency_root,
                    package_path: resolved.path,
                    package: resolved.package,
                    wit,
                    wit_override_path,
                });
            }
            // Registry dependencies are MASM base libraries (`miden-core`, `miden-protocol`)
            // consumed at link time only, so they carry no component WIT.
            miden_project::DependencyVersionScheme::Registry(_) => {}
            // Not supported at macro expansion time. Skipped without an error because a crate
            // may declare such a dependency without consuming it in a macro; a macro reference
            // to one of these names is diagnosed at the lookup site instead.
            miden_project::DependencyVersionScheme::Workspace { .. }
            | miden_project::DependencyVersionScheme::WorkspacePath { .. }
            | miden_project::DependencyVersionScheme::Git { .. } => {}
        }
    }

    Ok(sources)
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
        let mut wit_files = fs::read_dir(&path)
            .map_err(|err| {
                Error::new(
                    error_span,
                    format!(
                        "failed to read the WIT override directory '{}' for dependency \
                         '{dependency_name}': {err}",
                        path.display()
                    ),
                )
            })?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|err| {
                Error::new(
                    error_span,
                    format!(
                        "failed to iterate the WIT override directory '{}' for dependency \
                         '{dependency_name}': {err}",
                        path.display()
                    ),
                )
            })?
            .into_iter()
            .map(|entry| entry.path())
            .filter(|path| {
                path.is_file() && path.extension().is_some_and(|extension| extension == "wit")
            })
            .collect::<Vec<_>>();
        wit_files.sort();
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
         '{PACKAGE_WIT_SECTION_ID}'); it was likely built with an older Miden toolchain. Rebuild \
         the dependency with the current `cargo miden build`, or provide the WIT manually via \
         package.metadata.miden.dependencies.{dependency_name}.wit in miden-project.toml. For \
         manually authored components (a hand-written `wit/` directory with a bare \
         `miden::generate!()`), the WIT is embedded only when the `wit/` directory contains \
         exactly one `.wit` file that is self-contained and exports an interface.",
        package_path.display()
    )
}

/// Returns the package section id carrying the embedded component WIT.
pub(crate) fn wit_section_id() -> SectionId {
    SectionId::custom(PACKAGE_WIT_SECTION_ID)
        .expect("the WIT section id must be a valid custom section id")
}

/// Deserialized package reads, keyed by path and validated by (modification time, length).
type PackageReadCache =
    std::collections::HashMap<PathBuf, (std::time::SystemTime, u64, Arc<Package>)>;

thread_local! {
    /// One deserialized package per path, revalidated by file modification time and length.
    ///
    /// A single expansion resolves the same dependency package from several entry points, and
    /// deserializing a MAST forest is expensive — worse, split reads could pair bindings from
    /// one cache generation with procedure roots from another when a concurrent build rewrites
    /// the package between them. The host process may also outlive many builds (rust-analyzer
    /// keeps proc-macro servers running), so entries are never trusted by path alone.
    static PACKAGE_READS: core::cell::RefCell<PackageReadCache> =
        core::cell::RefCell::new(std::collections::HashMap::new());
}

/// Reads and deserializes a compiled Miden package, reusing the previous read of an unchanged
/// file.
pub(crate) fn read_package(package_path: &Path) -> Result<Arc<Package>, Error> {
    let fingerprint = fs::metadata(package_path)
        .and_then(|metadata| Ok((metadata.modified()?, metadata.len())))
        .ok();
    if let Some(fingerprint) = fingerprint
        && let Some(package) = PACKAGE_READS.with(|reads| {
            reads.borrow().get(package_path).and_then(|(mtime, len, package)| {
                ((*mtime, *len) == fingerprint).then(|| package.clone())
            })
        })
    {
        return Ok(package);
    }

    let error_span = Span::call_site();
    let package_bytes = fs::read(package_path).map_err(|err| {
        Error::new(
            error_span,
            format!("failed to read dependency package '{}': {err}", package_path.display()),
        )
    })?;
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
    if let Some((mtime, len)) = fingerprint {
        PACKAGE_READS.with(|reads| {
            reads
                .borrow_mut()
                .insert(package_path.to_path_buf(), (mtime, len, package.clone()));
        });
    }
    Ok(package)
}

/// Extracts the component WIT embedded in a compiled Miden package.
///
/// Returns `Ok(None)` when the package has no WIT section; a section that is present but not
/// valid UTF-8 is an error (the package claims its own WIT, so nothing may substitute it).
fn package_wit(package: &Package, package_path: &Path) -> Result<Option<String>, Error> {
    let error_span = Span::call_site();
    let wit_section_id = wit_section_id();
    let Some(section) = package.sections.iter().find(|section| section.id == wit_section_id) else {
        return Ok(None);
    };

    String::from_utf8(section.data.to_vec()).map(Some).map_err(|err| {
        Error::new(
            error_span,
            format!(
                "dependency package '{}' contains an invalid component WIT section (not UTF-8): \
                 {err}",
                package_path.display()
            ),
        )
    })
}

/// A located and deserialized dependency package.
pub(crate) struct ResolvedDependencyPackage {
    /// Path of the `.masp` file the package was read from.
    pub(crate) path: PathBuf,
    /// The deserialized package.
    pub(crate) package: Arc<Package>,
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
/// trying the hyphen/underscore stem spellings the cache writers use. The cache is fingerprinted
/// by the build inputs and rewritten by every build, so the package found under the dependency's
/// name is trusted as-is; id, version, and digest verification belong to the compiler's project
/// resolution. Without a configured cache the dependency cannot be resolved at all.
pub(crate) fn resolve_dependency_package(
    name: &str,
    root: &Path,
) -> Result<ResolvedDependencyPackage, Error> {
    if root.is_file() {
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
        let candidate = filesystem_cache_dir.join(format!("{stem}.{}", Package::EXTENSION));
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
    env::var_os("MIDENC_PACKAGE_CACHE").map(PathBuf::from)
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
         MIDENC_PACKAGE_CACHE directory '{}'. The cache is populated by a midenc-driven build \
         (`cargo miden build`), and by the contract `build.rs` for plain cargo builds; rebuild \
         through either so the dependency package is available during macro expansion.",
        root.display(),
        filesystem_cache_dir.display(),
    )
}

/// Formats the diagnostic for an expansion without a configured package cache.
fn missing_package_cache_message(name: &str, root: &Path) -> String {
    format!(
        "the Miden package cache is not configured (MIDENC_PACKAGE_CACHE is not set), so the \
         compiled package for Miden dependency '{name}' (root '{}') cannot be resolved during \
         Rust macro expansion. Build through `cargo miden build`, which exports the variable to \
         its nested builds, or add the contract `build.rs` from a generated template so plain \
         `cargo build`/`cargo check` and IDE analysis populate and export the cache.",
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
}
