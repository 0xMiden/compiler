//! Miden package discovery for note schema macros.

use std::{
    env, fs,
    path::{Path, PathBuf},
};

use miden_mast_package::Package;

use crate::{Error, NoteStorageSchema, Result};

/// Directory used to exchange Miden packages with nested Cargo builds.
const PACKAGE_CACHE_ENV: &str = "MIDENC_PACKAGE_CACHE";

/// A loaded package artifact and its note storage schema.
pub struct NotePackageArtifact {
    path: PathBuf,
    schema: NoteStorageSchema,
}

impl NotePackageArtifact {
    /// Returns the exact package path used to load the schema.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Returns the schema loaded from the package.
    pub const fn schema(&self) -> &NoteStorageSchema {
        &self.schema
    }
}

/// Resolves package artifacts for one note macro crate.
pub struct NotePackageResolver<'a> {
    macro_crate: &'a str,
}

impl<'a> NotePackageResolver<'a> {
    /// Creates a resolver whose diagnostics name `macro_crate`.
    pub const fn new(macro_crate: &'a str) -> Self {
        Self { macro_crate }
    }

    /// Loads the package built by a Miden project.
    pub fn from_project(&self, project: &str) -> Result<NotePackageArtifact> {
        let project_dir = self.resolve_manifest_path(project)?;
        if !project_dir.is_dir() {
            return Err(Error::new(format!(
                "{}: note project directory '{}' does not exist",
                self.macro_crate,
                project_dir.display()
            )));
        }
        let package_path = resolve_project_package(&project_dir)
            .map_err(|error| Error::new(format!("{}: {error}", self.macro_crate)))?
            .ok_or_else(|| {
                Error::new(missing_project_package_message(self.macro_crate, &project_dir))
            })?;
        self.load_package(package_path)
    }

    /// Loads one exact Miden package path.
    pub fn from_package(&self, package: &str) -> Result<NotePackageArtifact> {
        let package_path = self.resolve_manifest_path(package)?;
        if !package_path.is_file() {
            return Err(Error::new(format!(
                "{}: Miden package '{}' does not exist",
                self.macro_crate,
                package_path.display()
            )));
        }
        self.load_package(package_path)
    }

    /// Resolves one path relative to the consuming crate manifest.
    fn resolve_manifest_path(&self, value: &str) -> Result<PathBuf> {
        let path = PathBuf::from(value);
        if path.is_absolute() {
            return Ok(path);
        }
        let manifest_dir = env::var_os("CARGO_MANIFEST_DIR").ok_or_else(|| {
            Error::new(format!(
                "{}: CARGO_MANIFEST_DIR is not set during macro expansion",
                self.macro_crate
            ))
        })?;
        Ok(PathBuf::from(manifest_dir).join(path))
    }

    /// Loads the package and its unique schema section.
    fn load_package(&self, path: PathBuf) -> Result<NotePackageArtifact> {
        let path = path.canonicalize().map_err(|error| {
            Error::new(format!(
                "{}: failed to resolve Miden package '{}': {error}",
                self.macro_crate,
                path.display()
            ))
        })?;
        let package = Package::deserialize_from_file(&path).map_err(|error| {
            Error::new(format!(
                "{}: failed to read Miden package '{}': {error}",
                self.macro_crate,
                path.display()
            ))
        })?;
        let schema = NoteStorageSchema::from_package(&package).map_err(|error| {
            Error::new(format!(
                "{}: failed to read note storage schema from '{}': {error}",
                self.macro_crate,
                path.display()
            ))
        })?;
        Ok(NotePackageArtifact { path, schema })
    }
}

/// Resolves one project package by package identity and output-directory priority.
fn resolve_project_package(project_dir: &Path) -> Result<Option<PathBuf>> {
    let stems = project_package_stems(project_dir);

    if let Some(cache_dir) = env::var_os(PACKAGE_CACHE_ENV) {
        return find_project_package_in_dir(&absolutize(PathBuf::from(cache_dir))?, &stems);
    }

    let profiles = candidate_profiles();
    for output_dir in project_output_dirs(project_dir, &profiles) {
        if let Some(package) = find_project_package_in_dir(&output_dir, &stems)? {
            return Ok(Some(package));
        }
    }
    Ok(None)
}

/// Returns Cargo and Miden profile names in lookup order.
fn candidate_profiles() -> Vec<String> {
    let mut profiles = Vec::new();
    if let Ok(profile) = env::var("PROFILE") {
        push_profile(&mut profiles, profile);
    }
    push_profile(&mut profiles, "release".to_owned());
    push_profile(&mut profiles, "debug".to_owned());
    push_profile(&mut profiles, "dev".to_owned());
    profiles
}

/// Returns candidate output directories for one project.
fn project_output_dirs(project_dir: &Path, profiles: &[String]) -> Vec<PathBuf> {
    let mut dirs = Vec::new();

    // Keep this policy in parity with dependency_output_dirs in base-macros/src/fpi.rs. The
    // crates cannot share the implementation without adding the full SDK macro dependency graph.
    push_profile_dirs(&mut dirs, project_dir.join("target"), profiles);
    push_manifest_ancestor_target_profile_dirs(&mut dirs, project_dir, profiles);
    push_ancestor_target_profile_dirs(&mut dirs, project_dir, profiles);

    if let Some(target_dir) = env::var_os("CARGO_TARGET_DIR") {
        push_profile_dirs(&mut dirs, PathBuf::from(target_dir), profiles);
    }
    if let Some(out_dir) = env::var_os("OUT_DIR") {
        for ancestor in Path::new(&out_dir).ancestors() {
            push_profile_dirs(&mut dirs, ancestor.to_path_buf(), profiles);
        }
    }
    if let Ok(current_dir) = env::current_dir() {
        push_profile_dirs(&mut dirs, current_dir.join("target"), profiles);
        push_manifest_ancestor_target_profile_dirs(&mut dirs, &current_dir, profiles);
        push_ancestor_target_profile_dirs(&mut dirs, &current_dir, profiles);
    }
    dirs
}

/// Adds `target/miden/<profile>` directories while preserving order.
fn push_profile_dirs(dirs: &mut Vec<PathBuf>, target_root: PathBuf, profiles: &[String]) {
    for profile in profiles {
        let dir = target_root.join("miden").join(profile);
        if !dirs.contains(&dir) {
            dirs.push(dir);
        }
    }
}

/// Adds target directories found among the ancestors of `path`.
fn push_ancestor_target_profile_dirs(dirs: &mut Vec<PathBuf>, path: &Path, profiles: &[String]) {
    for ancestor in path.ancestors() {
        if ancestor.file_name().is_some_and(|name| name == "target") {
            push_profile_dirs(dirs, ancestor.to_path_buf(), profiles);
        }
    }
}

/// Adds target directories for Cargo manifest ancestors of `path`.
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

/// Finds a package in one output directory by ordered package identity.
fn find_project_package_in_dir(dir: &Path, stems: &[String]) -> Result<Option<PathBuf>> {
    if !dir.is_dir() {
        return Ok(None);
    }

    let mut packages = fs::read_dir(dir)
        .map_err(|error| Error::new(format!("failed to read '{}': {error}", dir.display())))?
        .collect::<core::result::Result<Vec<_>, _>>()
        .map_err(|error| {
            Error::new(format!("failed to read an entry in '{}': {error}", dir.display()))
        })?
        .into_iter()
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_file() && path.extension().is_some_and(|extension| extension == "masp")
        })
        .collect::<Vec<_>>();
    packages.sort();

    // The stems are ordered by identity. The canonical miden-project package name comes first,
    // followed by legacy aliases. This matches base-macros/src/fpi.rs and prevents a newer legacy
    // artifact from shadowing the canonical package.
    for stem in stems {
        if let Some(package) = packages
            .iter()
            .find(|path| path.file_stem().and_then(|value| value.to_str()) == Some(stem.as_str()))
        {
            return Ok(Some(package.clone()));
        }
    }
    Ok(None)
}

/// Returns ordered package filename stems for one project.
fn project_package_stems(project_dir: &Path) -> Vec<String> {
    let mut stems = Vec::new();
    if let Some(name) = package_name_from_manifest(&project_dir.join("miden-project.toml")) {
        push_package_stem(&mut stems, &name);
    }
    if let Some(name) = package_name_from_manifest(&project_dir.join("Cargo.toml")) {
        push_package_stem(&mut stems, &name);
    }
    if let Some(name) = project_dir.file_name().and_then(|name| name.to_str()) {
        push_package_stem(&mut stems, name);
    }
    stems
}

/// Reads a package name from one TOML manifest.
fn package_name_from_manifest(manifest_path: &Path) -> Option<String> {
    let manifest = fs::read_to_string(manifest_path).ok()?;
    let manifest = manifest.parse::<toml::Table>().ok()?;
    manifest
        .get("package")
        .and_then(toml::Value::as_table)
        .and_then(|package| package.get("name"))
        .and_then(toml::Value::as_str)
        .map(ToOwned::to_owned)
}

/// Adds a package filename stem and its underscore alias.
fn push_package_stem(stems: &mut Vec<String>, name: &str) {
    if !name.is_empty() && !stems.iter().any(|existing| existing == name) {
        stems.push(name.to_owned());
    }
    let normalized = name.replace('-', "_");
    if !normalized.is_empty() && !stems.contains(&normalized) {
        stems.push(normalized);
    }
}

/// Makes a cache path absolute for generated `include_bytes!` inputs.
fn absolutize(path: PathBuf) -> Result<PathBuf> {
    if path.is_absolute() {
        return Ok(path);
    }
    Ok(env::current_dir()
        .map_err(|error| Error::new(format!("failed to resolve current directory: {error}")))?
        .join(path))
}

/// Adds one profile name once.
fn push_profile(profiles: &mut Vec<String>, profile: String) {
    if !profile.is_empty() && !profiles.contains(&profile) {
        profiles.push(profile);
    }
}

/// Formats the diagnostic for a project without a built package.
fn missing_project_package_message(macro_crate: &str, project_dir: &Path) -> String {
    let manifest = project_dir.join("Cargo.toml");
    let build = if manifest.is_file() {
        format!("cargo miden build --manifest-path {} --release", manifest.display())
    } else {
        "cargo miden build --release".to_owned()
    };
    format!(
        "{macro_crate} could not find a built `.masp` package under '{}'. Build the note project \
         first with `{build}`.",
        project_dir.join("target/miden/<profile>").display()
    )
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::{
        find_project_package_in_dir, missing_project_package_message, project_output_dirs,
        project_package_stems,
    };

    #[test]
    fn canonical_project_identity_wins_over_legacy_aliases() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(
            temp.path().join("miden-project.toml"),
            "[package]\nname='canonical-note'\nversion='0.1.0'",
        )
        .unwrap();
        fs::write(temp.path().join("Cargo.toml"), "[package]\nname='legacy-note'\nversion='0.1.0'")
            .unwrap();
        let output = temp.path().join("output");
        fs::create_dir(&output).unwrap();
        fs::write(output.join("legacy-note.masp"), b"newer legacy package").unwrap();
        fs::write(output.join("canonical-note.masp"), b"canonical package").unwrap();

        let stems = project_package_stems(temp.path());
        let selected = find_project_package_in_dir(&output, &stems).unwrap().unwrap();
        assert_eq!(selected, output.join("canonical-note.masp"));
    }

    #[test]
    fn manifest_ancestor_target_directories_are_candidates() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(temp.path().join("Cargo.lock"), "").unwrap();
        let project = temp.path().join("crates/note");
        fs::create_dir_all(&project).unwrap();
        let profiles = vec!["release".to_owned()];

        let dirs = project_output_dirs(&project, &profiles);

        assert!(dirs.contains(&temp.path().join("target/miden/release")));
    }

    #[test]
    fn missing_package_diagnostic_names_macro_and_build_command() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(temp.path().join("Cargo.toml"), "[package]\nname='note'\nversion='0.1.0'")
            .unwrap();
        let message = missing_project_package_message("test-note-macro", temp.path());
        assert!(message.contains("test-note-macro"));
        assert!(message.contains("cargo miden build --manifest-path"));
        assert!(message.contains("--release"));
    }
}
