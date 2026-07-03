//! Locating and reading compiled Miden dependency packages (`.masp`) at macro-expansion time.
//!
//! A Miden path dependency is consumed through its compiled package: the `.masp` carries both the
//! dependency's embedded component WIT (read here) and its procedure roots (read by [`crate::fpi`]).

use std::{
    env, fs,
    path::{Path, PathBuf},
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
    /// The component WIT source embedded in the package.
    pub(crate) wit: String,
}

/// Reads the embedded WIT of every Miden path dependency's compiled package.
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
                let package_path =
                    resolve_dependency_package_path(dependency.name().as_ref(), &dependency_root)?;
                let wit = read_package_wit(&package_path)?;
                sources.push(DependencyWitSource {
                    name: dependency.name().to_string(),
                    root: dependency_root,
                    package_path,
                    wit,
                });
            }
            // TODO(pauls): We should also handle git dependencies at some point
            _ => continue,
        }
    }

    Ok(sources)
}

/// Returns the package section id carrying the embedded component WIT.
pub(crate) fn wit_section_id() -> SectionId {
    SectionId::custom(PACKAGE_WIT_SECTION_ID)
        .expect("the WIT section id must be a valid custom section id")
}

/// Reads the component WIT embedded in a compiled Miden package.
pub(crate) fn read_package_wit(package_path: &Path) -> Result<String, Error> {
    let error_span = Span::call_site();
    let package_bytes = fs::read(package_path).map_err(|err| {
        Error::new(
            error_span,
            format!("failed to read dependency package '{}': {err}", package_path.display()),
        )
    })?;
    let package = Package::read_from_bytes(&package_bytes).map_err(|err| {
        Error::new(
            error_span,
            format!("failed to deserialize dependency package '{}': {err}", package_path.display()),
        )
    })?;

    let wit_section_id = wit_section_id();
    let Some(section) = package.sections.iter().find(|section| section.id == wit_section_id) else {
        return Err(Error::new(
            error_span,
            format!(
                "dependency package '{}' does not embed component WIT (missing package section \
                 '{PACKAGE_WIT_SECTION_ID}'); it was likely built with an older Miden toolchain. \
                 Rebuild the dependency with the current `cargo miden build`. For manually \
                 authored components (a hand-written `wit/` directory with a bare \
                 `miden::generate!()`), the WIT is embedded only when the `wit/` directory \
                 contains exactly one `.wit` file.",
                package_path.display()
            ),
        ));
    };

    String::from_utf8(section.data.to_vec()).map_err(|err| {
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

/// Finds the `.masp` package artifact for the dependency named `name` rooted at `root`.
pub(crate) fn resolve_dependency_package_path(name: &str, root: &Path) -> Result<PathBuf, Error> {
    if root.is_file() {
        return Ok(root.to_path_buf());
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

    // Prefer the freshest name-matched package among the dependency's own output directories:
    // `PROFILE` is never set for proc macros, so profile order alone would let a stale debug
    // package shadow a fresh release build.
    let own_matches = output_dirs
        .own
        .iter()
        .filter_map(|dir| find_stem_match_in_dir(dir, &package_stems).transpose())
        .collect::<Result<Vec<_>, _>>()?;
    if let Some(package) = latest_modified(own_matches) {
        return Ok(package);
    }

    // A solitary `.masp` is accepted only in the dependency's own directories, where it cannot
    // belong to anything else; an ambient directory may hold an unrelated package.
    for dir in &output_dirs.own {
        if let Some(package) = find_solitary_package_in_dir(dir)? {
            return Ok(package);
        }
    }

    for dir in &output_dirs.ambient {
        if let Some(package) = find_stem_match_in_dir(dir, &package_stems)? {
            return Ok(package);
        }
    }

    Err(Error::new(
        Span::call_site(),
        missing_dependency_package_message(name, root, &package_stems, &output_dirs, &profiles),
    ))
}

/// Returns the most recently modified of the given package paths.
///
/// Ties (including unreadable timestamps) resolve to the earliest candidate, i.e. the most
/// precise search directory.
fn latest_modified(packages: Vec<PathBuf>) -> Option<PathBuf> {
    packages
        .into_iter()
        .rev()
        .max_by_key(|path| fs::metadata(path).and_then(|metadata| metadata.modified()).ok())
}

/// Formats the diagnostic emitted when a dependency's compiled package cannot be located.
fn missing_dependency_package_message(
    name: &str,
    root: &Path,
    package_stems: &[String],
    output_dirs: &DependencyOutputDirs,
    profiles: &[String],
) -> String {
    let searched = output_dirs
        .own
        .iter()
        .chain(output_dirs.ambient.iter())
        .map(|dir| format!("'{}'", dir.display()))
        .collect::<Vec<_>>()
        .join(", ");
    let expected_files = package_stems
        .iter()
        .flat_map(|stem| profiles.iter().map(move |profile| format!("{stem}.masp in {profile}")))
        .collect::<Vec<_>>()
        .join(", ");
    let build_hint = dependency_build_hint(root);

    format!(
        "could not find a built `.masp` package for Miden dependency '{name}' (root '{}'). The \
         SDK macros need the dependency package during Rust macro expansion to read its embedded \
         WIT and procedure roots. Expected one of: {expected_files}. Searched: {searched}. \
         {build_hint}",
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
    /// Directories derived from the dependency's own root; a package found here belongs to it.
    own: Vec<PathBuf>,
    /// Ambient directories (`CARGO_TARGET_DIR`, `OUT_DIR`, cwd targets) that may also hold
    /// packages of unrelated projects.
    ambient: Vec<PathBuf>,
}

/// Returns candidate output directories where a dependency `.masp` may have been written.
fn dependency_output_dirs(root: &Path, profiles: &[String]) -> DependencyOutputDirs {
    // The dependency root is the most precise location for path dependencies. Prefer it over
    // ambient target directories so restored or previously built artifacts cannot shadow the
    // package that belongs to the dependency being wrapped.
    let mut own = Vec::new();
    push_profile_dirs(&mut own, root.join("target"), profiles);
    push_manifest_ancestor_target_profile_dirs(&mut own, root, profiles);
    push_ancestor_target_profile_dirs(&mut own, root, profiles);

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

    DependencyOutputDirs { own, ambient }
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

/// Finds a package in `dir` whose filename matches one of the dependency's name stems.
fn find_stem_match_in_dir(dir: &Path, package_stems: &[String]) -> Result<Option<PathBuf>, Error> {
    let packages = packages_in_dir(dir)?;
    for stem in package_stems {
        if let Some(package) = packages.iter().find(|path| {
            path.file_stem()
                .and_then(|value| value.to_str())
                .is_some_and(|file_stem| file_stem == stem)
        }) {
            return Ok(Some(package.clone()));
        }
    }
    Ok(None)
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
        let temp_root = env::temp_dir()
            .join(format!("midenc-fpi-dependency-output-dirs-{}", std::process::id()));
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

    #[test]
    fn prefers_the_freshest_stem_match_across_profile_dirs() {
        let temp_root =
            env::temp_dir().join(format!("midenc-dep-package-freshest-{}", std::process::id()));
        let debug_dir = temp_root.join("target/miden/debug");
        let release_dir = temp_root.join("target/miden/release");
        std::fs::create_dir_all(&debug_dir).unwrap();
        std::fs::create_dir_all(&release_dir).unwrap();
        let debug_path = debug_dir.join("dep_fixture.masp");
        let release_path = release_dir.join("dep_fixture.masp");
        std::fs::write(&debug_path, b"stale").unwrap();
        std::fs::write(&release_path, b"fresh").unwrap();
        // `PROFILE` is unset for proc macros, so the debug dir is searched first; only the
        // freshest-match rule makes the newer release artifact win.
        let stale = std::time::SystemTime::now() - std::time::Duration::from_secs(600);
        std::fs::File::options()
            .write(true)
            .open(&debug_path)
            .unwrap()
            .set_modified(stale)
            .unwrap();

        let resolved = resolve_dependency_package_path("dep-fixture", &temp_root).unwrap();

        assert_eq!(resolved, release_path);

        std::fs::remove_dir_all(temp_root).unwrap();
    }

    #[test]
    fn accepts_a_solitary_package_in_the_dependency_own_dirs() {
        let temp_root =
            env::temp_dir().join(format!("midenc-dep-package-solitary-{}", std::process::id()));
        let debug_dir = temp_root.join("target/miden/debug");
        std::fs::create_dir_all(&debug_dir).unwrap();
        let package_path = debug_dir.join("oddly_named.masp");
        std::fs::write(&package_path, b"package bytes").unwrap();

        let resolved = resolve_dependency_package_path("dep-fixture", &temp_root).unwrap();

        assert_eq!(resolved, package_path);

        std::fs::remove_dir_all(temp_root).unwrap();
    }

    #[test]
    fn missing_dependency_package_message_explains_macro_time_requirement() {
        let temp_root =
            env::temp_dir().join(format!("midenc-fpi-missing-package-{}", std::process::id()));
        std::fs::create_dir_all(&temp_root).unwrap();
        std::fs::write(temp_root.join("Cargo.toml"), "[package]\nname = \"counter\"\n").unwrap();

        let profiles = vec!["release".to_string(), "debug".to_string()];
        let stems = vec!["counter".to_string(), "counter_component".to_string()];
        let output_dirs = DependencyOutputDirs {
            own: vec![temp_root.join("target/miden/release"), temp_root.join("target/miden/debug")],
            ambient: Vec::new(),
        };

        let message = missing_dependency_package_message(
            "counter",
            &temp_root,
            &stems,
            &output_dirs,
            &profiles,
        );

        assert!(message.contains("could not find a built `.masp` package"));
        assert!(message.contains("during Rust macro expansion"));
        assert!(message.contains("embedded WIT and procedure roots"));
        assert!(message.contains("counter.masp in release"));
        assert!(message.contains("counter_component.masp in debug"));
        assert!(message.contains("cargo miden build --manifest-path"));
        assert!(message.contains(&temp_root.display().to_string()));

        std::fs::remove_dir_all(temp_root).unwrap();
    }
}
