//! Procedural macros that generate typed host bindings for Miden package schemas.

#![deny(missing_docs)]

extern crate proc_macro;

mod storage;

use miden_account_storage_schema::AccountStorageSchema;
use miden_mast_package::SectionId;
use miden_note_schema::NoteStorageSchema;
use miden_schema::{PackageArtifact, PackageResolver, PackageSchemaKind};
use miden_schema_codegen::{RuntimePaths, generate_host_types};
use proc_macro::TokenStream;
use proc_macro_crate::{FoundCrate, crate_name};
use proc_macro2::{Span, TokenStream as TokenStream2};
use quote::{format_ident, quote};
use syn::{LitStr, parse_macro_input};

/// Generates typed bindings from the package built by a Miden project.
///
/// The project path is relative to `CARGO_MANIFEST_DIR`. Build the note or account component
/// project with `cargo miden build` before compiling the consumer.
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

/// The crate name diagnostics use.
const MACRO_CRATE: &str = "miden-bindings";

/// Expands a project-relative binding request.
fn expand_from_project(input: &LitStr) -> syn::Result<TokenStream2> {
    let artifact = PackageResolver::new(MACRO_CRATE)
        .from_project(&input.value())
        .map_err(|error| syn::Error::new(input.span(), error.to_string()))?;
    expand_package_artifact(&artifact, input.span())
}

/// Expands an exact package binding request.
fn expand_from_package(input: &LitStr) -> syn::Result<TokenStream2> {
    let artifact = PackageResolver::new(MACRO_CRATE)
        .from_package(&input.value())
        .map_err(|error| syn::Error::new(input.span(), error.to_string()))?;
    expand_package_artifact(&artifact, input.span())
}

/// Expands bindings from a loaded package and tracks the artifact as a macro input.
///
/// The schema section the package carries selects the bindings: a note gets its storage root
/// type, an account component its `Storage` root.
fn expand_package_artifact(artifact: &PackageArtifact, span: Span) -> syn::Result<TokenStream2> {
    let read_error = |kind: &str, error: miden_schema::Error| {
        syn::Error::new(
            span,
            format!(
                "{MACRO_CRATE}: failed to read {kind} schema from '{}': {error}",
                artifact.path().display()
            ),
        )
    };
    let scope_key = artifact.path().to_string_lossy();
    let kind = artifact
        .schema_kind()
        .map_err(|error| syn::Error::new(span, format!("{MACRO_CRATE}: {error}")))?;
    let bindings = match kind {
        PackageSchemaKind::NoteStorage => {
            let schema = NoteStorageSchema::from_package(artifact.package())
                .map_err(|error| read_error("note storage", error))?;
            expand_schema(&schema, span, &scope_key)?
        }
        PackageSchemaKind::AccountStorage => {
            let schema = AccountStorageSchema::from_package(artifact.package())
                .map_err(|error| read_error("account storage", error))?;
            let metadata = artifact
                .package()
                .sections
                .iter()
                .find(|section| section.id == SectionId::ACCOUNT_COMPONENT_METADATA)
                .map(|section| section.data.as_ref())
                .expect("an account storage schema is read together with the protocol metadata");
            storage::expand_storage_schema(
                &schema,
                metadata,
                &binding_facade(),
                span,
                &scope_ident(&scope_key),
            )?
        }
    };
    let tracked_path = artifact.path().to_string_lossy();
    let tracked_manifests = artifact
        .tracked_inputs()
        .iter()
        .map(|path| path.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    let package_cache_env = midenc_frontend_wasm_metadata::package_cache::PACKAGE_CACHE_ENV;
    Ok(quote! {
        // These constants exist only to register the package file, the manifests that named it,
        // and the cache path as proc-macro rebuild inputs.
        const _: &[u8] = ::core::include_bytes!(#tracked_path);
        #(const _: &[u8] = ::core::include_bytes!(#tracked_manifests);)*
        const _: ::core::option::Option<&str> = ::core::option_env!(#package_cache_env);
        #bindings
    })
}

/// Expands bindings from a WIT string literal.
///
/// Only a note storage schema is accepted: the slot names of an account storage schema come
/// from the package's protocol metadata.
fn expand_from_wit_text(input: &LitStr) -> syn::Result<TokenStream2> {
    let wit_text = input.value();
    let schema = NoteStorageSchema::from_wit_text(&wit_text).map_err(|error| {
        let message = if AccountStorageSchema::from_wit_text(&wit_text).is_ok() {
            "an account storage schema needs the slot names of its package's protocol metadata; \
             generate its bindings with from_project! or from_package!"
                .to_owned()
        } else {
            error.to_string()
        };
        syn::Error::new(input.span(), message)
    })?;
    expand_schema(&schema, input.span(), &wit_text)
}

/// Adds the typed consumer API to shared generated host types.
fn expand_schema(
    schema: &NoteStorageSchema,
    span: Span,
    scope_key: &str,
) -> syn::Result<TokenStream2> {
    let facade_path = binding_facade();
    let runtime_paths = RuntimePaths::through_facade(quote!(#facade_path));
    let generated = generate_host_types(schema.schema(), &runtime_paths)
        .map_err(|error| syn::Error::new(span, error.to_string()))?;
    let type_tokens = generated.tokens();
    let root_ident = generated.root_ident();
    let type_idents = generated.type_idents();
    let wit_text = schema.wit_text();
    let scope_ident = scope_ident(scope_key);
    let runtime = quote!(#facade_path::__private);

    Ok(quote! {
        #[doc(hidden)]
        mod #scope_ident {
            #type_tokens

            #[doc(hidden)]
            const __MIDEN_NOTE_STORAGE_SCHEMA_WIT: &str = #wit_text;

            /// Returns the resolved schema, which is parsed once for the whole process.
            #[doc(hidden)]
            fn __miden_note_storage_schema() -> #runtime::miden_note_schema::Result<
                &'static #runtime::miden_note_schema::NoteStorageSchema,
            > {
                static __MIDEN_NOTE_STORAGE_SCHEMA: ::std::sync::OnceLock<
                    #runtime::miden_note_schema::Result<
                        #runtime::miden_note_schema::NoteStorageSchema,
                    >,
                > = ::std::sync::OnceLock::new();
                __MIDEN_NOTE_STORAGE_SCHEMA
                    .get_or_init(|| {
                        #runtime::miden_note_schema::NoteStorageSchema::from_wit_text(
                            __MIDEN_NOTE_STORAGE_SCHEMA_WIT,
                        )
                    })
                    .as_ref()
                    .map_err(::core::clone::Clone::clone)
            }

            impl #root_ident {
                /// Encodes this typed value as note storage in WIT declaration order.
                pub fn to_note_storage(
                    &self,
                ) -> #runtime::miden_note_schema::Result<#runtime::miden_note_schema::NoteStorage> {
                    let mut felts = ::std::vec::Vec::new();
                    self.__write_felts(
                        &mut #runtime::miden_field_repr::FeltWriter::new(&mut felts),
                    )?;
                    #runtime::miden_note_schema::NoteStorage::new(felts).map_err(|error| {
                        #runtime::miden_note_schema::Error::new(format!(
                            "failed to create note storage: {error}"
                        ))
                    })
                }

                /// Decodes this typed value from complete note storage.
                pub fn from_note_storage(
                    storage: &#runtime::miden_note_schema::NoteStorage,
                ) -> #runtime::miden_note_schema::Result<Self> {
                    let mut reader = #runtime::miden_field_repr::FeltReader::new(storage.items());
                    let value = Self::__read_felts(&mut reader)?;
                    reader.ensure_eof().map_err(|error| {
                        #runtime::miden_note_schema::Error::new(format!(
                            "note storage has trailing data: {error}"
                        ))
                    })?;
                    Ok(value)
                }

                /// Builds a typed value with a caller-provided codec registry.
                pub fn from_str_values_with(
                    values: &::std::collections::BTreeMap<
                        ::std::string::String,
                        ::std::string::String,
                    >,
                    codecs: &#runtime::miden_note_schema::CodecRegistry,
                ) -> #runtime::miden_note_schema::Result<Self> {
                    let schema = __miden_note_storage_schema()?;
                    let mut builder = schema.builder_with_registry(codecs);
                    for (path, value) in values {
                        builder = builder.set(path, value)?;
                    }
                    let storage = builder.build()?;
                    Self::from_note_storage(&storage)
                }

                /// Builds a typed value with the standard codec registry.
                pub fn from_str_values(
                    values: &::std::collections::BTreeMap<
                        ::std::string::String,
                        ::std::string::String,
                    >,
                ) -> #runtime::miden_note_schema::Result<Self> {
                    let codecs =
                        #runtime::miden_note_schema::CodecRegistry::with_standard_codecs();
                    Self::from_str_values_with(values, &codecs)
                }

                /// Validates this value with structural rules and caller-provided codecs.
                pub fn validate_with(
                    &self,
                    codecs: &#runtime::miden_note_schema::CodecRegistry,
                ) -> #runtime::miden_note_schema::Result<()> {
                    let storage = self.to_note_storage()?;
                    __miden_note_storage_schema()?.decode_with_registry(&storage, codecs)?;
                    Ok(())
                }

                /// Validates this value with the standard codec registry.
                pub fn validate(&self) -> #runtime::miden_note_schema::Result<()> {
                    let codecs =
                        #runtime::miden_note_schema::CodecRegistry::with_standard_codecs();
                    self.validate_with(&codecs)
                }

                /// Displays this value with caller-provided codecs and structural fallbacks.
                pub fn display_with(
                    &self,
                    codecs: &#runtime::miden_note_schema::CodecRegistry,
                ) -> #runtime::miden_note_schema::Result<::std::string::String> {
                    let storage = self.to_note_storage()?;
                    let decoded =
                        __miden_note_storage_schema()?.decode_with_registry(&storage, codecs)?;
                    Ok(decoded.to_string())
                }

                /// Displays this value with standard codecs and structural fallbacks.
                pub fn display(
                    &self,
                ) -> #runtime::miden_note_schema::Result<::std::string::String> {
                    let codecs =
                        #runtime::miden_note_schema::CodecRegistry::with_standard_codecs();
                    self.display_with(&codecs)
                }
            }
        }

        pub use #scope_ident::{#(#type_idents),*};
    })
}

/// Resolves the bindings facade path in the consuming crate.
fn binding_facade() -> syn::Path {
    match crate_name(MACRO_CRATE) {
        Ok(FoundCrate::Itself) => syn::parse_quote!(crate),
        Ok(FoundCrate::Name(name)) => {
            let ident = syn::Ident::new(&name, Span::call_site());
            syn::parse_quote!(::#ident)
        }
        Err(_) => syn::parse_quote!(::miden_bindings),
    }
}

/// Returns the module that scopes the bindings of one macro input.
fn scope_ident(scope_key: &str) -> syn::Ident {
    format_ident!("__miden_bindings_{:016x}", stable_hash(scope_key))
}

/// Returns a deterministic FNV-1a scope suffix for one macro input.
fn stable_hash(value: &str) -> u64 {
    value.bytes().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

#[cfg(test)]
mod tests;
