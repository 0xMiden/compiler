//! Typed bindings for account storage schemas: the custom slot types and a `Storage` root with
//! one field per slot.

use miden_account_storage_schema::{AccountStorageSchema, StorageSlotKind};
use miden_schema_codegen::{RuntimePaths, generate_named_types, generated_value_ident};
use proc_macro2::{Literal, Span, TokenStream as TokenStream2};
use quote::{format_ident, quote};

/// The name of the generated root type.
const ROOT_TYPE: &str = "Storage";

/// Generates the custom slot types and the `Storage` root of an account storage schema.
///
/// `metadata` is the encoded protocol component metadata of the package; the generated
/// `Storage::schema` reads the schema again from it and the WIT, so the slots keep their full
/// names at run time.
pub(crate) fn expand_storage_schema(
    schema: &AccountStorageSchema,
    metadata: &[u8],
    facade_path: &syn::Path,
    span: Span,
    scope_ident: &syn::Ident,
) -> syn::Result<TokenStream2> {
    let error = |message: String| syn::Error::new(span, message);
    let runtime_paths = RuntimePaths::through_facade(quote!(#facade_path));
    let roots = schema
        .slots()
        .iter()
        .flat_map(|slot| match slot.kind() {
            StorageSlotKind::Value(ty) => vec![ty.as_ref()],
            StorageSlotKind::Map { key, value } => vec![key.as_ref(), value.as_ref()],
        })
        .collect::<Vec<_>>();
    let generated = generate_named_types(&roots, &runtime_paths)
        .map_err(|codegen| error(codegen.to_string()))?;
    let type_idents = generated.type_idents();
    if type_idents.iter().any(|ident| ident == ROOT_TYPE) {
        return Err(error(format!(
            "a custom type of the account storage schema maps to Rust type `{ROOT_TYPE}`, which \
             the bindings use for the storage root; rename the type"
        )));
    }

    let runtime = quote!(#facade_path::__private);
    let storage_schema = quote!(#runtime::miden_account_storage_schema);
    let mut fields = Vec::new();
    let mut consts = Vec::new();
    let mut init_steps = Vec::new();
    let mut decoders = Vec::new();
    for slot in schema.slots() {
        let leaf = slot.leaf();
        if leaf == "storage" {
            return Err(error(
                "account storage slot `storage` collides with the `Storage` root of the bindings; \
                 rename the slot"
                    .to_owned(),
            ));
        }
        let name = slot.name().ok_or_else(|| {
            error(format!("account storage slot `{leaf}` has no protocol slot name"))
        })?;
        let field = generated_value_ident(leaf);
        let const_ident = format_ident!("{}_SLOT", leaf.to_uppercase());
        let const_docs = format!("The full protocol name of the `{leaf}` slot.");
        consts.push(quote! {
            #[doc = #const_docs]
            pub const #const_ident: &'static str = #name;
        });
        match slot.kind() {
            StorageSlotKind::Value(ty) => {
                let ty = generated.rust_type(ty).map_err(|codegen| error(codegen.to_string()))?;
                let docs = slot_docs(slot.docs(), &format!("The value of the `{leaf}` slot."));
                fields.push(quote! {
                    #docs
                    pub #field: #ty,
                });
                init_steps.push(quote! {
                    builder = builder.set_word(#leaf, __to_word(&self.#field)?)?;
                });
                let decoder = format_ident!("decode_{leaf}");
                let decoder_docs = format!(
                    "Decodes a word of the `{leaf}` slot; the felts after the value must be zero."
                );
                decoders.push(quote! {
                    #[doc = #decoder_docs]
                    pub fn #decoder(word: #storage_schema::Word) -> #storage_schema::Result<#ty> {
                        __from_word(word)
                    }
                });
            }
            StorageSlotKind::Map { key, value } => {
                let key = generated.rust_type(key).map_err(|codegen| error(codegen.to_string()))?;
                let value =
                    generated.rust_type(value).map_err(|codegen| error(codegen.to_string()))?;
                let docs =
                    slot_docs(slot.docs(), &format!("The entries of the `{leaf}` map slot."));
                fields.push(quote! {
                    #docs
                    pub #field: ::std::vec::Vec<(#key, #value)>,
                });
                init_steps.push(quote! {
                    for (key, value) in &self.#field {
                        builder = builder.set_entry_words(
                            #leaf,
                            __to_word(key)?,
                            __to_word(value)?,
                        )?;
                    }
                });
                let decoder = format_ident!("decode_{leaf}_entry");
                let decoder_docs = format!(
                    "Decodes the key and value words of one `{leaf}` map entry; the felts after \
                     each must be zero."
                );
                decoders.push(quote! {
                    #[doc = #decoder_docs]
                    pub fn #decoder(
                        key: #storage_schema::Word,
                        value: #storage_schema::Word,
                    ) -> #storage_schema::Result<(#key, #value)> {
                        Ok((__from_word(key)?, __from_word(value)?))
                    }
                });
            }
        }
    }

    let type_tokens = generated.tokens();
    let wit_text = schema.wit_text();
    let metadata = Literal::byte_string(metadata);
    let root_ident = format_ident!("{ROOT_TYPE}");

    Ok(quote! {
        #[doc(hidden)]
        mod #scope_ident {
            #type_tokens

            #[doc(hidden)]
            const __MIDEN_ACCOUNT_STORAGE_SCHEMA_WIT: &str = #wit_text;

            #[doc(hidden)]
            const __MIDEN_ACCOUNT_COMPONENT_METADATA: &[u8] = #metadata;

            /// Packs a typed slot value, map key, or map value into its storage word.
            #[doc(hidden)]
            fn __to_word<T: __MidenSchemaEncode>(
                value: &T,
            ) -> #storage_schema::Result<#storage_schema::Word> {
                let mut felts = ::std::vec::Vec::new();
                value.__write_felts(&mut #runtime::miden_field_repr::FeltWriter::new(&mut felts))?;
                #storage_schema::word_from_felts(&felts)
            }

            /// Reads a typed slot value, map key, or map value from its storage word.
            #[doc(hidden)]
            fn __from_word<T: __MidenSchemaDecode>(
                word: #storage_schema::Word,
            ) -> #storage_schema::Result<T> {
                let felts = #storage_schema::felts_from_word(word);
                let mut reader = #runtime::miden_field_repr::FeltReader::new(&felts);
                let value = T::__read_felts(&mut reader)?;
                // The felts after the value are padding.
                if let Some(index) = felts[reader.pos()..]
                    .iter()
                    .position(|felt| *felt != #runtime::miden_field::Felt::ZERO)
                {
                    return Err(#storage_schema::Error::new(format!(
                        "the word has a nonzero padding felt at index {}",
                        reader.pos() + index
                    )));
                }
                Ok(value)
            }

            /// The initial values of the account component's storage slots, one field per slot.
            #[derive(
                ::core::clone::Clone,
                ::core::fmt::Debug,
                ::core::cmp::PartialEq,
                ::core::cmp::Eq,
            )]
            pub struct #root_ident {
                #(#fields)*
            }

            impl #root_ident {
                #(#consts)*

                /// Returns the resolved schema with full slot names, which is parsed once for the
                /// whole process.
                pub fn schema() -> #storage_schema::Result<
                    &'static #storage_schema::AccountStorageSchema,
                > {
                    static __MIDEN_ACCOUNT_STORAGE_SCHEMA: ::std::sync::OnceLock<
                        #storage_schema::Result<#storage_schema::AccountStorageSchema>,
                    > = ::std::sync::OnceLock::new();
                    __MIDEN_ACCOUNT_STORAGE_SCHEMA
                        .get_or_init(|| {
                            let metadata =
                                <#runtime::miden_protocol::account::AccountComponentMetadata as
                                    #runtime::miden_protocol::utils::serde::Deserializable>
                                    ::read_from_bytes(__MIDEN_ACCOUNT_COMPONENT_METADATA)
                                    .map_err(|error| {
                                        #storage_schema::Error::new(format!(
                                            "failed to decode the component metadata: {error}"
                                        ))
                                    })?;
                            #storage_schema::AccountStorageSchema::from_wit_text_and_metadata(
                                __MIDEN_ACCOUNT_STORAGE_SCHEMA_WIT,
                                &metadata,
                            )
                        })
                        .as_ref()
                        .map_err(::core::clone::Clone::clone)
                }

                /// Encodes every slot as the init storage data of the account component.
                pub fn to_init_storage_data(
                    &self,
                ) -> #storage_schema::Result<#storage_schema::InitStorageData> {
                    let mut builder = Self::schema()?.init_builder();
                    #(#init_steps)*
                    builder.build()
                }

                #(#decoders)*
            }
        }

        pub use #scope_ident::{#root_ident #(, #type_idents)*};
    })
}

/// Returns the docs of a generated `Storage` field: the slot's WIT docs, or `fallback`.
fn slot_docs(docs: Option<&str>, fallback: &str) -> TokenStream2 {
    let docs = docs.unwrap_or(fallback);
    quote!(#[doc = #docs])
}
