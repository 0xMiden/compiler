//! Procedural macros that generate typed host bindings for Miden note storage.

#![deny(missing_docs)]

extern crate proc_macro;

use std::{
    env, fs,
    path::{Path, PathBuf},
};

use miden_mast_package::Package;
use miden_note_schema::NoteStorageSchema;
use miden_note_schema_codegen::generate_host_types;
use proc_macro::TokenStream;
use proc_macro2::{Span, TokenStream as TokenStream2};
use quote::quote;
use syn::{LitStr, parse_macro_input};

/// Generates typed bindings from the freshest package built by a Miden project.
///
/// The project path is relative to `CARGO_MANIFEST_DIR`. Build the note project with
/// `cargo miden build` before compiling the consumer.
#[proc_macro]
pub fn from_project(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as LitStr);
    expand_from_project(&input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// Generates typed bindings from one exact Miden package path.
///
/// A relative path is resolved against `CARGO_MANIFEST_DIR`.
#[proc_macro]
pub fn from_package(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as LitStr);
    expand_from_package(&input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// Generates typed bindings from WIT text for internal tests.
#[doc(hidden)]
#[proc_macro]
pub fn from_wit_text(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as LitStr);
    expand_from_wit_text(&input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// Expands a project-relative note binding request.
fn expand_from_project(input: &LitStr) -> syn::Result<TokenStream2> {
    let project_dir = resolve_manifest_path(&input.value(), input.span())?;
    if !project_dir.is_dir() {
        return Err(syn::Error::new(
            input.span(),
            format!("note project directory '{}' does not exist", project_dir.display()),
        ));
    }
    let package_path = freshest_project_package(&project_dir, input.span())?.ok_or_else(|| {
        syn::Error::new(input.span(), missing_project_package_message(&project_dir))
    })?;
    expand_package_path(&package_path, input.span())
}

/// Expands an exact package binding request.
fn expand_from_package(input: &LitStr) -> syn::Result<TokenStream2> {
    let package_path = resolve_manifest_path(&input.value(), input.span())?;
    if !package_path.is_file() {
        return Err(syn::Error::new(
            input.span(),
            format!("Miden package '{}' does not exist", package_path.display()),
        ));
    }
    expand_package_path(&package_path, input.span())
}

/// Expands bindings from a loaded package and tracks the artifact as a macro input.
fn expand_package_path(package_path: &Path, span: Span) -> syn::Result<TokenStream2> {
    let package = Package::deserialize_from_file(package_path).map_err(|error| {
        syn::Error::new(
            span,
            format!("failed to read Miden package '{}': {error}", package_path.display()),
        )
    })?;
    let schema = NoteStorageSchema::from_package(&package).map_err(|error| {
        syn::Error::new(
            span,
            format!(
                "failed to read note storage schema from '{}': {error}",
                package_path.display()
            ),
        )
    })?;
    let bindings = expand_schema(&schema, span)?;
    let tracked_path = package_path.to_string_lossy();
    Ok(quote! {
        #[doc(hidden)]
        const _: &[u8] = include_bytes!(#tracked_path);
        #bindings
    })
}

/// Expands bindings from a WIT string literal.
fn expand_from_wit_text(input: &LitStr) -> syn::Result<TokenStream2> {
    let schema = NoteStorageSchema::from_wit_text(&input.value())
        .map_err(|error| syn::Error::new(input.span(), error.to_string()))?;
    expand_schema(&schema, input.span())
}

/// Adds the typed consumer API to shared generated host types.
fn expand_schema(schema: &NoteStorageSchema, span: Span) -> syn::Result<TokenStream2> {
    let generated =
        generate_host_types(schema).map_err(|error| syn::Error::new(span, error.to_string()))?;
    let type_tokens = generated.tokens();
    let root_ident = generated.root_ident();
    let wit_text = schema.wit_text();

    let (from_str_values, validate_with, display_with) = if generated.has_custom_types() {
        (
            quote! {
                /// Builds a typed value from normalized string paths and a codec registry.
                pub fn from_str_values(
                    values: &::std::collections::BTreeMap<String, String>,
                    codecs: &::miden_note_schema::CodecRegistry,
                ) -> ::miden_note_schema::Result<Self> {
                    let schema = __miden_note_storage_schema()?;
                    let mut builder = schema.builder_with_registry(codecs);
                    for (path, value) in values {
                        builder = builder.set(path, value)?;
                    }
                    let storage = builder.build()?;
                    Self::from_note_storage(&storage)
                }
            },
            quote! {
                /// Validates this value with structural rules and the supplied codecs.
                pub fn validate_with(
                    &self,
                    codecs: &::miden_note_schema::CodecRegistry,
                ) -> ::miden_note_schema::Result<()> {
                    let storage = self.to_note_storage()?;
                    __miden_note_storage_schema()?.decode_with_registry(&storage, codecs)?;
                    Ok(())
                }
            },
            quote! {
                /// Displays this value with the supplied codecs and structural fallbacks.
                pub fn display_with(
                    &self,
                    codecs: &::miden_note_schema::CodecRegistry,
                ) -> ::miden_note_schema::Result<String> {
                    let storage = self.to_note_storage()?;
                    let decoded =
                        __miden_note_storage_schema()?.decode_with_registry(&storage, codecs)?;
                    Ok(decoded.to_string())
                }
            },
        )
    } else {
        (
            quote! {
                /// Builds a typed value from normalized string paths.
                pub fn from_str_values(
                    values: &::std::collections::BTreeMap<String, String>,
                ) -> ::miden_note_schema::Result<Self> {
                    let schema = __miden_note_storage_schema()?;
                    let mut builder = schema.builder();
                    for (path, value) in values {
                        builder = builder.set(path, value)?;
                    }
                    let storage = builder.build()?;
                    Self::from_note_storage(&storage)
                }
            },
            quote! {
                /// Validates this value with structural rules and standard codecs.
                pub fn validate_with(&self) -> ::miden_note_schema::Result<()> {
                    let storage = self.to_note_storage()?;
                    __miden_note_storage_schema()?.decode(&storage)?;
                    Ok(())
                }
            },
            quote! {
                /// Displays this value with standard codecs and structural fallbacks.
                pub fn display_with(&self) -> ::miden_note_schema::Result<String> {
                    let storage = self.to_note_storage()?;
                    let decoded = __miden_note_storage_schema()?.decode(&storage)?;
                    Ok(decoded.to_string())
                }
            },
        )
    };

    Ok(quote! {
        #type_tokens

        #[doc(hidden)]
        const __MIDEN_NOTE_STORAGE_SCHEMA_WIT: &str = #wit_text;

        #[doc(hidden)]
        fn __miden_note_storage_schema(
        ) -> ::miden_note_schema::Result<::miden_note_schema::NoteStorageSchema> {
            ::miden_note_schema::NoteStorageSchema::from_wit_text(
                __MIDEN_NOTE_STORAGE_SCHEMA_WIT,
            )
        }

        impl #root_ident {
            /// Encodes this typed value as note storage in WIT declaration order.
            pub fn to_note_storage(
                &self,
            ) -> ::miden_note_schema::Result<::miden_note_schema::NoteStorage> {
                let mut felts = Vec::new();
                self.__write_note_felts(
                    &mut ::miden_field_repr::FeltWriter::new(&mut felts),
                )?;
                ::miden_note_schema::NoteStorage::new(felts).map_err(|error| {
                    ::miden_note_schema::Error::new(format!(
                        "failed to create note storage: {error}"
                    ))
                })
            }

            /// Decodes this typed value from complete note storage.
            pub fn from_note_storage(
                storage: &::miden_note_schema::NoteStorage,
            ) -> ::miden_note_schema::Result<Self> {
                let mut reader = ::miden_field_repr::FeltReader::new(storage.items());
                let value = Self::__read_note_felts(&mut reader)?;
                reader.ensure_eof().map_err(|error| {
                    ::miden_note_schema::Error::new(format!(
                        "note storage has trailing data: {error}"
                    ))
                })?;
                Ok(value)
            }

            #from_str_values
            #validate_with
            #display_with
        }
    })
}

/// Resolves a macro path relative to the consuming crate manifest.
fn resolve_manifest_path(value: &str, span: Span) -> syn::Result<PathBuf> {
    let path = PathBuf::from(value);
    if path.is_absolute() {
        return Ok(path);
    }
    let manifest_dir = env::var_os("CARGO_MANIFEST_DIR").ok_or_else(|| {
        syn::Error::new(span, "CARGO_MANIFEST_DIR is not set during note binding generation")
    })?;
    Ok(PathBuf::from(manifest_dir).join(path))
}

/// Returns the newest package directly inside any project Miden profile directory.
fn freshest_project_package(project_dir: &Path, span: Span) -> syn::Result<Option<PathBuf>> {
    let target_dir = project_dir.join("target/miden");
    if !target_dir.is_dir() {
        return Ok(None);
    }

    let profile_dirs = candidate_profile_dirs(&target_dir, span)?;
    let mut candidates = Vec::new();
    for profile_dir in profile_dirs {
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
            if !path.is_file() || path.extension().is_none_or(|extension| extension != "masp") {
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

/// Returns project profile directories in candidate order without duplicates.
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
fn missing_project_package_message(project_dir: &Path) -> String {
    let manifest = project_dir.join("Cargo.toml");
    let build = if manifest.is_file() {
        format!("cargo miden build --manifest-path {} --release", manifest.display())
    } else {
        "cargo miden build --release".to_owned()
    };
    format!(
        "miden-note-bindings could not find a built `.masp` package under '{}'. Build the note \
         project first with `{build}`.",
        project_dir.join("target/miden/<profile>").display()
    )
}

#[cfg(test)]
mod tests;
