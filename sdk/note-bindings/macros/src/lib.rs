//! Procedural macros that generate typed host bindings for Miden note storage.

#![deny(missing_docs)]

extern crate proc_macro;

use miden_note_schema::{NotePackageArtifact, NotePackageResolver, NoteStorageSchema};
use miden_note_schema_codegen::generate_host_types;
use proc_macro::TokenStream;
use proc_macro_crate::{FoundCrate, crate_name};
use proc_macro2::{Span, TokenStream as TokenStream2};
use quote::{format_ident, quote};
use syn::{
    LitStr, Token, parse::Parser, parse_macro_input, punctuated::Punctuated, visit_mut::VisitMut,
};

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
    let artifact = NotePackageResolver::new("miden-note-bindings")
        .from_project(&input.value())
        .map_err(|error| syn::Error::new(input.span(), error.to_string()))?;
    expand_package_artifact(&artifact, input.span())
}

/// Expands an exact package binding request.
fn expand_from_package(input: &LitStr) -> syn::Result<TokenStream2> {
    let artifact = NotePackageResolver::new("miden-note-bindings")
        .from_package(&input.value())
        .map_err(|error| syn::Error::new(input.span(), error.to_string()))?;
    expand_package_artifact(&artifact, input.span())
}

/// Expands bindings from a loaded package and tracks the artifact as a macro input.
fn expand_package_artifact(
    artifact: &NotePackageArtifact,
    span: Span,
) -> syn::Result<TokenStream2> {
    let scope_key = artifact.path().to_string_lossy();
    let bindings = expand_schema(artifact.schema(), span, &scope_key)?;
    let tracked_path = artifact.path().to_string_lossy();
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
    expand_schema(&schema, input.span(), &input.value())
}

/// Adds the typed consumer API to shared generated host types.
fn expand_schema(
    schema: &NoteStorageSchema,
    span: Span,
    scope_key: &str,
) -> syn::Result<TokenStream2> {
    let generated =
        generate_host_types(schema).map_err(|error| syn::Error::new(span, error.to_string()))?;
    let facade_path = binding_facade();
    let type_tokens = rewrite_runtime_paths(generated.tokens().clone(), &facade_path)?;
    let root_ident = generated.root_ident();
    let type_idents = generated.type_idents();
    let wit_text = schema.wit_text();
    let scope_ident = format_ident!("__miden_note_bindings_{:016x}", stable_hash(scope_key));
    let runtime = quote!(#facade_path::__private);

    Ok(quote! {
        #[doc(hidden)]
        mod #scope_ident {
            #type_tokens

            #[doc(hidden)]
            const __MIDEN_NOTE_STORAGE_SCHEMA_WIT: &str = #wit_text;

            #[doc(hidden)]
            fn __miden_note_storage_schema(
            ) -> #runtime::miden_note_schema::Result<#runtime::miden_note_schema::NoteStorageSchema> {
                #runtime::miden_note_schema::NoteStorageSchema::from_wit_text(
                    __MIDEN_NOTE_STORAGE_SCHEMA_WIT,
                )
            }

            impl #root_ident {
                /// Encodes this typed value as note storage in WIT declaration order.
                pub fn to_note_storage(
                    &self,
                ) -> #runtime::miden_note_schema::Result<#runtime::miden_note_schema::NoteStorage> {
                    let mut felts = Vec::new();
                    self.__write_note_felts(
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
                    let value = Self::__read_note_felts(&mut reader)?;
                    reader.ensure_eof().map_err(|error| {
                        #runtime::miden_note_schema::Error::new(format!(
                            "note storage has trailing data: {error}"
                        ))
                    })?;
                    Ok(value)
                }

                /// Builds a typed value with a caller-provided codec registry.
                pub fn from_str_values_with(
                    values: &::std::collections::BTreeMap<String, String>,
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
                    values: &::std::collections::BTreeMap<String, String>,
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
                ) -> #runtime::miden_note_schema::Result<String> {
                    let storage = self.to_note_storage()?;
                    let decoded =
                        __miden_note_storage_schema()?.decode_with_registry(&storage, codecs)?;
                    Ok(decoded.to_string())
                }

                /// Displays this value with standard codecs and structural fallbacks.
                pub fn display(&self) -> #runtime::miden_note_schema::Result<String> {
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
    match crate_name("miden-note-bindings") {
        Ok(FoundCrate::Itself) => syn::parse_quote!(crate),
        Ok(FoundCrate::Name(name)) => {
            let ident = syn::Ident::new(&name, Span::call_site());
            syn::parse_quote!(::#ident)
        }
        Err(_) => syn::parse_quote!(::miden_note_bindings),
    }
}

/// Rewrites shared generated paths through the bindings facade.
fn rewrite_runtime_paths(tokens: TokenStream2, facade: &syn::Path) -> syn::Result<TokenStream2> {
    let mut file = syn::parse2::<syn::File>(tokens)?;
    RuntimePathRewriter { facade }.visit_file_mut(&mut file);
    Ok(quote!(#file))
}

/// Rewrites runtime crate paths in shared host-profile code generation.
struct RuntimePathRewriter<'a> {
    facade: &'a syn::Path,
}

impl VisitMut for RuntimePathRewriter<'_> {
    fn visit_item_struct_mut(&mut self, item: &mut syn::ItemStruct) {
        add_felt_repr_crate_path(&mut item.attrs, self.facade);
        syn::visit_mut::visit_item_struct_mut(self, item);
    }

    fn visit_item_enum_mut(&mut self, item: &mut syn::ItemEnum) {
        add_felt_repr_crate_path(&mut item.attrs, self.facade);
        syn::visit_mut::visit_item_enum_mut(self, item);
    }

    fn visit_attribute_mut(&mut self, attribute: &mut syn::Attribute) {
        if attribute.path().is_ident("derive") {
            let parser = Punctuated::<syn::Path, Token![,]>::parse_terminated;
            let mut paths = parser
                .parse2(attribute.meta.require_list().expect("derive is a list").tokens.clone())
                .expect("generated derive paths must parse");
            for path in &mut paths {
                self.visit_path_mut(path);
            }
            attribute.meta = syn::parse_quote!(derive(#paths));
            return;
        }
        syn::visit_mut::visit_attribute_mut(self, attribute);
    }

    fn visit_path_mut(&mut self, path: &mut syn::Path) {
        let Some(first) = path.segments.first() else {
            return;
        };
        if path.leading_colon.is_none()
            || !matches!(
                first.ident.to_string().as_str(),
                "miden_field" | "miden_field_repr" | "miden_note_schema" | "miden_protocol"
            )
        {
            syn::visit_mut::visit_path_mut(self, path);
            return;
        }

        let crate_name = first.ident.clone();
        let tail = path.segments.iter().skip(1).cloned().collect::<Vec<_>>();
        let facade = self.facade;
        let mut rewritten: syn::Path = syn::parse_quote!(#facade::__private::#crate_name);
        rewritten.segments.extend(tail);
        *path = rewritten;
    }
}

/// Selects the facade's felt representation runtime for generated derives.
fn add_felt_repr_crate_path(attributes: &mut Vec<syn::Attribute>, facade: &syn::Path) {
    let has_felt_repr_derive = attributes.iter().any(|attribute| {
        if !attribute.path().is_ident("derive") {
            return false;
        }
        let parser = Punctuated::<syn::Path, Token![,]>::parse_terminated;
        parser
            .parse2(attribute.meta.require_list().expect("derive is a list").tokens.clone())
            .expect("generated derive paths must parse")
            .iter()
            .any(|path| {
                path.segments.last().is_some_and(|segment| {
                    matches!(segment.ident.to_string().as_str(), "ToFeltRepr" | "FromFeltRepr")
                })
            })
    });
    if has_felt_repr_derive {
        let path = format!("{}::__private::miden_field_repr", quote!(#facade)).replace(' ', "");
        let path = LitStr::new(&path, Span::call_site());
        attributes.push(syn::parse_quote!(#[felt_repr(crate_path = #path)]));
    }
}

/// Returns a deterministic scope suffix for one macro input.
fn stable_hash(value: &str) -> u64 {
    value.bytes().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

#[cfg(test)]
mod tests;
