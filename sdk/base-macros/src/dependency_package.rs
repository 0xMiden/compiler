//! Locating and reading compiled Miden dependency packages (`.masp`) at macro-expansion time.
//!
//! A Miden path dependency is consumed through its compiled package: the `.masp` carries both the
//! dependency's embedded component WIT (read here) and its procedure roots (read by [`crate::fpi`]).

use std::{
    env, fs,
    path::{Path, PathBuf},
    sync::Arc,
};

use miden_mast_package::{Package, SectionId};
use miden_protocol::utils::serde::Deserializable;
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
    /// the exact read the package id was verified against.
    pub(crate) package: Arc<Package>,
    /// The component WIT source: embedded in the package, or supplied by the dependency's `wit`
    /// manifest key when the package embeds none.
    pub(crate) wit: String,
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
            miden_project::DependencyVersionScheme::Path { path, version } => {
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
                let resolved = resolve_dependency_package(
                    dependency.name().as_ref(),
                    &dependency_root,
                    version.as_ref(),
                )?;
                let wit_override = dependency_wit_override(package, dependency.name().as_ref())?;
                let wit = match (package_wit(&resolved.package, &resolved.path)?, wit_override) {
                    (Some(_), Some(_)) => {
                        return Err(Error::new(
                            error_span,
                            format!(
                                "dependency '{}': package '{}' embeds component WIT, but \
                                 miden-project.toml also sets \
                                 package.metadata.miden.dependencies.{}.wit; remove the `wit` key \
                                 — embedded WIT is authoritative",
                                dependency.name(),
                                resolved.path.display(),
                                dependency.name(),
                            ),
                        ));
                    }
                    (Some(wit), None) => wit,
                    (None, Some(wit_override)) => {
                        read_wit_override(&wit_override, manifest_dir, dependency.name().as_ref())?
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
                });
            }
            // Registry dependencies are MASM base libraries (`miden-core`, `miden-protocol`)
            // consumed at link time only, so they carry no component WIT. Git and workspace
            // schemes are not yet supported at macro expansion time (TODO(pauls)).
            _ => continue,
        }
    }

    Ok(sources)
}

/// Returns the raw WIT override path from `package.metadata.miden.dependencies.<name>.wit`.
fn dependency_wit_override(
    package: &miden_project::Package,
    dependency_name: &str,
) -> Result<Option<String>, Error> {
    let Some(wit_value) = package
        .metadata()
        .get("miden")
        .and_then(|meta| meta.get("dependencies"))
        .and_then(|value| value.as_table())
        .and_then(|dependencies| dependencies.get(dependency_name))
        .and_then(|config| config.as_table())
        .and_then(|config| config.get("wit"))
    else {
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
/// exactly one top-level `.wit` file.
///
/// The override is validated like embedded WIT: it must resolve against the bundled SDK WIT alone
/// and export an interface, so every macro flow gets the accurate diagnostic at the source.
fn read_wit_override(
    wit_path: &str,
    manifest_dir: &Path,
    dependency_name: &str,
) -> Result<String, Error> {
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
    Ok(wit)
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

/// Reads and deserializes a compiled Miden package.
pub(crate) fn read_package(package_path: &Path) -> Result<Arc<Package>, Error> {
    let error_span = Span::call_site();
    let package_bytes = fs::read(package_path).map_err(|err| {
        Error::new(
            error_span,
            format!("failed to read dependency package '{}': {err}", package_path.display()),
        )
    })?;
    Package::read_from_bytes(&package_bytes).map(Arc::new).map_err(|err| {
        Error::new(
            error_span,
            format!(
                "failed to deserialize dependency package '{}': {err}. The package may have been \
                 produced by a different Miden toolchain version; rebuild the dependency with the \
                 current `cargo miden build`.",
                package_path.display()
            ),
        )
    })
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

/// A located, deserialized, and identity-checked dependency package.
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
/// Every candidate located by searching is deserialized and accepted only when its package id
/// matches the dependency's name — and, when the manifest pins a version, when its version
/// satisfies the pin — so a renamed, unrelated, or outdated artifact is never adopted. A `root`
/// that is itself a `.masp` file is the manifest's explicit choice and is read without those
/// checks (the manifest key need not equal the prebuilt package's id).
pub(crate) fn resolve_dependency_package(
    name: &str,
    root: &Path,
    version: Option<&miden_project::VersionRequirement>,
) -> Result<ResolvedDependencyPackage, Error> {
    if root.is_file() {
        return Ok(ResolvedDependencyPackage {
            path: root.to_path_buf(),
            package: read_package(root)?,
        });
    }

    let preferred_profile = env::var("PROFILE").unwrap_or_else(|_| "debug".to_string());
    let mut profiles = vec![preferred_profile.clone()];
    if preferred_profile != "release" {
        profiles.push("release".to_string());
    }
    if preferred_profile != "debug" {
        profiles.push("debug".to_string());
    }

    let package_stems = dependency_package_stems(name, root);
    let output_dirs = dependency_output_dirs(root, &profiles);

    // Candidates in preference order. Name matches are ordered freshest-first within each
    // directory class: `PROFILE` is never set for proc macros, so profile order alone would let
    // a stale debug package shadow a fresh release build.
    let mut candidates = Vec::new();

    let mut own_matches = Vec::new();
    for dir in output_dirs.private.iter().chain(output_dirs.shared.iter()) {
        own_matches.extend(stem_matches_in_dir(dir, &package_stems)?);
    }
    candidates.extend(sort_by_freshness(own_matches));

    // A solitary `.masp` is considered only in the dependency's private `<root>/target`
    // directories: a shared workspace or ambient target directory may hold a package of an
    // unrelated project.
    for dir in &output_dirs.private {
        candidates.extend(find_solitary_package_in_dir(dir)?);
    }

    let mut ambient_matches = Vec::new();
    for dir in &output_dirs.ambient {
        ambient_matches.extend(stem_matches_in_dir(dir, &package_stems)?);
    }
    candidates.extend(sort_by_freshness(ambient_matches));

    let mut rejected = Vec::new();
    let mut seen = Vec::new();
    for candidate in candidates {
        if seen.contains(&candidate) {
            continue;
        }
        seen.push(candidate.clone());

        let package = read_package(&candidate)?;
        if !package_id_matches(&package, &package_stems) {
            rejected.push((candidate, format!("package id '{}'", package.name)));
            continue;
        }
        if !package_version_matches(&package, version) {
            rejected.push((
                candidate,
                format!("version {} does not satisfy the manifest requirement", package.version),
            ));
            continue;
        }
        return Ok(ResolvedDependencyPackage {
            path: candidate,
            package,
        });
    }

    Err(Error::new(
        Span::call_site(),
        missing_dependency_package_message(
            name,
            root,
            &package_stems,
            &output_dirs,
            &profiles,
            &rejected,
        ),
    ))
}

/// Returns true when the package's id matches one of the dependency's name stems.
///
/// Hyphens and underscores are interchangeable between manifest keys, Cargo package names, and
/// Miden package ids, so the comparison normalizes them.
fn package_id_matches(package: &Package, package_stems: &[String]) -> bool {
    let package_id = package.name.to_string().replace('-', "_");
    package_stems.iter().any(|stem| stem.replace('-', "_") == package_id)
}

/// Returns true when the package's version satisfies the manifest's version pin, if any.
///
/// Digest pins are enforced by the assembler at link time; checking them here would require
/// computing content digests during macro expansion, so they are accepted as-is.
fn package_version_matches(
    package: &Package,
    version: Option<&miden_project::VersionRequirement>,
) -> bool {
    match version {
        Some(miden_project::VersionRequirement::Semantic(requirement)) => {
            requirement.inner().matches(&package.version)
        }
        Some(miden_project::VersionRequirement::Exact(exact)) => exact.version == package.version,
        Some(miden_project::VersionRequirement::Digest(_)) | None => true,
    }
}

/// Sorts package paths by modification time, freshest first.
///
/// Ties (including unreadable timestamps, which sort last) preserve the input order, i.e. the
/// most precise search directory.
fn sort_by_freshness(packages: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut packages_with_mtime = packages
        .into_iter()
        .map(|path| {
            let mtime = fs::metadata(&path).and_then(|metadata| metadata.modified()).ok();
            (path, mtime)
        })
        .collect::<Vec<_>>();
    packages_with_mtime.sort_by_key(|(_, mtime)| core::cmp::Reverse(*mtime));
    packages_with_mtime.into_iter().map(|(path, _)| path).collect()
}

/// Formats the diagnostic emitted when a dependency's compiled package cannot be located.
fn missing_dependency_package_message(
    name: &str,
    root: &Path,
    package_stems: &[String],
    output_dirs: &DependencyOutputDirs,
    profiles: &[String],
    rejected: &[(PathBuf, String)],
) -> String {
    let searched = output_dirs
        .private
        .iter()
        .chain(output_dirs.shared.iter())
        .chain(output_dirs.ambient.iter())
        .map(|dir| format!("'{}'", dir.display()))
        .collect::<Vec<_>>()
        .join(", ");
    let expected_files = package_stems
        .iter()
        .flat_map(|stem| profiles.iter().map(move |profile| format!("{stem}.masp in {profile}")))
        .collect::<Vec<_>>()
        .join(", ");
    let rejected = if rejected.is_empty() {
        String::new()
    } else {
        let rejected = rejected
            .iter()
            .map(|(path, reason)| format!("'{}' ({reason})", path.display()))
            .collect::<Vec<_>>()
            .join(", ");
        format!(" Rejected candidates that do not match the dependency: {rejected}.")
    };
    let build_hint = dependency_build_hint(root);

    format!(
        "could not find a built `.masp` package for Miden dependency '{name}' (root '{}'). The \
         SDK macros need the dependency package during Rust macro expansion to read its embedded \
         WIT and procedure roots. Expected one of: {expected_files}. Searched: \
         {searched}.{rejected} {build_hint}",
        root.display(),
    )
}

/// Returns a command hint for building a dependency package before expanding dependent macros.
fn dependency_build_hint(root: &Path) -> String {
    let manifest_path = root.join("Cargo.toml");
    if manifest_path.is_file() {
        format!(
            "Build the dependency first with `cargo miden build --manifest-path {} --release`, or \
             persist the compiled package to '{}/target/miden/<profile>' before compiling this \
             crate.",
            manifest_path.display(),
            root.display(),
        )
    } else {
        format!(
            "Build the dependency first with `cargo miden build`, or persist the compiled package \
             to '{}/target/miden/<profile>' before compiling this crate.",
            root.display(),
        )
    }
}

/// Candidate output directories where a dependency `.masp` may have been written.
struct DependencyOutputDirs {
    /// The dependency's own `<root>/target` directories; a package found here belongs to it.
    private: Vec<PathBuf>,
    /// Target directories of the dependency root's ancestors — a surrounding workspace's target
    /// holds packages of all its members, so a package here needs a name/id match.
    shared: Vec<PathBuf>,
    /// Ambient directories (`CARGO_TARGET_DIR`, `OUT_DIR`, cwd targets) that may hold packages
    /// of entirely unrelated projects.
    ambient: Vec<PathBuf>,
}

/// Returns candidate output directories where a dependency `.masp` may have been written.
fn dependency_output_dirs(root: &Path, profiles: &[String]) -> DependencyOutputDirs {
    // The dependency root is the most precise location for path dependencies. Prefer it over
    // shared and ambient target directories so restored or previously built artifacts cannot
    // shadow the package that belongs to the dependency being wrapped.
    let mut private = Vec::new();
    push_profile_dirs(&mut private, root.join("target"), profiles);

    let mut shared = Vec::new();
    push_manifest_ancestor_target_profile_dirs(&mut shared, root, profiles);
    push_ancestor_target_profile_dirs(&mut shared, root, profiles);
    // The ancestor walks start at the root itself, re-discovering the private dirs.
    shared.retain(|dir| !private.contains(dir));

    let mut ambient = Vec::new();
    if let Ok(target_dir) = env::var("CARGO_TARGET_DIR") {
        push_profile_dirs(&mut ambient, PathBuf::from(target_dir), profiles);
    }

    if let Ok(out_dir) = env::var("OUT_DIR") {
        for ancestor in Path::new(&out_dir).ancestors() {
            push_profile_dirs(&mut ambient, ancestor.to_path_buf(), profiles);
        }
    }

    if let Ok(current_dir) = env::current_dir() {
        push_profile_dirs(&mut ambient, current_dir.join("target"), profiles);
        push_manifest_ancestor_target_profile_dirs(&mut ambient, &current_dir, profiles);
        push_ancestor_target_profile_dirs(&mut ambient, &current_dir, profiles);
    }

    DependencyOutputDirs {
        private,
        shared,
        ambient,
    }
}

/// Adds `target/miden/<profile>` directories while preserving insertion order.
fn push_profile_dirs(dirs: &mut Vec<PathBuf>, target_root: PathBuf, profiles: &[String]) {
    for profile in profiles {
        let dir = target_root.join("miden").join(profile);
        if !dirs.iter().any(|existing| existing == &dir) {
            dirs.push(dir);
        }
    }
}

/// Adds `target/miden/<profile>` directories found in ancestors of `path`.
fn push_ancestor_target_profile_dirs(dirs: &mut Vec<PathBuf>, path: &Path, profiles: &[String]) {
    for ancestor in path.ancestors() {
        if ancestor.file_name().is_some_and(|name| name == "target") {
            push_profile_dirs(dirs, ancestor.to_path_buf(), profiles);
        }
    }
}

/// Adds `target/miden/<profile>` directories for Cargo manifest ancestors.
fn push_manifest_ancestor_target_profile_dirs(
    dirs: &mut Vec<PathBuf>,
    path: &Path,
    profiles: &[String],
) {
    for ancestor in path.ancestors() {
        if ancestor.join("Cargo.toml").is_file() || ancestor.join("Cargo.lock").is_file() {
            push_profile_dirs(dirs, ancestor.join("target"), profiles);
        }
    }
}

/// Lists the `.masp` packages in `dir`, sorted by path.
fn packages_in_dir(dir: &Path) -> Result<Vec<PathBuf>, Error> {
    if !dir.is_dir() {
        return Ok(Vec::new());
    }

    let mut packages = fs::read_dir(dir)
        .map_err(|err| {
            Error::new(
                Span::call_site(),
                format!("failed to read dependency output directory '{}': {err}", dir.display()),
            )
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| {
            Error::new(
                Span::call_site(),
                format!("failed to iterate dependency output directory '{}': {err}", dir.display()),
            )
        })?
        .into_iter()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "masp"))
        .collect::<Vec<_>>();
    packages.sort();
    Ok(packages)
}

/// Returns all packages in `dir` whose filename matches one of the dependency's name stems.
fn stem_matches_in_dir(dir: &Path, package_stems: &[String]) -> Result<Vec<PathBuf>, Error> {
    let mut packages = packages_in_dir(dir)?;
    packages.retain(|path| {
        path.file_stem()
            .and_then(|value| value.to_str())
            .is_some_and(|file_stem| package_stems.iter().any(|stem| stem == file_stem))
    });
    Ok(packages)
}

/// Returns the package in `dir` when it is the directory's only one, regardless of its name.
fn find_solitary_package_in_dir(dir: &Path) -> Result<Option<PathBuf>, Error> {
    let mut packages = packages_in_dir(dir)?;
    Ok((packages.len() == 1).then(|| packages.remove(0)))
}

/// Returns likely `.masp` filename stems for a dependency.
fn dependency_package_stems(name: &str, root: &Path) -> Vec<String> {
    let mut stems = Vec::new();

    if let Some(package_name) = dependency_manifest_package_name(root) {
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

/// Reads the Cargo package name for dependency directories.
fn dependency_manifest_package_name(root: &Path) -> Option<String> {
    let manifest_path = root.join("Cargo.toml");
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

    #[test]
    fn dependency_stem_preserves_package_filename_before_legacy_alias() {
        let mut stems = Vec::new();

        push_dependency_stem(&mut stems, "no-arg-account");

        assert_eq!(stems, ["no-arg-account", "no_arg_account"]);
    }

    #[test]
    fn dependency_output_dirs_include_manifest_ancestor_targets() {
        let temp_root = fixture_root("output-dirs");
        let workspace_root = temp_root.join("workspace");
        let dependency_root = workspace_root.join("tests/fixtures/dependency");
        std::fs::create_dir_all(&dependency_root).unwrap();
        std::fs::write(workspace_root.join("Cargo.lock"), "").unwrap();
        std::fs::write(dependency_root.join("Cargo.toml"), "").unwrap();

        let mut dirs = Vec::new();
        push_manifest_ancestor_target_profile_dirs(
            &mut dirs,
            &dependency_root,
            &[String::from("release")],
        );

        assert_eq!(dirs[0], dependency_root.join("target/miden/release"));
        assert!(
            dirs.contains(&workspace_root.join("target/miden/release")),
            "expected workspace target in {dirs:?}"
        );

        std::fs::remove_dir_all(temp_root).unwrap();
    }

    use crate::test_support::write_masp_fixture;

    /// Creates a unique fixture root under the temp dir.
    fn fixture_root(name: &str) -> PathBuf {
        let root =
            env::temp_dir().join(format!("midenc-dep-package-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    /// Backdates a package file so a sibling artifact is strictly fresher.
    fn backdate(path: &Path) {
        let stale = std::time::SystemTime::now() - std::time::Duration::from_secs(600);
        std::fs::File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(stale)
            .unwrap();
    }

    #[test]
    fn prefers_the_freshest_stem_match_across_profile_dirs() {
        let temp_root = fixture_root("freshest");
        let debug_path = temp_root.join("target/miden/debug/dep_fixture.masp");
        let release_path = temp_root.join("target/miden/release/dep_fixture.masp");
        write_masp_fixture(&debug_path, "dep-fixture", None);
        write_masp_fixture(&release_path, "dep-fixture", None);
        // `PROFILE` is unset for proc macros, so the debug dir is searched first; only the
        // freshest-match rule makes the newer release artifact win.
        backdate(&debug_path);

        let resolved = resolve_dependency_package("dep-fixture", &temp_root, None).unwrap();

        assert_eq!(resolved.path, release_path);

        std::fs::remove_dir_all(temp_root).unwrap();
    }

    #[test]
    fn prefers_the_freshest_match_across_stem_aliases_in_one_dir() {
        // The Cargo-name (underscore) and package-id (hyphen) naming schemes have both been used
        // for `.masp` artifacts; a stale artifact under one alias must not shadow a fresh one
        // under the other.
        let temp_root = fixture_root("stem-aliases");
        let stale_path = temp_root.join("target/miden/debug/dep_fixture.masp");
        let fresh_path = temp_root.join("target/miden/debug/dep-fixture.masp");
        write_masp_fixture(&stale_path, "dep-fixture", None);
        write_masp_fixture(&fresh_path, "dep-fixture", None);
        backdate(&stale_path);

        let resolved = resolve_dependency_package("dep-fixture", &temp_root, None).unwrap();

        assert_eq!(resolved.path, fresh_path);

        std::fs::remove_dir_all(temp_root).unwrap();
    }

    #[test]
    fn accepts_a_solitary_package_in_the_dependency_private_dirs() {
        let temp_root = fixture_root("solitary");
        let package_path = temp_root.join("target/miden/debug/oddly_named.masp");
        write_masp_fixture(&package_path, "dep-fixture", None);

        let resolved = resolve_dependency_package("dep-fixture", &temp_root, None).unwrap();

        assert_eq!(resolved.path, package_path);

        std::fs::remove_dir_all(temp_root).unwrap();
    }

    #[test]
    fn rejects_a_solitary_package_with_a_mismatched_id() {
        let temp_root = fixture_root("solitary-mismatch");
        let package_path = temp_root.join("target/miden/debug/oddly_named.masp");
        write_masp_fixture(&package_path, "other-package", None);

        let error = resolve_dependency_package("dep-fixture", &temp_root, None)
            .expect_err("a solitary package with a foreign id must not be adopted");
        let message = error.to_string();

        assert!(
            message.contains("could not find a built `.masp` package"),
            "unexpected error: {message}"
        );
        assert!(message.contains("package id 'other-package'"), "unexpected error: {message}");

        std::fs::remove_dir_all(temp_root).unwrap();
    }

    #[test]
    fn does_not_adopt_a_solitary_package_from_a_shared_workspace_target() {
        // The workspace-level target dir holds packages of all members; a solitary unrelated
        // package there must not be adopted for a member that was never built.
        let temp_root = fixture_root("workspace-solitary");
        let workspace_root = temp_root.join("workspace");
        let dependency_root = workspace_root.join("dep");
        std::fs::create_dir_all(&dependency_root).unwrap();
        std::fs::write(workspace_root.join("Cargo.lock"), "").unwrap();
        write_masp_fixture(
            &workspace_root.join("target/miden/debug/unrelated.masp"),
            "unrelated",
            None,
        );

        let error = resolve_dependency_package("dep-fixture", &dependency_root, None)
            .expect_err("an unrelated solitary package in the workspace target must be ignored");
        let message = error.to_string();

        assert!(
            message.contains("could not find a built `.masp` package"),
            "unexpected error: {message}"
        );

        std::fs::remove_dir_all(temp_root).unwrap();
    }

    #[test]
    fn verifies_a_manifest_version_pin() {
        // The test fixture package carries version 0.1.0.
        let temp_root = fixture_root("version-pin");
        let package_path = temp_root.join("target/miden/debug/dep_fixture.masp");
        write_masp_fixture(&package_path, "dep-fixture", None);

        let satisfied = miden_project::VersionRequirement::Semantic(
            miden_assembly_syntax::debuginfo::Span::unknown("^0.1".parse().unwrap()),
        );
        let resolved =
            resolve_dependency_package("dep-fixture", &temp_root, Some(&satisfied)).unwrap();
        assert_eq!(resolved.path, package_path);

        let unsatisfied = miden_project::VersionRequirement::Semantic(
            miden_assembly_syntax::debuginfo::Span::unknown("^2.0".parse().unwrap()),
        );
        let error = resolve_dependency_package("dep-fixture", &temp_root, Some(&unsatisfied))
            .expect_err("a version outside the manifest pin must reject the candidate");
        let message = error.to_string();

        assert!(
            message.contains("does not satisfy the manifest requirement"),
            "unexpected error: {message}"
        );
        assert!(message.contains("0.1.0"), "unexpected error: {message}");

        std::fs::remove_dir_all(temp_root).unwrap();
    }

    #[test]
    fn corrupt_dependency_package_reports_rebuild_hint() {
        let temp_root = fixture_root("corrupt");
        let package_path = temp_root.join("target/miden/debug/dep_fixture.masp");
        std::fs::create_dir_all(package_path.parent().unwrap()).unwrap();
        std::fs::write(&package_path, b"garbage").unwrap();

        let error = resolve_dependency_package("dep-fixture", &temp_root, None)
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

    #[test]
    fn missing_dependency_package_message_explains_macro_time_requirement() {
        let temp_root = fixture_root("missing-message");
        std::fs::write(temp_root.join("Cargo.toml"), "[package]\nname = \"counter\"\n").unwrap();

        let profiles = vec!["release".to_string(), "debug".to_string()];
        let stems = vec!["counter".to_string(), "counter_component".to_string()];
        let output_dirs = DependencyOutputDirs {
            private: vec![
                temp_root.join("target/miden/release"),
                temp_root.join("target/miden/debug"),
            ],
            shared: Vec::new(),
            ambient: Vec::new(),
        };
        let rejected = vec![(
            temp_root.join("target/miden/debug/stray.masp"),
            "package id 'stray'".to_string(),
        )];

        let message = missing_dependency_package_message(
            "counter",
            &temp_root,
            &stems,
            &output_dirs,
            &profiles,
            &rejected,
        );

        assert!(message.contains("could not find a built `.masp` package"));
        assert!(message.contains("during Rust macro expansion"));
        assert!(message.contains("embedded WIT and procedure roots"));
        assert!(message.contains("counter.masp in release"));
        assert!(message.contains("counter_component.masp in debug"));
        assert!(message.contains("package id 'stray'"));
        assert!(message.contains("cargo miden build --manifest-path"));
        assert!(message.contains(&temp_root.display().to_string()));

        std::fs::remove_dir_all(temp_root).unwrap();
    }
}
