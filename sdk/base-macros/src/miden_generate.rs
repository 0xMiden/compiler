use std::{
    env, fs,
    path::{Path, PathBuf},
};

use proc_macro2::{Literal, Span};
use quote::quote;
use syn::Error;

/// Implements the expansion logic for the `miden_generate!` macro.
pub(crate) fn expand(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    if !input.is_empty() {
        return Error::new(Span::call_site(), "miden_generate! does not take any arguments")
            .to_compile_error()
            .into();
    }

    match manifest_paths::resolve_wit_paths() {
        Ok(config) => {
            if config.paths.is_empty() {
                return Error::new(
                    Span::call_site(),
                    "no WIT dependencies declared under \
                     [package.metadata.component.target.dependencies]",
                )
                .to_compile_error()
                .into();
            }

            let path_literals = config.paths.iter().map(|path| Literal::string(path));
            let world_clause = config.world.as_ref().map(|world| {
                let literal = Literal::string(world);
                quote! { world: #literal, }
            });

            quote! {
                #[doc(hidden)]
                #[allow(dead_code)]
                pub mod bindings {
                    ::miden::wit_bindgen::generate!({
                        #world_clause
                        path: [#(#path_literals),*],
                        generate_all,
                        runtime_path: "::miden::wit_bindgen::rt",
                        // path to use in the generated `export!` macro
                        default_bindings_module: "bindings",
                        with: {
                            "miden:base/core-types@1.0.0": generate,
                            "miden:base/core-types@1.0.0/felt": ::miden::Felt,
                            "miden:base/core-types@1.0.0/word": ::miden::Word,
                            "miden:base/core-types@1.0.0/asset": ::miden::Asset,
                            "miden:base/core-types@1.0.0/account-id": ::miden::AccountId,
                            "miden:base/core-types@1.0.0/tag": ::miden::Tag,
                            "miden:base/core-types@1.0.0/note-type": ::miden::NoteType,
                            "miden:base/core-types@1.0.0/recipient": ::miden::Recipient,
                            "miden:base/core-types@1.0.0/note-idx": ::miden::NoteIdx,
                        },
                    });
                }
            }
            .into()
        }
        Err(err) => err.to_compile_error().into(),
    }
}

mod manifest_paths {
    use toml::Value;

    use super::*;

    /// WIT metadata extracted from the consuming crate.
    pub struct ResolvedWit {
        pub paths: Vec<String>,
        pub world: Option<String>,
    }

    /// Collects WIT search paths and the target world from `Cargo.toml` + local files.
    pub fn resolve_wit_paths() -> Result<ResolvedWit, Error> {
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

        let dependencies = manifest
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
            .ok_or_else(|| {
                Error::new(
                    Span::call_site(),
                    "missing [package.metadata.component.target.dependencies] table in Cargo.toml",
                )
            })?;

        let mut resolved = Vec::new();

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

        let local_wit_root = Path::new(&manifest_dir).join("wit");
        if !local_wit_root.exists() {
            return Err(Error::new(
                Span::call_site(),
                format!(
                    "expected a 'wit' directory with WIT files in '{manifest_dir}', but none was \
                     found",
                ),
            ));
        }
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

        let world = detect_world_name(&local_root)?;

        Ok(ResolvedWit {
            paths: resolved,
            world,
        })
    }

    /// Scans the component's `wit` directory to find the default world.
    fn detect_world_name(wit_root: &Path) -> Result<Option<String>, Error> {
        let mut entries = fs::read_dir(wit_root)
            .map_err(|err| {
                Error::new(
                    Span::call_site(),
                    format!("failed to read '{}': {err}", wit_root.display()),
                )
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
            let trimmed = strip_comment(line).trim_start();
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

    /// Returns the first world identifier from a WIT source string, if present.
    fn extract_world_name(contents: &str) -> Option<String> {
        for line in contents.lines() {
            let trimmed = strip_comment(line).trim_start();
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

    /// Strips line comments starting with `//` from the provided source line.
    fn strip_comment(line: &str) -> &str {
        match line.split_once("//") {
            Some((before, _)) => before,
            None => line,
        }
    }
}
