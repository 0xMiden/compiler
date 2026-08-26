use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::{Item, parse_macro_input};

use crate::types::{
    ExportedTypeDef, custom_type_shape_assertions, export_type_shape_const,
    exported_type_from_enum, exported_type_from_struct, nominal_type_identity_guards,
    register_export_type, registered_export_type_map,
};

/// Builds the guard and identity items emitted next to one exported type.
fn export_type_identity_items(
    def: &ExportedTypeDef,
    generics: &syn::Generics,
    span: proc_macro2::Span,
) -> Result<TokenStream2, syn::Error> {
    let guards = nominal_type_identity_guards(def, span)?;
    register_export_type(def.clone(), span)?;
    // The registry lookup runs after registration so a self-referential type sees itself.
    let registry = registered_export_type_map();
    let assertions = custom_type_shape_assertions(def, &registry, span)?;
    let shape_const = export_type_shape_const(def, generics, span);
    Ok(quote! { #guards #shape_const #assertions })
}

/// Expands `#[export_type]` and registers the annotated record or enum for schema emission.
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
            match exported_type_from_struct(&item_struct)
                .and_then(|def| export_type_identity_items(&def, &item_struct.generics, span))
            {
                Ok(items) => quote! { #item_struct #items }.into(),
                Err(err) => err.to_compile_error().into(),
            }
        }
        Item::Enum(item_enum) => {
            let span = item_enum.ident.span();
            match exported_type_from_enum(&item_enum)
                .and_then(|def| export_type_identity_items(&def, &item_enum.generics, span))
            {
                Ok(items) => quote! { #item_enum #items }.into(),
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
