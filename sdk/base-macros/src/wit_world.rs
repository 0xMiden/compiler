//! Shared manifest and WIT world helpers used by script-like SDK proc macros.

use std::{
    env, fs,
    path::{Path, PathBuf},
};

use proc_macro2::Span;
use toml::{Value, value::Table};

use crate::{
    util::{generated_wit_folder_at, strip_line_comment},
    wit_builder::WitBuilder,
};

/// Parsed package metadata from the consuming crate's manifest.
pub(crate) struct ManifestPackage {
    manifest_dir: PathBuf,
    package_table: Table,
}

impl ManifestPackage {
    /// Loads the current crate's `[package]` table from `Cargo.toml`.
    pub(crate) fn load(error_span: Span) -> Result<Self, syn::Error> {
        let manifest_dir = env::var("CARGO_MANIFEST_DIR").map_err(|err| {
            syn::Error::new(error_span, format!("failed to read CARGO_MANIFEST_DIR: {err}"))
        })?;
        let manifest_path = Path::new(&manifest_dir).join("Cargo.toml");
        let manifest_content = fs::read_to_string(&manifest_path).map_err(|err| {
            syn::Error::new(
                error_span,
                format!("failed to read manifest '{}': {err}", manifest_path.display()),
            )
        })?;
        let manifest: Value = manifest_content.parse().map_err(|err| {
            syn::Error::new(
                error_span,
                format!("failed to parse manifest '{}': {err}", manifest_path.display()),
            )
        })?;

        let package_table = manifest
            .get("package")
            .and_then(Value::as_table)
            .ok_or_else(|| syn::Error::new(error_span, "manifest missing [package] table"))?
            .clone();

        Ok(Self {
            manifest_dir: PathBuf::from(manifest_dir),
            package_table,
        })
    }

    /// Returns the crate name declared in `[package]`.
    pub(crate) fn crate_name(&self, error_span: Span) -> Result<&str, syn::Error> {
        self.package_table
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| syn::Error::new(error_span, "manifest package missing `name`"))
    }

    /// Returns the declared component package identifier from manifest metadata.
    pub(crate) fn component_package(&self, error_span: Span) -> Result<&str, syn::Error> {
        self.package_table
            .get("metadata")
            .and_then(Value::as_table)
            .and_then(|meta| meta.get("component"))
            .and_then(Value::as_table)
            .and_then(|component| component.get("package"))
            .and_then(Value::as_str)
            .ok_or_else(|| {
                syn::Error::new(error_span, "manifest missing package.metadata.component.package")
            })
    }

    /// Resolves fully-qualified imports exported by `package.metadata.miden.dependencies`.
    pub(crate) fn collect_miden_dependency_imports(
        &self,
        error_span: Span,
    ) -> Result<Vec<String>, syn::Error> {
        collect_miden_dependency_imports(&self.manifest_dir, &self.package_table, error_span)
    }
}

/// Writes a WIT world block with the provided imports and exports.
pub(crate) fn write_world_block(
    wit: &mut WitBuilder,
    world_name: &str,
    imports: &[String],
    exports: &[String],
) {
    wit.world(world_name, |world| {
        for import in imports {
            world.line(&format!("import {import};"));
        }
        if !imports.is_empty() && !exports.is_empty() {
            world.blank_line();
        }
        for export in exports {
            world.line(&format!("export {export};"));
        }
    });
}

/// Collects fully-qualified imports from `[package.metadata.miden.dependencies]`.
fn collect_miden_dependency_imports(
    manifest_dir: &Path,
    package_table: &Table,
    error_span: Span,
) -> Result<Vec<String>, syn::Error> {
    let dependencies = package_table
        .get("metadata")
        .and_then(Value::as_table)
        .and_then(|meta| meta.get("miden"))
        .and_then(Value::as_table)
        .and_then(|miden| miden.get("dependencies"))
        .and_then(Value::as_table);

    let mut imports = Vec::new();

    if let Some(dep_table) = dependencies {
        for (dep_name, dep_value) in dep_table {
            let dep_config = dep_value.as_table().ok_or_else(|| {
                syn::Error::new(
                    error_span,
                    format!(
                        "dependency '{dep_name}' under package.metadata.miden.dependencies must \
                         be a table"
                    ),
                )
            })?;

            let dependency_path =
                dep_config.get("path").and_then(Value::as_str).ok_or_else(|| {
                    syn::Error::new(
                        error_span,
                        format!(
                            "dependency '{dep_name}' under package.metadata.miden.dependencies is \
                             missing a 'path' entry"
                        ),
                    )
                })?;

            let absolute_path = manifest_dir.join(dependency_path);
            let canonical = fs::canonicalize(&absolute_path).map_err(|err| {
                syn::Error::new(
                    error_span,
                    format!(
                        "failed to canonicalize dependency '{dep_name}' path '{}': {err}",
                        absolute_path.display()
                    ),
                )
            })?;

            let dependency_wit = parse_dependency_wit(&canonical).map_err(|msg| {
                syn::Error::new(
                    error_span,
                    format!("failed to process WIT for dependency '{dep_name}': {msg}"),
                )
            })?;

            imports.push(qualify_dependency_export(&dependency_wit, error_span)?);
        }
    }

    imports.sort();
    Ok(imports)
}

/// Parses the first exported WIT world exposed by a dependency root or WIT file.
fn parse_dependency_wit(root: &Path) -> Result<DependencyWit, String> {
    if root.is_file() {
        return parse_wit_file(root)?.ok_or_else(|| {
            format!("WIT file '{}' does not contain a world export", root.display())
        });
    }

    let default_wit_dir = root.join("wit");
    let generated_wit_dir = generated_wit_folder_at(root)?;
    let wit_dirs = [default_wit_dir, generated_wit_dir];
    for wit_dir in &wit_dirs {
        if !wit_dir.exists() {
            continue;
        }
        let mut entries = fs::read_dir(wit_dir)
            .map_err(|err| format!("failed to read '{}': {err}", wit_dir.display()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|err| format!("failed to iterate '{}': {err}", wit_dir.display()))?;

        entries.sort_by_key(|entry| entry.file_name());

        for entry in entries {
            let path = entry.path();
            if path.is_dir() {
                if path.file_name().is_some_and(|name| name == "deps") {
                    continue;
                }
                continue;
            }

            if path.extension().and_then(|ext| ext.to_str()) != Some("wit") {
                continue;
            }

            if let Some(info) = parse_wit_file(&path)? {
                return Ok(info);
            }
        }
    }

    Err(format!("no WIT world definition found in directories '{wit_dirs:?}'"))
}

/// WIT package identifier plus exported world entries extracted from a dependency.
struct DependencyWit {
    package: String,
    version: Option<String>,
    exports: Vec<String>,
}

/// Parses a single WIT file and returns its exported world metadata.
fn parse_wit_file(path: &Path) -> Result<Option<DependencyWit>, String> {
    let contents = fs::read_to_string(path)
        .map_err(|err| format!("failed to read WIT file '{}': {err}", path.display()))?;

    let (package, version) = match extract_package_identifier(&contents) {
        Some(parts) => parts,
        None => return Ok(None),
    };

    let exports = extract_world_exports(&contents);
    if exports.is_empty() {
        return Ok(None);
    }

    Ok(Some(DependencyWit {
        package,
        version,
        exports,
    }))
}

/// Extracts the declared WIT package identifier from a source file.
fn extract_package_identifier(contents: &str) -> Option<(String, Option<String>)> {
    for line in contents.lines() {
        let trimmed = strip_line_comment(line).trim_start();
        if let Some(rest) = trimmed.strip_prefix("package ") {
            let token = rest.trim_end_matches(';').trim();
            if let Some((name, version)) = token.split_once('@') {
                return Some((name.trim().to_string(), Some(version.trim().to_string())));
            }
            return Some((token.to_string(), None));
        }
    }
    None
}

/// Extracts world export declarations from a WIT source file.
fn extract_world_exports(contents: &str) -> Vec<String> {
    let mut exports = Vec::new();

    for line in contents.lines() {
        let trimmed = strip_line_comment(line).trim();
        if let Some(rest) = trimmed.strip_prefix("export ") {
            let export = rest.trim_end_matches(';').trim();
            if !export.is_empty() {
                exports.push(export.to_string());
            }
        }
    }

    exports
}

/// Resolves the dependency's first world export to a fully-qualified import path.
fn qualify_dependency_export(
    dependency_wit: &DependencyWit,
    error_span: Span,
) -> Result<String, syn::Error> {
    let export = dependency_wit.exports.first().ok_or_else(|| {
        syn::Error::new(
            error_span,
            format!(
                "dependency package '{}' did not export any interfaces in its world definition",
                dependency_wit.package
            ),
        )
    })?;

    if export.contains(':') {
        return Ok(export.clone());
    }

    let version = dependency_wit.version.as_deref().ok_or_else(|| {
        syn::Error::new(
            error_span,
            format!("WIT package '{}' is missing a version suffix", dependency_wit.package),
        )
    })?;

    Ok(format!("{}/{export}@{version}", dependency_wit.package))
}
