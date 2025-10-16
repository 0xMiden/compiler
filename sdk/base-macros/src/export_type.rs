use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, ItemStruct};

use crate::types::{exported_type_from_struct, register_export_type};

pub(crate) fn expand(attr: TokenStream, item: TokenStream) -> TokenStream {
    if !attr.is_empty() {
        return syn::Error::new_spanned(
            proc_macro2::TokenStream::from(attr),
            "#[export_type] does not accept arguments",
        )
        .into_compile_error()
        .into();
    }

    let item_struct = parse_macro_input!(item as ItemStruct);

    let span = item_struct.ident.span();
    match exported_type_from_struct(&item_struct) {
        Ok(def) => {
            if let Err(err) = register_export_type(def, span) {
                err.to_compile_error().into()
            } else {
                quote! {
                    #item_struct
                }
                .into()
            }
        }
        Err(err) => err.to_compile_error().into(),
    }
}
