use std::{
    env, fs,
    io::ErrorKind,
    path::{Path, PathBuf},
};

use proc_macro2::{Literal, Span};
use quote::quote;
use syn::{
    parse::{Parse, ParseStream},
    Error, LitStr, Token,
};

/// Folder within a project that holds bundled WIT dependencies.
const BUNDLED_WIT_DEPS_DIR: &str = "miden-wit-auto-generated";
/// File name for the embedded Miden SDK WIT .
const SDK_WIT_FILE_NAME: &str = "miden.wit";
/// Embedded Miden SDK WIT source.
const SDK_WIT_SOURCE: &str = include_str!("../../base/wit/miden.wit");

#[derive(Default)]
struct GenerateArgs {
    inline: Option<LitStr>,
}

impl Parse for GenerateArgs {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let mut args = GenerateArgs::default();

        while !input.is_empty() {
            let ident: syn::Ident = input.parse()?;
            let name = ident.to_string();
            input.parse::<Token![=]>()?;

            match name.as_str() {
                "inline" => {
                    if args.inline.is_some() {
                        return Err(syn::Error::new(ident.span(), "duplicate `inline` argument"));
                    }
                    args.inline = Some(input.parse()?);
                }
                _ => {
                    return Err(syn::Error::new(
                        ident.span(),
                        format!("unsupported generate! argument `{name}`"),
                    ));
                }
            }

            if input.peek(Token![,]) {
                let _ = input.parse::<Token![,]>()?;
            }
        }

        Ok(args)
    }
}

/// Implements the expansion logic for the `generate!` macro.
pub(crate) fn expand(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let input_tokens: proc_macro2::TokenStream = input.into();
    let args = if input_tokens.is_empty() {
        GenerateArgs::default()
    } else {
        match syn::parse2::<GenerateArgs>(input_tokens) {
            Ok(parsed) => parsed,
            Err(err) => return err.to_compile_error().into(),
        }
    };

    let resolve_opts = manifest_paths::ResolveOptions {
        allow_missing_local_wit: args.inline.is_some(),
    };

    match manifest_paths::resolve_wit_paths(resolve_opts) {
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

            let path_literals: Vec<_> =
                config.paths.iter().map(|path| Literal::string(path)).collect();

            let inline_clause = args.inline.as_ref().map(|src| quote! { inline: #src, });
            let inline_world = args
                .inline
                .as_ref()
                .and_then(|src| manifest_paths::extract_world_name(&src.value()));
            let world_value = inline_world.or_else(|| config.world.clone());

            if args.inline.is_some() && world_value.is_none() {
                return Error::new(
                    Span::call_site(),
                    "failed to detect world name for inline WIT provided to generate!",
                )
                .to_compile_error()
                .into();
            }

            let world_clause = world_value.as_ref().map(|world| {
                let literal = Literal::string(world);
                quote! { world: #literal, }
            });

            quote! {
                // Wrap the bindings in the `bindings` module since `generate!` makes a top level
                // module named after the package namespace which is `miden` for all our projects
                // so its conflicts with the `miden` crate (SDK)
                #[doc(hidden)]
                #[allow(dead_code)]
                pub mod bindings {
                    ::miden::wit_bindgen::generate!({
                        #inline_clause
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

    #[derive(Default)]
    pub struct ResolveOptions {
        pub allow_missing_local_wit: bool,
    }

    /// Collects WIT search paths and the target world from `Cargo.toml` + local files.
    pub fn resolve_wit_paths(options: ResolveOptions) -> Result<ResolvedWit, Error> {
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

        if local_wit_root.exists() {
            if !options.allow_missing_local_wit {
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
        } else if !options.allow_missing_local_wit {
            return Err(Error::new(
                Span::call_site(),
                format!(
                    "expected a 'wit' directory with WIT files in '{manifest_dir}', but none was \
                     found",
                ),
            ));
        }

        Ok(ResolvedWit {
            paths: resolved,
            world,
        })
    }

    /// Ensures the embedded Miden SDK WIT is materialized in the project's folder.
    fn ensure_sdk_wit() -> Result<PathBuf, Error> {
        let out_dir = PathBuf::from(env::var("CARGO_TARGET_DIR").unwrap_or_else(|_| {
            let mut manifest_dir =
                env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is not set");
            manifest_dir.push_str("/target/");
            manifest_dir
        }));
        let wit_deps_dir = out_dir.join(BUNDLED_WIT_DEPS_DIR);
        fs::create_dir_all(&wit_deps_dir).map_err(|err| {
            Error::new(
                Span::call_site(),
                format!(
                    "failed to create WIT dependencies directory '{}': {err}",
                    wit_deps_dir.display()
                ),
            )
        })?;

        let sdk_wit_path = wit_deps_dir.join(super::SDK_WIT_FILE_NAME);
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

        Ok(fs::canonicalize(&wit_deps_dir).unwrap_or(wit_deps_dir))
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
    pub(super) fn extract_world_name(contents: &str) -> Option<String> {
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
