//! Macro expansion for generated author types and component dispatch.

use miden_note_schema::{NotePackageArtifact, NotePackageResolver, NoteStorageSchema};
use miden_note_schema_codegen::generate_host_types;
use proc_macro_crate::{FoundCrate, crate_name};
use proc_macro2::{Span, TokenStream};
use quote::{ToTokens, quote};
use syn::{ItemImpl, LitStr, Type, visit_mut::VisitMut};

use crate::registry::{register_codec, register_schema, registered_codecs};

/// The component world embedded in generated export glue.
const NOTE_CODEC_WIT: &str = include_str!("../wit/note-codec.wit");

/// Expands a project-relative type generation request.
pub(crate) fn from_project(input: &LitStr) -> syn::Result<TokenStream> {
    let artifact = NotePackageResolver::new("miden-note-codec")
        .from_project(&input.value())
        .map_err(|error| syn::Error::new(input.span(), error.to_string()))?;
    expand_package_artifact(&artifact, input.span())
}

/// Expands an exact package type generation request.
pub(crate) fn from_package(input: &LitStr) -> syn::Result<TokenStream> {
    let artifact = NotePackageResolver::new("miden-note-codec")
        .from_package(&input.value())
        .map_err(|error| syn::Error::new(input.span(), error.to_string()))?;
    expand_package_artifact(&artifact, input.span())
}

/// Expands a WIT string literal for internal tests.
pub(crate) fn from_wit_text(input: &LitStr) -> syn::Result<TokenStream> {
    let schema = NoteStorageSchema::from_wit_text(&input.value())
        .map_err(|error| syn::Error::new(input.span(), error.to_string()))?;
    expand_schema(&schema, input.span())
}

/// Loads one package, tracks it as an input, and expands its schema types.
fn expand_package_artifact(artifact: &NotePackageArtifact, span: Span) -> syn::Result<TokenStream> {
    let types = expand_schema(artifact.schema(), span)?;
    let tracked_path = artifact.path().to_string_lossy();
    Ok(quote! {
        #[doc(hidden)]
        const _: &[u8] = include_bytes!(#tracked_path);
        #types
    })
}

/// Generates host-profile types and records their WIT identities.
fn expand_schema(schema: &NoteStorageSchema, span: Span) -> syn::Result<TokenStream> {
    let generated =
        generate_host_types(schema).map_err(|error| syn::Error::new(span, error.to_string()))?;
    register_schema(schema, span)?;
    let generated = rewrite_runtime_paths(generated.tokens().clone())?;
    let felt_repr_alias = felt_repr_alias();
    Ok(quote! {
        #felt_repr_alias

        #generated
    })
}

/// Provides the fixed crate name used by the felt representation derives.
fn felt_repr_alias() -> TokenStream {
    match crate_name("miden-field-repr") {
        Ok(FoundCrate::Itself) => TokenStream::new(),
        Ok(FoundCrate::Name(name)) if name == "miden_field_repr" => TokenStream::new(),
        Ok(FoundCrate::Name(name)) => {
            let name = syn::Ident::new(&name, Span::call_site());
            quote! {
                #[doc(hidden)]
                extern crate #name as miden_field_repr;
            }
        }
        Err(_) => quote! {
            #[doc(hidden)]
            extern crate miden_note_codec as miden_field_repr;
        },
    }
}

/// Validates and records one marked author codec implementation.
pub(crate) fn note_codec(args: TokenStream, item: ItemImpl) -> syn::Result<TokenStream> {
    if !args.is_empty() {
        return Err(syn::Error::new_spanned(args, "#[note_codec] does not accept arguments"));
    }
    let (_, trait_path, _) = item.trait_.as_ref().ok_or_else(|| {
        syn::Error::new_spanned(&item.self_ty, "#[note_codec] needs an AuthorTypeCodec impl")
    })?;
    if !trait_path
        .segments
        .last()
        .is_some_and(|segment| segment.ident == "AuthorTypeCodec")
    {
        return Err(syn::Error::new_spanned(
            trait_path,
            "#[note_codec] can only mark an AuthorTypeCodec impl",
        ));
    }
    if !item.generics.params.is_empty() {
        return Err(syn::Error::new_spanned(
            &item.generics,
            "#[note_codec] does not support generic implementations",
        ));
    }
    let rust_name = rust_type_name(&item.self_ty)?;
    register_codec(&rust_name, item.self_ty.to_token_stream().to_string(), item.self_ty.span())?;
    Ok(quote!(#item))
}

/// Generates native dispatch and Wasm-only component export glue.
pub(crate) fn export_codecs(input: TokenStream) -> syn::Result<TokenStream> {
    if !input.is_empty() {
        return Err(syn::Error::new_spanned(input, "export_codecs! does not accept arguments"));
    }
    let codecs = registered_codecs(Span::call_site())?;
    let registrations = codecs
        .iter()
        .map(|codec| {
            let fqn = &codec.fqn;
            let ty = syn::parse_str::<Type>(&codec.rust_type).map_err(|error| {
                syn::Error::new(
                    Span::call_site(),
                    format!("failed to restore codec type `{}`: {error}", codec.rust_type),
                )
            })?;
            Ok((fqn, ty))
        })
        .collect::<syn::Result<Vec<_>>>()?;
    let fqns = registrations.iter().map(|(fqn, _)| fqn).collect::<Vec<_>>();
    let parse_arms = registrations.iter().map(|(fqn, ty)| {
        quote! {
            #fqn => {
                let value = <#ty as ::miden_note_codec::AuthorTypeCodec>::parse(value)?;
                let mut felts = Vec::new();
                <#ty as __MidenNoteEncode>::__write_note_felts(
                    &value,
                    &mut ::miden_note_codec::__private::miden_field_repr::FeltWriter::new(
                        &mut felts,
                    ),
                )
                .map_err(|error| format!(
                    "failed to encode codec type `{}`: {error}",
                    #fqn,
                ))?;
                Ok(::miden_note_codec::felts_to_u64(&felts))
            }
        }
    });
    let display_arms = registrations.iter().map(|(fqn, ty)| {
        quote! {
            #fqn => {
                let felts = ::miden_note_codec::felts_from_u64(value)?;
                let mut reader =
                    ::miden_note_codec::__private::miden_field_repr::FeltReader::new(&felts);
                let value = <#ty as __MidenNoteDecode>::__read_note_felts(&mut reader)
                    .map_err(|error| format!(
                        "failed to decode codec type `{}`: {error}",
                        #fqn,
                    ))?;
                reader.ensure_eof().map_err(|error| format!(
                    "codec type `{}` has trailing felt data: {error}",
                    #fqn,
                ))?;
                Ok(<#ty as ::miden_note_codec::AuthorTypeCodec>::display(&value))
            }
        }
    });
    let validate_arms = registrations.iter().map(|(fqn, ty)| {
        quote! {
            #fqn => {
                let felts = ::miden_note_codec::felts_from_u64(value)?;
                let mut reader =
                    ::miden_note_codec::__private::miden_field_repr::FeltReader::new(&felts);
                let value = <#ty as __MidenNoteDecode>::__read_note_felts(&mut reader)
                    .map_err(|error| format!(
                        "failed to decode codec type `{}`: {error}",
                        #fqn,
                    ))?;
                reader.ensure_eof().map_err(|error| format!(
                    "codec type `{}` has trailing felt data: {error}",
                    #fqn,
                ))?;
                <#ty as ::miden_note_codec::AuthorTypeCodec>::validate(&value)
            }
        }
    });
    let wit = NOTE_CODEC_WIT;

    Ok(quote! {
        /// Native dispatch used by the note codec component adapter.
        #[doc(hidden)]
        pub mod __miden_note_codec_dispatch {
            use super::*;

            /// Returns all supported canonical WIT FQNs.
            pub fn supported_types() -> Vec<String> {
                vec![#(#fqns.to_owned()),*]
            }

            /// Parses one value through its marked author codec.
            pub fn parse(type_fqn: &str, value: &str) -> Result<Vec<u64>, String> {
                match type_fqn {
                    #(#parse_arms,)*
                    _ => Err(format!(
                        "no note codec is registered for WIT type `{type_fqn}`"
                    )),
                }
            }

            /// Displays one value through its marked author codec.
            pub fn display(type_fqn: &str, value: &[u64]) -> Result<String, String> {
                match type_fqn {
                    #(#display_arms,)*
                    _ => Err(format!(
                        "no note codec is registered for WIT type `{type_fqn}`"
                    )),
                }
            }

            /// Validates one value through its marked author codec.
            pub fn validate(type_fqn: &str, value: &[u64]) -> Result<(), String> {
                match type_fqn {
                    #(#validate_arms,)*
                    _ => Err(format!(
                        "no note codec is registered for WIT type `{type_fqn}`"
                    )),
                }
            }
        }

        #[cfg(target_family = "wasm")]
        mod __miden_note_codec_component {
            ::miden_note_codec::__private::wit_bindgen::generate!({
                inline: #wit,
                world: "note-codec",
                runtime_path: "::miden_note_codec::__private::wit_bindgen::rt",
            });

            struct Component;

            impl exports::miden::note_codec::codec::Guest for Component {
                fn supported_types() -> Vec<String> {
                    super::__miden_note_codec_dispatch::supported_types()
                }

                fn parse(type_fqn: String, value: String) -> Result<Vec<u64>, String> {
                    super::__miden_note_codec_dispatch::parse(&type_fqn, &value)
                }

                fn display(type_fqn: String, value: Vec<u64>) -> Result<String, String> {
                    super::__miden_note_codec_dispatch::display(&type_fqn, &value)
                }

                fn validate(type_fqn: String, value: Vec<u64>) -> Result<(), String> {
                    super::__miden_note_codec_dispatch::validate(&type_fqn, &value)
                }
            }

            export!(Component);
        }
    })
}

/// Returns the final identifier of one concrete impl self type.
fn rust_type_name(ty: &Type) -> syn::Result<String> {
    let Type::Path(path) = ty else {
        return Err(syn::Error::new_spanned(ty, "#[note_codec] needs a concrete generated type"));
    };
    path.path
        .segments
        .last()
        .map(|segment| segment.ident.to_string())
        .ok_or_else(|| syn::Error::new_spanned(ty, "#[note_codec] type path is empty"))
}

/// Rewrites generated runtime paths through `miden-note-codec` re-exports.
fn rewrite_runtime_paths(tokens: TokenStream) -> syn::Result<TokenStream> {
    let mut file = syn::parse2::<syn::File>(tokens)?;
    RuntimePathRewriter.visit_file_mut(&mut file);
    Ok(quote!(#file))
}

/// Rewrites the four runtime crates referenced by shared host-profile codegen.
struct RuntimePathRewriter;

impl VisitMut for RuntimePathRewriter {
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
        let mut rewritten: syn::Path =
            syn::parse_quote!(::miden_note_codec::__private::#crate_name);
        rewritten.segments.extend(tail);
        *path = rewritten;
    }
}

use syn::spanned::Spanned;
