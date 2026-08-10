//! Miden package discovery for note schema macros.

use std::{
    env, fs,
    path::{Path, PathBuf},
};

use miden_mast_package::Package;

use crate::{Error, NoteStorageSchema, Result};

/// Compiler-provided path to the package staged for note codec generation.
const NOTE_CODEC_PACKAGE_PATH_ENV: &str = "MIDENC_NOTE_CODEC_PACKAGE_PATH";

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

    /// Loads the freshest package built by a Miden project.
    pub fn from_project(&self, project: &str) -> Result<NotePackageArtifact> {
        if let Some(path) = self.compiler_staged_package()? {
            return self.load_package(path);
        }

        let project_dir = self.resolve_manifest_path(project)?;
        if !project_dir.is_dir() {
            return Err(Error::new(format!(
                "{}: note project directory '{}' does not exist",
                self.macro_crate,
                project_dir.display()
            )));
        }
        let package_path = freshest_project_package(&project_dir)
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

    /// Returns the compiler-staged package when one is available.
    fn compiler_staged_package(&self) -> Result<Option<PathBuf>> {
        let Some(path) = env::var_os(NOTE_CODEC_PACKAGE_PATH_ENV) else {
            return Ok(None);
        };
        let path = PathBuf::from(path);
        if !path.is_file() {
            return Err(Error::new(format!(
                "{}: {NOTE_CODEC_PACKAGE_PATH_ENV} points to missing Miden package '{}'",
                self.macro_crate,
                path.display()
            )));
        }
        Ok(Some(path))
    }

    /// Loads the package and its unique schema section.
    fn load_package(&self, path: PathBuf) -> Result<NotePackageArtifact> {
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

/// Returns the newest package directly inside a Miden project profile directory.
fn freshest_project_package(project_dir: &Path) -> Result<Option<PathBuf>> {
    let target_dir = project_dir.join("target/miden");
    if !target_dir.is_dir() {
        return Ok(None);
    }

    let mut candidates = Vec::new();
    for profile_dir in candidate_profile_dirs(&target_dir)? {
        let entries = fs::read_dir(&profile_dir).map_err(|error| {
            Error::new(format!("failed to read '{}': {error}", profile_dir.display()))
        })?;
        for entry in entries {
            let entry = entry.map_err(|error| {
                Error::new(format!(
                    "failed to read an entry in '{}': {error}",
                    profile_dir.display()
                ))
            })?;
            let path = entry.path();
            if !path.is_file() || !path.extension().is_some_and(|extension| extension == "masp") {
                continue;
            }
            let modified =
                entry.metadata().and_then(|metadata| metadata.modified()).map_err(|error| {
                    Error::new(format!(
                        "failed to read modification time for '{}': {error}",
                        path.display()
                    ))
                })?;
            candidates.push((modified, path));
        }
    }
    candidates.sort_by(|(left_time, left_path), (right_time, right_path)| {
        left_time.cmp(right_time).then_with(|| left_path.cmp(right_path))
    });
    Ok(candidates.pop().map(|(_, path)| path))
}

/// Returns project profile directories in deterministic candidate order.
fn candidate_profile_dirs(target_dir: &Path) -> Result<Vec<PathBuf>> {
    let mut profiles = Vec::new();
    push_profile(&mut profiles, "release".to_owned());
    push_profile(&mut profiles, "debug".to_owned());

    let entries = fs::read_dir(target_dir).map_err(|error| {
        Error::new(format!("failed to read '{}': {error}", target_dir.display()))
    })?;
    let mut discovered = entries
        .filter_map(core::result::Result::ok)
        .filter(|entry| entry.path().is_dir())
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| name != "packages" && name != "generated-wit")
        .collect::<Vec<_>>();
    discovered.sort();
    for profile in discovered {
        push_profile(&mut profiles, profile);
    }

    Ok(profiles
        .into_iter()
        .map(|profile| target_dir.join(profile))
        .filter(|path| path.is_dir())
        .collect())
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
    use std::{fs, thread, time::Duration};

    use super::{freshest_project_package, missing_project_package_message};

    #[test]
    fn selects_the_freshest_package_across_profiles() {
        let temp = tempfile::tempdir().unwrap();
        let debug = temp.path().join("target/miden/debug");
        let release = temp.path().join("target/miden/release");
        fs::create_dir_all(&debug).unwrap();
        fs::create_dir_all(&release).unwrap();
        fs::write(debug.join("note.masp"), b"old").unwrap();
        thread::sleep(Duration::from_millis(20));
        fs::write(release.join("note.masp"), b"new").unwrap();

        let selected = freshest_project_package(temp.path()).unwrap().unwrap();
        assert_eq!(selected, release.join("note.masp"));
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
