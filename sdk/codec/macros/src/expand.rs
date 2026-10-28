//! Macro expansion for generated author types and component dispatch.

use miden_codec_wit::CODEC_WIT;
use miden_note_schema::NoteStorageSchema;
use miden_schema::{PackageArtifact, PackageResolver, Schema};
use miden_schema_codegen::{RuntimePaths, generate_host_types};
use proc_macro_crate::{FoundCrate, crate_name};
use proc_macro2::{Span, TokenStream};
use quote::{ToTokens, quote};
use syn::{ItemImpl, LitStr, Type, spanned::Spanned};

use crate::registry::{register_codec, register_schema, registered_codecs};

/// The crate name diagnostics use.
const MACRO_CRATE: &str = "miden-codec";

/// Expands a project-relative type generation request.
pub(crate) fn from_project(input: &LitStr) -> syn::Result<TokenStream> {
    let artifact = PackageResolver::new(MACRO_CRATE)
        .from_project(&input.value())
        .map_err(|error| syn::Error::new(input.span(), error.to_string()))?;
    expand_package_artifact(&artifact, input.span())
}

/// Expands an exact package type generation request.
pub(crate) fn from_package(input: &LitStr) -> syn::Result<TokenStream> {
    let artifact = PackageResolver::new(MACRO_CRATE)
        .from_package(&input.value())
        .map_err(|error| syn::Error::new(input.span(), error.to_string()))?;
    expand_package_artifact(&artifact, input.span())
}

/// Expands a WIT string literal for internal tests.
pub(crate) fn from_wit_text(input: &LitStr) -> syn::Result<TokenStream> {
    let schema = NoteStorageSchema::from_wit_text(&input.value())
        .map_err(|error| syn::Error::new(input.span(), error.to_string()))?;
    expand_schema(schema.schema(), input.span())
}

/// Loads one package, tracks it as an input, and expands its schema types.
fn expand_package_artifact(artifact: &PackageArtifact, span: Span) -> syn::Result<TokenStream> {
    let schema = NoteStorageSchema::from_package(artifact.package()).map_err(|error| {
        syn::Error::new(
            span,
            format!(
                "{MACRO_CRATE}: failed to read note storage schema from '{}': {error}",
                artifact.path().display()
            ),
        )
    })?;
    let types = expand_schema(schema.schema(), span)?;
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
        #types
    })
}

/// Generates host-profile types and records their WIT identities.
fn expand_schema(schema: &Schema, span: Span) -> syn::Result<TokenStream> {
    let facade = codec_facade();
    let runtime = RuntimePaths::through_facade(quote!(#facade));
    let generated = generate_host_types(schema, &runtime)
        .map_err(|error| syn::Error::new(span, error.to_string()))?;
    register_schema(schema, span)?;
    Ok(generated.tokens().clone())
}

/// Validates and records one marked author codec implementation.
pub(crate) fn codec(args: TokenStream, item: ItemImpl) -> syn::Result<TokenStream> {
    if !args.is_empty() {
        return Err(syn::Error::new_spanned(args, "#[codec] does not accept arguments"));
    }
    let (_, trait_path, _) = item.trait_.as_ref().ok_or_else(|| {
        syn::Error::new_spanned(&item.self_ty, "#[codec] needs an AuthorTypeCodec impl")
    })?;
    if !trait_path
        .segments
        .last()
        .is_some_and(|segment| segment.ident == "AuthorTypeCodec")
    {
        return Err(syn::Error::new_spanned(
            trait_path,
            "#[codec] can only mark an AuthorTypeCodec impl",
        ));
    }
    if !item.generics.params.is_empty() {
        return Err(syn::Error::new_spanned(
            &item.generics,
            "#[codec] does not support generic implementations",
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
    let facade = codec_facade();
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
                let value = <#ty as #facade::AuthorTypeCodec>::parse(value)?;
                let mut felts = ::std::vec::Vec::new();
                <#ty as __MidenSchemaEncode>::__write_felts(
                    &value,
                    &mut #facade::__private::miden_field_repr::FeltWriter::new(
                        &mut felts,
                    ),
                )
                .map_err(|error| format!(
                    "failed to encode codec type `{}`: {error}",
                    #fqn,
                ))?;
                Ok(#facade::felts_to_u64(&felts))
            }
        }
    });
    let display_arms = registrations.iter().map(|(fqn, ty)| {
        quote! {
            #fqn => {
                let felts = #facade::felts_from_u64(value)?;
                let mut reader =
                    #facade::__private::miden_field_repr::FeltReader::new(&felts);
                let value = <#ty as __MidenSchemaDecode>::__read_felts(&mut reader)
                    .map_err(|error| format!(
                        "failed to decode codec type `{}`: {error}",
                        #fqn,
                    ))?;
                reader.ensure_eof().map_err(|error| format!(
                    "codec type `{}` has trailing felt data: {error}",
                    #fqn,
                ))?;
                Ok(<#ty as #facade::AuthorTypeCodec>::display(&value))
            }
        }
    });
    let validate_arms = registrations.iter().map(|(fqn, ty)| {
        quote! {
            #fqn => {
                let felts = #facade::felts_from_u64(value)?;
                let mut reader =
                    #facade::__private::miden_field_repr::FeltReader::new(&felts);
                let value = <#ty as __MidenSchemaDecode>::__read_felts(&mut reader)
                    .map_err(|error| format!(
                        "failed to decode codec type `{}`: {error}",
                        #fqn,
                    ))?;
                reader.ensure_eof().map_err(|error| format!(
                    "codec type `{}` has trailing felt data: {error}",
                    #fqn,
                ))?;
                <#ty as #facade::AuthorTypeCodec>::validate(&value)
            }
        }
    });
    let wit = CODEC_WIT;
    // `quote` prints `::` paths with spaces around each separator. `generate!` reads this value
    // as one path, so the spaces come out.
    let wit_runtime_path = LitStr::new(
        &format!("{facade}::__private::wit_bindgen::rt", facade = quote!(#facade)).replace(' ', ""),
        Span::call_site(),
    );

    Ok(quote! {
        /// Native dispatch used by the codec component adapter.
        #[doc(hidden)]
        pub mod __miden_codec_dispatch {
            use super::*;

            /// Returns all supported canonical WIT FQNs.
            pub fn supported_types() -> ::std::vec::Vec<::std::string::String> {
                vec![#(#fqns.to_owned()),*]
            }

            /// Parses one value through its marked author codec.
            pub fn parse(
                type_fqn: &str,
                value: &str,
            ) -> ::core::result::Result<::std::vec::Vec<u64>, ::std::string::String> {
                match type_fqn {
                    #(#parse_arms,)*
                    _ => Err(format!(
                        "no codec is registered for WIT type `{type_fqn}`"
                    )),
                }
            }

            /// Displays one value through its marked author codec.
            pub fn display(
                type_fqn: &str,
                value: &[u64],
            ) -> ::core::result::Result<::std::string::String, ::std::string::String> {
                match type_fqn {
                    #(#display_arms,)*
                    _ => Err(format!(
                        "no codec is registered for WIT type `{type_fqn}`"
                    )),
                }
            }

            /// Validates one value through its marked author codec.
            pub fn validate(
                type_fqn: &str,
                value: &[u64],
            ) -> ::core::result::Result<(), ::std::string::String> {
                match type_fqn {
                    #(#validate_arms,)*
                    _ => Err(format!(
                        "no codec is registered for WIT type `{type_fqn}`"
                    )),
                }
            }
        }

        #[cfg(target_family = "wasm")]
        mod __miden_codec_component {
            #facade::__private::wit_bindgen::generate!({
                inline: #wit,
                world: "codec-component",
                runtime_path: #wit_runtime_path,
            });

            struct Component;

            impl exports::miden::codec::codec::Guest for Component {
                fn supported_types() -> Vec<String> {
                    super::__miden_codec_dispatch::supported_types()
                }

                fn parse(type_fqn: String, value: String) -> Result<Vec<u64>, String> {
                    super::__miden_codec_dispatch::parse(&type_fqn, &value)
                }

                fn display(type_fqn: String, value: Vec<u64>) -> Result<String, String> {
                    super::__miden_codec_dispatch::display(&type_fqn, &value)
                }

                fn validate(type_fqn: String, value: Vec<u64>) -> Result<(), String> {
                    super::__miden_codec_dispatch::validate(&type_fqn, &value)
                }
            }

            export!(Component);
        }
    })
}

/// Returns the final identifier of one concrete impl self type.
fn rust_type_name(ty: &Type) -> syn::Result<String> {
    let Type::Path(path) = ty else {
        return Err(syn::Error::new_spanned(ty, "#[codec] needs a concrete generated type"));
    };
    path.path
        .segments
        .last()
        .map(|segment| segment.ident.to_string())
        .ok_or_else(|| syn::Error::new_spanned(ty, "#[codec] type path is empty"))
}

/// Resolves the codec facade path in the consuming crate.
fn codec_facade() -> syn::Path {
    match crate_name(MACRO_CRATE) {
        Ok(FoundCrate::Itself) => syn::parse_quote!(crate),
        Ok(FoundCrate::Name(name)) => {
            let ident = syn::Ident::new(&name, Span::call_site());
            syn::parse_quote!(::#ident)
        }
        Err(_) => syn::parse_quote!(::miden_codec),
    }
}
