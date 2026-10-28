//! Type export and identity guard expansion for `#[export_type]`.

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::{Item, parse_macro_input};

use crate::{
    types::{
        ExportedTypeDef, export_type_shape_metadata, exported_type_from_enum,
        exported_type_from_struct, known_custom_type_shape_assertions,
        nominal_type_identity_guards, register_export_type, registered_export_type_map,
    },
    util::base_macros_derive_path,
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
    let assertions = known_custom_type_shape_assertions(def, &registry, span)?;
    let shape_const = export_type_shape_metadata(def, generics, span)?;
    Ok(quote! { #guards #shape_const #assertions })
}

/// Collects the last path segment of every trait in the item's `#[derive(...)]` attributes.
///
/// Only the attributes written below `#[export_type]` are visible here: rustc strips a `derive`
/// attribute that precedes an attribute macro before invoking it.
fn derived_trait_names(attrs: &[syn::Attribute]) -> Vec<String> {
    let mut names = Vec::new();
    for attr in attrs.iter().filter(|attr| attr.path().is_ident("derive")) {
        let Ok(paths) = attr.parse_args_with(
            syn::punctuated::Punctuated::<syn::Path, syn::Token![,]>::parse_terminated,
        ) else {
            continue;
        };
        names.extend(
            paths
                .iter()
                .filter_map(|path| path.segments.last())
                .map(|s| s.ident.to_string()),
        );
    }
    names
}

/// Builds the account storage impls for an exported type that derives the felt-repr traits.
///
/// A type that derives both `ToFeltRepr` and `FromFeltRepr` is stored as one zero-padded word,
/// so it gets `WordValue`; when it also derives `Copy` it can be a map key and gets `WordKey`.
/// A type without the felt-repr derives gets nothing: it is a WIT interface type only.
fn storage_impls(
    ident: &syn::Ident,
    generics: &syn::Generics,
    attrs: &[syn::Attribute],
) -> TokenStream2 {
    let derives = derived_trait_names(attrs);
    let has = |name: &str| derives.iter().any(|derive| derive == name);
    if !generics.params.is_empty() || !has("ToFeltRepr") || !has("FromFeltRepr") {
        return TokenStream2::new();
    }
    let width_message = format!(
        "`{ident}` is wider than one storage word: an account storage slot type encodes to at \
         most 4 felts"
    );
    let word_key = has("Copy").then(|| {
        quote! {
            impl ::miden::WordKey for #ident {
                fn try_into_word(self) -> ::core::result::Result<::miden::Word, &'static str> {
                    ::miden::padded_word_from_felt_repr(&self)
                }
            }
        }
    });
    quote! {
        // A variable-length encoding (`None`) is checked when the value is written; the
        // component's storage schema bounds it at build time.
        const _: () = assert!(
            match <#ident as ::miden::felt_repr::FromFeltRepr>::FIXED_LEN {
                ::core::option::Option::Some(width) => width <= 4,
                ::core::option::Option::None => true,
            },
            #width_message
        );

        impl ::miden::WordValue for #ident {
            fn try_into_word(self) -> ::core::result::Result<::miden::Word, &'static str> {
                ::miden::padded_word_from_felt_repr(&self)
            }

            fn try_from_word(
                word: ::miden::Word,
            ) -> ::core::result::Result<Self, &'static str> {
                ::miden::felt_repr_from_padded_word(word)
            }
        }

        #word_key
    }
}

/// Adds a derive so rustc filters conditional fields before type registration.
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
    let derive_crate = base_macros_derive_path();

    match item {
        Item::Struct(item_struct) => {
            let storage =
                storage_impls(&item_struct.ident, &item_struct.generics, &item_struct.attrs);
            quote! {
                #[derive(#derive_crate::__MidenExportType)]
                #item_struct
                #storage
            }
            .into()
        }
        Item::Enum(item_enum) => {
            let storage = storage_impls(&item_enum.ident, &item_enum.generics, &item_enum.attrs);
            quote! {
                #[derive(#derive_crate::__MidenExportType)]
                #item_enum
                #storage
            }
            .into()
        }
        other => {
            syn::Error::new_spanned(other, "#[export_type] may only be applied to structs or enums")
                .into_compile_error()
                .into()
        }
    }
}

/// Registers the cfg-filtered record or enum and emits its identity items.
pub(crate) fn derive(item: TokenStream) -> TokenStream {
    let item = parse_macro_input!(item as Item);

    match item {
        Item::Struct(item_struct) => {
            let span = item_struct.ident.span();
            match exported_type_from_struct(&item_struct)
                .and_then(|def| export_type_identity_items(&def, &item_struct.generics, span))
            {
                Ok(items) => items.into(),
                Err(err) => err.to_compile_error().into(),
            }
        }
        Item::Enum(item_enum) => {
            let span = item_enum.ident.span();
            match exported_type_from_enum(&item_enum)
                .and_then(|def| export_type_identity_items(&def, &item_enum.generics, span))
            {
                Ok(items) => items.into(),
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
