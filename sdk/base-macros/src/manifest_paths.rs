//! Utilities for resolving WIT paths from Cargo.toml manifest metadata.

use std::{
    env, fs,
    io::ErrorKind,
    path::{Path, PathBuf},
};

use proc_macro2::Span;
use syn::Error;
use toml::Value;

use crate::util::{bundled_wit_folder, strip_line_comment};

/// File name for the embedded Miden SDK WIT.
const SDK_WIT_FILE_NAME: &str = "miden.wit";

/// Embedded Miden SDK WIT source.
pub(crate) const SDK_WIT_SOURCE: &str = include_str!("../wit/miden.wit");

/// WIT metadata extracted from the consuming crate.
pub(crate) struct ResolvedWit {
    pub paths: Vec<String>,
    pub world: Option<String>,
}

#[derive(Default)]
pub(crate) struct ResolveOptions {
    pub allow_missing_local_wit: bool,
}

/// Collects WIT search paths and the target world from `Cargo.toml` + local files.
pub(crate) fn resolve_wit_paths(options: ResolveOptions) -> Result<ResolvedWit, Error> {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").map_err(|err| {
        Error::new(Span::call_site(), format!("failed to read CARGO_MANIFEST_DIR: {err}"))
    })?;
    let manifest_path = Path::new(&manifest_dir).join("Cargo.toml");

    let manifest_content = fs::read_to_string(&manifest_path).map_err(|err| {
        Error::new(
            Span::call_site(),
            format!("failed to read manifest '{}': {err}", manifest_path.display()),
        )
    })?;

    let manifest: Value = manifest_content.parse().map_err(|err| {
        Error::new(
            Span::call_site(),
            format!("failed to parse manifest '{}': {err}", manifest_path.display()),
        )
    })?;

    let canonical_prelude_dir = ensure_sdk_wit()?;

    let mut resolved = Vec::new();

    let prelude_dir = canonical_prelude_dir
        .to_str()
        .ok_or_else(|| {
            Error::new(
                Span::call_site(),
                format!("path '{}' contains invalid UTF-8", canonical_prelude_dir.display()),
            )
        })?
        .to_owned();

    resolved.push(prelude_dir);

    if let Some(dependencies) = manifest
        .get("package")
        .and_then(Value::as_table)
        .and_then(|package| package.get("metadata"))
        .and_then(Value::as_table)
        .and_then(|metadata| metadata.get("component"))
        .and_then(Value::as_table)
        .and_then(|component| component.get("target"))
        .and_then(Value::as_table)
        .and_then(|target| target.get("dependencies"))
        .and_then(Value::as_table)
    {
        for (name, dependency) in dependencies.iter() {
            let table = dependency.as_table().ok_or_else(|| {
                Error::new(
                    Span::call_site(),
                    format!(
                        "dependency '{name}' under \
                         [package.metadata.component.target.dependencies] must be a table"
                    ),
                )
            })?;

            let path_value = table.get("path").and_then(Value::as_str).ok_or_else(|| {
                Error::new(
                    Span::call_site(),
                    format!("dependency '{name}' is missing a 'path' entry"),
                )
            })?;

            let raw_path = PathBuf::from(path_value);
            let absolute = if raw_path.is_absolute() {
                raw_path
            } else {
                Path::new(&manifest_dir).join(&raw_path)
            };

            let canonical = fs::canonicalize(&absolute).unwrap_or_else(|_| absolute.clone());

            let metadata = fs::metadata(&canonical).map_err(|err| {
                Error::new(
                    Span::call_site(),
                    format!(
                        "failed to read metadata for dependency '{name}' path '{}': {err}",
                        canonical.display()
                    ),
                )
            })?;

            let search_path = if metadata.is_dir() {
                canonical
            } else if let Some(parent) = canonical.parent() {
                parent.to_path_buf()
            } else {
                return Err(Error::new(
                    Span::call_site(),
                    format!(
                        "dependency '{name}' path '{}' does not have a parent directory",
                        canonical.display()
                    ),
                ));
            };

            let path_str = search_path.to_str().ok_or_else(|| {
                Error::new(
                    Span::call_site(),
                    format!("dependency '{name}' path contains invalid UTF-8"),
                )
            })?;

            if !resolved.iter().any(|existing| existing == path_str) {
                resolved.push(path_str.to_owned());
            }
        }
    }

    let local_wit_root = Path::new(&manifest_dir).join("wit");
    let mut world = None;

    if local_wit_root.exists() && !options.allow_missing_local_wit {
        let local_root = fs::canonicalize(&local_wit_root).unwrap_or(local_wit_root);
        let local_root_str = local_root.to_str().ok_or_else(|| {
            Error::new(
                Span::call_site(),
                format!("path '{}' contains invalid UTF-8", local_root.display()),
            )
        })?;
        if !resolved.iter().any(|existing| existing == local_root_str) {
            resolved.push(local_root_str.to_owned());
        }
        world = detect_world_name(&local_root)?;
    }

    Ok(ResolvedWit {
        paths: resolved,
        world,
    })
}

/// Ensures the embedded Miden SDK WIT is materialized in the project's folder.
fn ensure_sdk_wit() -> Result<PathBuf, Error> {
    let autogenerated_wit_folder = bundled_wit_folder()?;

    let sdk_wit_path = autogenerated_wit_folder.join(SDK_WIT_FILE_NAME);
    let sdk_version: &str = env!("CARGO_PKG_VERSION");
    let expected_source = format!(
        "/// NOTE: This file is auto-generated from the Miden SDK.\n/// Version: \
         v{sdk_version}\n/// Any manual edits will be overwritten.\n\n{SDK_WIT_SOURCE}"
    );
    let should_write_wit = match fs::read_to_string(&sdk_wit_path) {
        Ok(existing) => existing != expected_source,
        Err(err) if err.kind() == ErrorKind::NotFound => true,
        Err(err) => {
            return Err(Error::new(
                Span::call_site(),
                format!("failed to read '{}': {err}", sdk_wit_path.display()),
            ));
        }
    };

    if should_write_wit {
        fs::write(&sdk_wit_path, expected_source).map_err(|err| {
            Error::new(
                Span::call_site(),
                format!("failed to write '{}': {err}", sdk_wit_path.display()),
            )
        })?;
    }

    Ok(fs::canonicalize(&autogenerated_wit_folder).unwrap_or(autogenerated_wit_folder))
}

/// Scans the component's `wit` directory to find the default world.
fn detect_world_name(wit_root: &Path) -> Result<Option<String>, Error> {
    let mut entries = fs::read_dir(wit_root)
        .map_err(|err| {
            Error::new(Span::call_site(), format!("failed to read '{}': {err}", wit_root.display()))
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| {
            Error::new(
                Span::call_site(),
                format!("failed to iterate '{}': {err}", wit_root.display()),
            )
        })?;
    entries.sort_by_key(|entry| entry.file_name());

    for entry in entries {
        let path = entry.path();
        if path.file_name().is_some_and(|name| name == "deps") {
            continue;
        }
        if path.is_dir() {
            continue;
        }
        if path.extension().and_then(|ext| ext.to_str()) != Some("wit") {
            continue;
        }

        if let Some((package, world)) = parse_package_and_world(&path)? {
            return Ok(Some(format!("{package}/{world}")));
        }
    }

    Ok(None)
}

/// Parses a WIT source file for its package declaration and first world definition.
fn parse_package_and_world(path: &Path) -> Result<Option<(String, String)>, Error> {
    let contents = fs::read_to_string(path).map_err(|err| {
        Error::new(
            Span::call_site(),
            format!("failed to read WIT file '{}': {err}", path.display()),
        )
    })?;

    let package = extract_package_name(&contents);
    let world = extract_world_name(&contents);

    match (package, world) {
        (Some(package), Some(world)) => Ok(Some((package, world))),
        _ => Ok(None),
    }
}

/// Returns the package identifier from a WIT source string, if present.
fn extract_package_name(contents: &str) -> Option<String> {
    for line in contents.lines() {
        let trimmed = strip_line_comment(line).trim_start();
        if let Some(rest) = trimmed.strip_prefix("package ") {
            let mut token = rest.trim();
            if let Some(idx) = token.find(';') {
                token = &token[..idx];
            }
            let mut name = token.trim();
            if let Some(idx) = name.find('@') {
                name = &name[..idx];
            }
            return Some(name.trim().to_string());
        }
    }
    None
}

/// Extracts the first world identifier from a WIT source string.
pub(crate) fn extract_world_name(contents: &str) -> Option<String> {
    for line in contents.lines() {
        let trimmed = strip_line_comment(line).trim_start();
        if let Some(rest) = trimmed.strip_prefix("world ") {
            let mut name = String::new();
            for ch in rest.trim().chars() {
                if ch.is_alphanumeric() || ch == '-' || ch == '_' {
                    name.push(ch);
                } else {
                    break;
                }
            }
            if !name.is_empty() {
                return Some(name);
            }
        }
    }
    None
}
