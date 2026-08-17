use proc_macro::TokenStream;
use quote::quote;
use syn::{Item, parse_macro_input};

use crate::types::{
    exported_type_from_enum, exported_type_from_struct, register_export_type,
    sdk_core_type_identity_guards,
};

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
                Ok(def) => match sdk_core_type_identity_guards(&def, span)
                    .and_then(|guards| register_export_type(def, span).map(|()| guards))
                {
                    Ok(guards) => quote! { #item_struct #guards }.into(),
                    Err(err) => err.to_compile_error().into(),
                },
                Err(err) => err.to_compile_error().into(),
            }
        }
        Item::Enum(item_enum) => {
            let span = item_enum.ident.span();
            match exported_type_from_enum(&item_enum) {
                Ok(def) => match sdk_core_type_identity_guards(&def, span)
                    .and_then(|guards| register_export_type(def, span).map(|()| guards))
                {
                    Ok(guards) => quote! { #item_enum #guards }.into(),
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
