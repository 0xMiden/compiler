use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, Item};

use crate::types::{exported_type_from_enum, exported_type_from_struct, register_export_type};

pub(crate) fn expand(attr: TokenStream, item: TokenStream) -> TokenStream {
    if !attr.is_empty() {
        return syn::Error::new_spanned(
            proc_macro2::TokenStream::from(attr),
            "#[export_type] does not accept arguments",
        )
        .into_compile_error()
        .into();
    }

    let item = parse_macro_input!(item as Item);

    match item {
        Item::Struct(item_struct) => {
            let span = item_struct.ident.span();
            match exported_type_from_struct(&item_struct) {
                Ok(def) => match register_export_type(def, span) {
                    Ok(()) => quote! { #item_struct }.into(),
                    Err(err) => err.to_compile_error().into(),
                },
                Err(err) => err.to_compile_error().into(),
            }
        }
        Item::Enum(item_enum) => {
            let span = item_enum.ident.span();
            match exported_type_from_enum(&item_enum) {
                Ok(def) => match register_export_type(def, span) {
                    Ok(()) => quote! { #item_enum }.into(),
                    Err(err) => err.to_compile_error().into(),
                },
                Err(err) => err.to_compile_error().into(),
            }
        }
        other => {
            syn::Error::new_spanned(other, "#[export_type] may only be applied to structs or enums")
                .into_compile_error()
                .into()
        }
    }
}
