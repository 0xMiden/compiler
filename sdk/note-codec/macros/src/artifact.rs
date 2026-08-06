//! Miden package artifact resolution for codec macros.

use std::{
    env, fs,
    path::{Path, PathBuf},
};

use proc_macro2::Span;

/// Resolves a macro path relative to the consuming crate manifest.
pub(crate) fn resolve_manifest_path(value: &str, span: Span) -> syn::Result<PathBuf> {
    let path = PathBuf::from(value);
    if path.is_absolute() {
        return Ok(path);
    }
    let manifest_dir = env::var_os("CARGO_MANIFEST_DIR").ok_or_else(|| {
        syn::Error::new(span, "CARGO_MANIFEST_DIR is not set during note codec generation")
    })?;
    Ok(PathBuf::from(manifest_dir).join(path))
}

/// Returns the newest package directly inside any project Miden profile directory.
pub(crate) fn freshest_project_package(
    project_dir: &Path,
    span: Span,
) -> syn::Result<Option<PathBuf>> {
    let target_dir = project_dir.join("target/miden");
    if !target_dir.is_dir() {
        return Ok(None);
    }

    let mut candidates = Vec::new();
    for profile_dir in candidate_profile_dirs(&target_dir, span)? {
        let entries = fs::read_dir(&profile_dir).map_err(|error| {
            syn::Error::new(span, format!("failed to read '{}': {error}", profile_dir.display()))
        })?;
        for entry in entries {
            let entry = entry.map_err(|error| {
                syn::Error::new(
                    span,
                    format!("failed to read an entry in '{}': {error}", profile_dir.display()),
                )
            })?;
            let path = entry.path();
            if !path.is_file() || !path.extension().is_some_and(|extension| extension == "masp") {
                continue;
            }
            let modified =
                entry.metadata().and_then(|metadata| metadata.modified()).map_err(|error| {
                    syn::Error::new(
                        span,
                        format!(
                            "failed to read modification time for '{}': {error}",
                            path.display()
                        ),
                    )
                })?;
            candidates.push((modified, path));
        }
    }
    candidates.sort_by(|(left_time, left_path), (right_time, right_path)| {
        left_time.cmp(right_time).then_with(|| left_path.cmp(right_path))
    });
    Ok(candidates.pop().map(|(_, path)| path))
}

/// Returns project profile directories without duplicates.
fn candidate_profile_dirs(target_dir: &Path, span: Span) -> syn::Result<Vec<PathBuf>> {
    let mut profiles = Vec::new();
    if let Ok(profile) = env::var("PROFILE") {
        push_profile(&mut profiles, profile);
    }
    push_profile(&mut profiles, "release".to_owned());
    push_profile(&mut profiles, "debug".to_owned());

    let entries = fs::read_dir(target_dir).map_err(|error| {
        syn::Error::new(span, format!("failed to read '{}': {error}", target_dir.display()))
    })?;
    let mut discovered = entries
        .filter_map(Result::ok)
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

/// Adds a profile name once.
fn push_profile(profiles: &mut Vec<String>, profile: String) {
    if !profile.is_empty() && !profiles.contains(&profile) {
        profiles.push(profile);
    }
}

/// Formats the missing-project-package diagnostic.
pub(crate) fn missing_project_package_message(project_dir: &Path) -> String {
    let manifest = project_dir.join("Cargo.toml");
    let build = if manifest.is_file() {
        format!("cargo miden build --manifest-path {} --release", manifest.display())
    } else {
        "cargo miden build --release".to_owned()
    };
    format!(
        "miden-note-codec could not find a built `.masp` package under '{}'. Build the note \
         project first with `{build}`.",
        project_dir.join("target/miden/<profile>").display()
    )
}
