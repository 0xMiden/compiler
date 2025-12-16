//! Derive macros for felt representation serialization/deserialization.

#![deny(warnings)]

extern crate proc_macro;

use proc_macro::TokenStream;
use quote::quote;
use syn::{
    parse_macro_input, punctuated::Punctuated, spanned::Spanned, token::Comma, Data, DeriveInput,
    Error, Field, Fields,
};

/// Extracts named fields from a struct, returning an error for unsupported types.
fn extract_named_fields<'a>(
    input: &'a DeriveInput,
    trait_name: &str,
) -> Result<&'a Punctuated<Field, Comma>, Error> {
    match &input.data {
        Data::Struct(data) => match &data.fields {
            Fields::Named(fields) => Ok(&fields.named),
            Fields::Unnamed(_) => Err(Error::new(
                input.span(),
                format!("{trait_name} can only be derived for structs with named fields"),
            )),
            Fields::Unit => Err(Error::new(
                input.span(),
                format!("{trait_name} cannot be derived for unit structs"),
            )),
        },
        Data::Enum(_) => {
            Err(Error::new(input.span(), format!("{trait_name} cannot be derived for enums")))
        }
        Data::Union(_) => {
            Err(Error::new(input.span(), format!("{trait_name} cannot be derived for unions")))
        }
    }
}

/// Derives `FromFeltRepr` trait for a struct with named fields.
///
/// Each field must implement `FromFeltRepr`. Fields are deserialized
/// sequentially from a `FeltReader`, with each field consuming its
/// required elements.
///
/// # Example
///
/// ```ignore
/// use miden_felt_repr_onchain::FromFeltRepr;
///
/// #[derive(FromFeltRepr)]
/// pub struct AccountId {
///     pub prefix: Felt,
///     pub suffix: Felt,
/// }
/// ```
#[proc_macro_derive(DeriveFromFeltReprOnchain)]
pub fn derive_from_felt_repr_onchain(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    match derive_from_felt_repr_impl(&input) {
        Ok(ts) => ts,
        Err(err) => err.into_compile_error().into(),
    }
}

fn derive_from_felt_repr_impl(input: &DeriveInput) -> Result<TokenStream, Error> {
    let name = &input.ident;
    let generics = &input.generics;
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

    let fields = extract_named_fields(input, "FromFeltRepr")?;

    let field_names: Vec<_> = fields.iter().map(|field| field.ident.as_ref().unwrap()).collect();
    let field_types: Vec<_> = fields.iter().map(|field| &field.ty).collect();

    let expanded = quote! {
        impl #impl_generics miden_felt_repr_onchain::FromFeltRepr for #name #ty_generics #where_clause {
            #[inline(always)]
            fn from_felt_repr(reader: &mut miden_felt_repr_onchain::FeltReader<'_>) -> Self {
                Self {
                    #(#field_names: <#field_types as miden_felt_repr_onchain::FromFeltRepr>::from_felt_repr(reader)),*
                }
            }
        }

        impl #impl_generics From<&[miden_stdlib_sys::Felt]> for #name #ty_generics #where_clause {
            #[inline(always)]
            fn from(felts: &[miden_stdlib_sys::Felt]) -> Self {
                let mut reader = miden_felt_repr_onchain::FeltReader::new(felts);
                <Self as miden_felt_repr_onchain::FromFeltRepr>::from_felt_repr(&mut reader)
            }
        }
    };

    Ok(expanded.into())
}

/// Derives `ToFeltRepr` trait (offchain) for a struct with named fields.
///
/// Each field must implement `ToFeltRepr`. Fields are serialized
/// into consecutive elements in the output vector.
///
/// # Example
///
/// ```ignore
/// use miden_felt_repr_offchain::ToFeltRepr;
///
/// #[derive(ToFeltRepr)]
/// pub struct AccountId {
///     pub prefix: Felt,
///     pub suffix: Felt,
/// }
/// ```
#[proc_macro_derive(DeriveToFeltReprOffchain)]
pub fn derive_to_felt_repr_offchain(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    match derive_to_felt_repr_impl(&input) {
        Ok(ts) => ts,
        Err(err) => err.into_compile_error().into(),
    }
}

fn derive_to_felt_repr_impl(input: &DeriveInput) -> Result<TokenStream, Error> {
    let name = &input.ident;
    let generics = &input.generics;
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

    let fields = extract_named_fields(input, "ToFeltRepr")?;

    let field_names: Vec<_> = fields.iter().map(|field| field.ident.as_ref().unwrap()).collect();

    let expanded = quote! {
        impl #impl_generics miden_felt_repr_offchain::ToFeltRepr for #name #ty_generics #where_clause {
            fn write_felt_repr(&self, writer: &mut miden_felt_repr_offchain::FeltWriter<'_>) {
                #(miden_felt_repr_offchain::ToFeltRepr::write_felt_repr(&self.#field_names, writer);)*
            }
        }
    };

    Ok(expanded.into())
}

/// Derives `ToFeltRepr` trait (onchain) for a struct with named fields.
///
/// Each field must implement `ToFeltRepr`. Fields are serialized
/// into consecutive elements in the output vector.
///
/// # Example
///
/// ```ignore
/// use miden_felt_repr_onchain::ToFeltRepr;
///
/// #[derive(ToFeltRepr)]
/// pub struct AccountId {
///     pub prefix: Felt,
///     pub suffix: Felt,
/// }
/// ```
#[proc_macro_derive(DeriveToFeltReprOnchain)]
pub fn derive_to_felt_repr_onchain(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    match derive_to_felt_repr_onchain_impl(&input) {
        Ok(ts) => ts,
        Err(err) => err.into_compile_error().into(),
    }
}

fn derive_to_felt_repr_onchain_impl(input: &DeriveInput) -> Result<TokenStream, Error> {
    let name = &input.ident;
    let generics = &input.generics;
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

    let fields = extract_named_fields(input, "ToFeltRepr")?;

    let field_names: Vec<_> = fields.iter().map(|field| field.ident.as_ref().unwrap()).collect();

    let expanded = quote! {
        impl #impl_generics miden_felt_repr_onchain::ToFeltRepr for #name #ty_generics #where_clause {
            #[inline(always)]
            fn write_felt_repr(&self, writer: &mut miden_felt_repr_onchain::FeltWriter<'_>) {
                #(miden_felt_repr_onchain::ToFeltRepr::write_felt_repr(&self.#field_names, writer);)*
            }
        }
    };

    Ok(expanded.into())
}

/// Derives `FromFeltRepr` trait (offchain) for a struct with named fields.
///
/// Each field must implement `FromFeltRepr`. Fields are deserialized
/// sequentially from a `FeltReader`, with each field consuming its
/// required elements.
///
/// # Example
///
/// ```ignore
/// use miden_felt_repr_offchain::FromFeltRepr;
///
/// #[derive(FromFeltRepr)]
/// pub struct AccountId {
///     pub prefix: Felt,
///     pub suffix: Felt,
/// }
/// ```
#[proc_macro_derive(DeriveFromFeltReprOffchain)]
pub fn derive_from_felt_repr_offchain(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    match derive_from_felt_repr_offchain_impl(&input) {
        Ok(ts) => ts,
        Err(err) => err.into_compile_error().into(),
    }
}

fn derive_from_felt_repr_offchain_impl(input: &DeriveInput) -> Result<TokenStream, Error> {
    let name = &input.ident;
    let generics = &input.generics;
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

    let fields = extract_named_fields(input, "FromFeltRepr")?;

    let field_names: Vec<_> = fields.iter().map(|field| field.ident.as_ref().unwrap()).collect();
    let field_types: Vec<_> = fields.iter().map(|field| &field.ty).collect();

    let expanded = quote! {
        impl #impl_generics miden_felt_repr_offchain::FromFeltRepr for #name #ty_generics #where_clause {
            fn from_felt_repr(reader: &mut miden_felt_repr_offchain::FeltReader<'_>) -> Self {
                Self {
                    #(#field_names: <#field_types as miden_felt_repr_offchain::FromFeltRepr>::from_felt_repr(reader)),*
                }
            }
        }

        impl #impl_generics From<&[miden_core::Felt]> for #name #ty_generics #where_clause {
            fn from(felts: &[miden_core::Felt]) -> Self {
                let mut reader = miden_felt_repr_offchain::FeltReader::new(felts);
                <Self as miden_felt_repr_offchain::FromFeltRepr>::from_felt_repr(&mut reader)
            }
        }
    };

    Ok(expanded.into())
}
