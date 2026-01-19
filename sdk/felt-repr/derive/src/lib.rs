//! Derive macros for felt representation serialization/deserialization.
//!
//! This crate provides proc-macros used by `miden-felt-repr` to derive `ToFeltRepr`/`FromFeltRepr`
//! implementations for user-defined types.
//!
//! # Usage
//!
//! This crate is not typically used directly. Instead, depend on `miden-felt-repr` and derive the
//! traits re-exported by that crate.
//!
//! ## Struct example
//!
//! ```ignore
//! use miden_felt_repr::{FromFeltRepr, ToFeltRepr};
//! use miden_core::Felt;
//!
//! #[derive(Debug, PartialEq, Eq, FromFeltRepr, ToFeltRepr)]
//! struct AccountId {
//!     prefix: Felt,
//!     suffix: Felt,
//! }
//!
//! let value = AccountId { prefix: Felt::new(1), suffix: Felt::new(2) };
//! let felts = value.to_felt_repr();
//! let roundtrip = AccountId::from(felts.as_slice());
//! assert_eq!(roundtrip, value);
//! ```
//!
//! ## Enum example
//!
//! ```ignore
//! use miden_felt_repr::{FromFeltRepr, ToFeltRepr};
//! use miden_core::Felt;
//!
//! #[derive(Debug, PartialEq, Eq, FromFeltRepr, ToFeltRepr)]
//! enum Message {
//!     Ping,
//!     Transfer { to: Felt, amount: u32 },
//! }
//!
//! // Encoded as: [tag, payload...], where `tag` is the variant ordinal in declaration order.
//! // Ping -> tag = 0
//! // Transfer -> tag = 1
//! let value = Message::Transfer { to: Felt::new(7), amount: 10 };
//! let felts = value.to_felt_repr();
//! let roundtrip = Message::from(felts.as_slice());
//! assert_eq!(roundtrip, value);
//! ```
//!
//! # Felt-repr format
//!
//! The *felt representation* of a value is a flat sequence of field elements (`Felt`). The format
//! is intentionally simple: it is just a concatenation of the encodings of each component, with no
//! self-describing schema, no field names, and no length prefixes unless the type itself contains
//! them.
//!
//! ## Primitives
//!
//! The following primitive encodings are provided by the runtime crates:
//!
//! - `Felt`: encoded as a single `Felt`
//! - `u64`: encoded as 2 `Felt`s (low `u32`, then high `u32`)
//! - `u32`, `u8`: encoded as a single `Felt`
//! - `bool`: encoded as a single `Felt` (`0` = `false`, non-zero = `true`)
//!
//! ## Structs
//!
//! Named-field structs are encoded by serializing fields in *declaration order*:
//!
//! `struct S { a: A, b: B }` → `A` then `B`
//!
//! Tuple structs are encoded by serializing fields left-to-right:
//!
//! `struct T(A, B)` → `A` then `B`
//!
//! Important: the field order is part of the wire format. Reordering fields (or inserting a field
//! in the middle) changes the encoding and will break compatibility with existing data.
//!
//! Current limitations:
//! - Unit structs are not supported.
//!
//! ## Enums
//!
//! Enums are encoded as:
//!
//! `tag: u32` (variant ordinal, starting at `0`, in *declaration order*) followed by the selected
//! variant payload (if any), encoded in declaration order.
//!
//! - Unit variants add no payload.
//! - Tuple variants serialize their fields left-to-right.
//! - Struct variants serialize their named fields in declaration order.
//!
//! Important: the **variant order is part of the wire format**. Reordering variants (or inserting
//! a new variant before existing ones) changes the tag values and will break compatibility.
//!
//! Current limitations:
//! - Explicit discriminants are not supported (e.g. `Foo = 10`); tags are always ordinals.
//!
//! ## Nesting
//!
//! Struct/enum fields may themselves be structs/enums (or other types) that implement
//! `ToFeltRepr`/`FromFeltRepr`. The overall encoding is always the concatenation of the nested
//! encodings.
//!
//! ## Unsupported items
//!
//! - Unions are not supported.
//!
//! ## Compatibility note
//!
//! Since the format is not self-describing, keeping field/variant order stable is required for
//! forward/backward compatibility. If you need evolution, introduce an explicit version field or a
//! dedicated schema layer on top.

#![deny(warnings)]

extern crate proc_macro;

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::{
    Data, DeriveInput, Error, Field, Fields, Index, Variant, parse_macro_input,
    punctuated::Punctuated, spanned::Spanned, token::Comma,
};

/// Field list extracted from a struct, either named or tuple-style.
enum StructFields<'a> {
    Named(&'a Punctuated<Field, Comma>),
    Unnamed(&'a Punctuated<Field, Comma>),
}

/// Extracts fields from a struct, returning an error for unsupported items.
fn extract_struct_fields<'a>(
    input: &'a DeriveInput,
    trait_name: &str,
) -> Result<StructFields<'a>, Error> {
    let name = &input.ident;
    match &input.data {
        Data::Struct(data) => match &data.fields {
            Fields::Named(fields) => Ok(StructFields::Named(&fields.named)),
            Fields::Unnamed(fields) => Ok(StructFields::Unnamed(&fields.unnamed)),
            Fields::Unit => Err(Error::new(
                input.span(),
                format!("{trait_name} cannot be derived for unit struct `{name}`"),
            )),
        },
        Data::Enum(_) => Err(Error::new(input.span(), enum_mismatch_msg(trait_name, name))),
        Data::Union(_) => Err(Error::new(
            input.span(),
            format!("{trait_name} cannot be derived for union `{name}`"),
        )),
    }
}

/// Extracts variants from an enum, returning an error for unsupported items.
fn extract_enum_variants<'a>(
    input: &'a DeriveInput,
    trait_name: &str,
) -> Result<&'a Punctuated<Variant, Comma>, Error> {
    let name = &input.ident;
    match &input.data {
        Data::Enum(data) => Ok(&data.variants),
        Data::Struct(_) => Err(Error::new(input.span(), struct_mismatch_msg(trait_name, name))),
        Data::Union(_) => Err(Error::new(
            input.span(),
            format!("{trait_name} cannot be derived for union `{name}`"),
        )),
    }
}

fn struct_mismatch_msg(trait_name: &str, name: &syn::Ident) -> String {
    format!("{trait_name} cannot be derived for struct `{name}`")
}

fn enum_mismatch_msg(trait_name: &str, name: &syn::Ident) -> String {
    format!("{trait_name} cannot be derived for enum `{name}`")
}

/// Validates that an enum does not use explicit discriminants.
fn ensure_no_explicit_discriminants(
    variants: &Punctuated<Variant, Comma>,
    trait_name: &str,
    enum_name: &syn::Ident,
) -> Result<(), Error> {
    for variant in variants {
        if variant.discriminant.is_some() {
            return Err(Error::new(
                variant.span(),
                format!(
                    "{trait_name} cannot be derived for enum `{enum_name}` with explicit \
                     discriminants"
                ),
            ));
        }
    }
    Ok(())
}

/// Derives `FromFeltRepr` for `miden-felt-repr` for a struct with named fields, or an enum.
///
/// Structs are encoded by serializing their fields in declaration order.
///
/// Enums are encoded as a `u32` tag (variant ordinal, starting from `0`)
/// followed by the selected variant payload encoded in declaration order.
///
/// # Example
///
/// ```ignore
/// use miden_felt_repr::FromFeltRepr;
///
/// #[derive(FromFeltRepr)]
/// pub struct AccountId {
///     pub prefix: Felt,
///     pub suffix: Felt,
/// }
/// ```
#[proc_macro_derive(DeriveFromFeltRepr)]
pub fn derive_from_felt_repr(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    let expanded =
        derive_from_felt_repr_impl(&input, quote!(miden_felt_repr), quote!(miden_felt_repr::Felt));
    match expanded {
        Ok(ts) => ts,
        Err(err) => err.into_compile_error().into(),
    }
}

fn derive_from_felt_repr_impl(
    input: &DeriveInput,
    felt_repr_crate: TokenStream2,
    felt_ty: TokenStream2,
) -> Result<TokenStream, Error> {
    let name = &input.ident;
    let generics = &input.generics;
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

    let trait_name = "FromFeltRepr";
    let expanded = match &input.data {
        Data::Struct(_) => match extract_struct_fields(input, trait_name)? {
            StructFields::Named(fields) => {
                let field_names: Vec<_> =
                    fields.iter().map(|field| field.ident.as_ref().unwrap()).collect();
                let field_types: Vec<_> = fields.iter().map(|field| &field.ty).collect();
                quote! {
                    impl #impl_generics #felt_repr_crate::FromFeltRepr for #name #ty_generics #where_clause {
                        #[inline(always)]
                        fn from_felt_repr(reader: &mut #felt_repr_crate::FeltReader<'_>) -> Self {
                            Self {
                                #(#field_names: <#field_types as #felt_repr_crate::FromFeltRepr>::from_felt_repr(reader)),*
                            }
                        }
                    }
                }
            }
            StructFields::Unnamed(fields) => {
                let field_types: Vec<_> = fields.iter().map(|field| &field.ty).collect();
                let reads = field_types.iter().map(|ty| {
                    quote! { <#ty as #felt_repr_crate::FromFeltRepr>::from_felt_repr(reader) }
                });
                quote! {
                    impl #impl_generics #felt_repr_crate::FromFeltRepr for #name #ty_generics #where_clause {
                        #[inline(always)]
                        fn from_felt_repr(reader: &mut #felt_repr_crate::FeltReader<'_>) -> Self {
                            Self(#(#reads),*)
                        }
                    }
                }
            }
        },
        Data::Enum(_) => {
            let variants = extract_enum_variants(input, trait_name)?;
            ensure_no_explicit_discriminants(variants, trait_name, name)?;

            let arms = variants.iter().enumerate().map(|(variant_ordinal, variant)| {
                let variant_ident = &variant.ident;
                let tag = variant_ordinal as u32;
                match &variant.fields {
                    Fields::Unit => quote! { #tag => Self::#variant_ident },
                    Fields::Unnamed(fields) => {
                        let field_types: Vec<_> = fields.unnamed.iter().map(|f| &f.ty).collect();
                        let reads = field_types.iter().map(|ty| {
                            quote! { <#ty as #felt_repr_crate::FromFeltRepr>::from_felt_repr(reader) }
                        });
                        quote! { #tag => Self::#variant_ident(#(#reads),*) }
                    }
                    Fields::Named(fields) => {
                        let field_idents: Vec<_> = fields
                            .named
                            .iter()
                            .map(|f| f.ident.as_ref().expect("named field"))
                            .collect();
                        let field_types: Vec<_> = fields.named.iter().map(|f| &f.ty).collect();
                        let reads = field_idents.iter().zip(field_types.iter()).map(|(ident, ty)| {
                            quote! { #ident: <#ty as #felt_repr_crate::FromFeltRepr>::from_felt_repr(reader) }
                        });
                        quote! { #tag => Self::#variant_ident { #(#reads),* } }
                    }
                }
            });

            quote! {
                impl #impl_generics #felt_repr_crate::FromFeltRepr for #name #ty_generics #where_clause {
                    #[inline(always)]
                    fn from_felt_repr(reader: &mut #felt_repr_crate::FeltReader<'_>) -> Self {
                        let tag: u32 = <u32 as #felt_repr_crate::FromFeltRepr>::from_felt_repr(reader);
                        match tag {
                            #(#arms,)*
                            other => panic!("Unknown `{}` enum variant tag: {}", stringify!(#name), other),
                        }
                    }
                }
            }
        }
        Data::Union(_) => {
            return Err(Error::new(
                input.span(),
                format!("{trait_name} cannot be derived for union `{name}`"),
            ));
        }
    };

    let expanded = quote! {
        #expanded

        impl #impl_generics From<&[#felt_ty]> for #name #ty_generics #where_clause {
            #[inline(always)]
            fn from(felts: &[#felt_ty]) -> Self {
                let mut reader = #felt_repr_crate::FeltReader::new(felts);
                <Self as #felt_repr_crate::FromFeltRepr>::from_felt_repr(&mut reader)
            }
        }
    };

    Ok(expanded.into())
}

/// Derives `ToFeltRepr` trait for a struct with named fields, or an enum.
///
/// Structs are encoded by serializing their fields in declaration order.
///
/// Enums are encoded as a `u32` tag (variant ordinal, starting from `0`)
/// followed by the selected variant payload encoded in declaration order.
///
/// # Example
///
/// ```ignore
/// use miden_felt_repr::ToFeltRepr;
///
/// #[derive(ToFeltRepr)]
/// pub struct AccountId {
///     pub prefix: Felt,
///     pub suffix: Felt,
/// }
/// ```
#[proc_macro_derive(DeriveToFeltRepr)]
pub fn derive_to_felt_repr(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    match derive_to_felt_repr_impl(&input, quote!(miden_felt_repr)) {
        Ok(ts) => ts,
        Err(err) => err.into_compile_error().into(),
    }
}

fn derive_to_felt_repr_impl(
    input: &DeriveInput,
    felt_repr_crate: TokenStream2,
) -> Result<TokenStream, Error> {
    let name = &input.ident;
    let generics = &input.generics;
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

    let trait_name = "ToFeltRepr";
    let expanded = match &input.data {
        Data::Struct(_) => match extract_struct_fields(input, trait_name)? {
            StructFields::Named(fields) => {
                let field_names: Vec<_> =
                    fields.iter().map(|field| field.ident.as_ref().unwrap()).collect();
                quote! {
                    impl #impl_generics #felt_repr_crate::ToFeltRepr for #name #ty_generics #where_clause {
                        fn write_felt_repr(&self, writer: &mut #felt_repr_crate::FeltWriter<'_>) {
                            #(#felt_repr_crate::ToFeltRepr::write_felt_repr(&self.#field_names, writer);)*
                        }
                    }
                }
            }
            StructFields::Unnamed(fields) => {
                let field_indexes: Vec<Index> = (0..fields.len()).map(Index::from).collect();
                quote! {
                    impl #impl_generics #felt_repr_crate::ToFeltRepr for #name #ty_generics #where_clause {
                        fn write_felt_repr(&self, writer: &mut #felt_repr_crate::FeltWriter<'_>) {
                            #(#felt_repr_crate::ToFeltRepr::write_felt_repr(&self.#field_indexes, writer);)*
                        }
                    }
                }
            }
        },
        Data::Enum(_) => {
            let variants = extract_enum_variants(input, trait_name)?;
            ensure_no_explicit_discriminants(variants, trait_name, name)?;

            let arms = variants.iter().enumerate().map(|(variant_ordinal, variant)| {
                let variant_ident = &variant.ident;
                let tag = variant_ordinal as u32;

                match &variant.fields {
                    Fields::Unit => quote! {
                        Self::#variant_ident => {
                            #felt_repr_crate::ToFeltRepr::write_felt_repr(&(#tag as u32), writer);
                            return;
                        }
                    },
                    Fields::Unnamed(fields) => {
                        let bindings: Vec<_> = (0..fields.unnamed.len())
                            .map(|i| format_ident!("__field{i}"))
                            .collect();
                        quote! {
                            Self::#variant_ident(#(#bindings),*) => {
                                #felt_repr_crate::ToFeltRepr::write_felt_repr(&(#tag as u32), writer);
                                #(#felt_repr_crate::ToFeltRepr::write_felt_repr(#bindings, writer);)*
                                return;
                            }
                        }
                    }
                    Fields::Named(fields) => {
                        let bindings: Vec<_> = fields
                            .named
                            .iter()
                            .map(|f| f.ident.as_ref().expect("named field"))
                            .collect();
                        quote! {
                            Self::#variant_ident { #(#bindings),* } => {
                                #felt_repr_crate::ToFeltRepr::write_felt_repr(&(#tag as u32), writer);
                                #(#felt_repr_crate::ToFeltRepr::write_felt_repr(#bindings, writer);)*
                                return;
                            }
                        }
                    }
                }
            });

            quote! {
                impl #impl_generics #felt_repr_crate::ToFeltRepr for #name #ty_generics #where_clause {
                    #[inline(always)]
                    fn write_felt_repr(&self, writer: &mut #felt_repr_crate::FeltWriter<'_>) {
                        match self {
                            #(#arms,)*
                        }
                    }
                }
            }
        }
        Data::Union(_) => {
            return Err(Error::new(
                input.span(),
                format!("{trait_name} cannot be derived for union `{name}`"),
            ));
        }
    };

    Ok(expanded.into())
}
