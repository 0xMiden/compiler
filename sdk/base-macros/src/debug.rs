//! Debug derive expansion for avoiding string formatting call graph cycles.
//!
//! Standard string debug formatting can introduce cycles unsupported by the Miden target.

use std::collections::HashSet;

use proc_macro_crate::{FoundCrate, crate_name};
use proc_macro2::{Span, TokenStream};
use quote::quote;
use syn::{Data, DeriveInput, Fields, Ident, ext::IdentExt, parse_quote};

/// Derives standard debug formatting with explicit raw string fields.
pub(crate) fn expand(mut input: DeriveInput) -> syn::Result<TokenStream> {
    for attr in &input.attrs {
        if attr.path().is_ident("miden_debug") {
            return Err(syn::Error::new_spanned(
                attr,
                "#[miden_debug(raw_str)] is only supported on fields",
            ));
        }
    }

    let fields = match &input.data {
        Data::Struct(data) => match &data.fields {
            Fields::Named(fields) => &fields.named,
            _ => {
                return Err(syn::Error::new_spanned(
                    &input,
                    "MidenDebug supports only structs with named fields",
                ));
            }
        },
        _ => {
            return Err(syn::Error::new_spanned(
                &input,
                "MidenDebug supports only structs with named fields",
            ));
        }
    };

    let sdk = match crate_name("miden") {
        Ok(FoundCrate::Name(name)) => {
            let ident = Ident::new(&name, Span::call_site());
            quote!(::#ident)
        }
        // Ordinary SDK users depend on `miden`
        _ => quote!(::miden),
    };
    let mut field_formatters = Vec::new();
    let mut bounded_types = HashSet::new();
    for field in fields {
        let mut raw = false;
        for attr in &field.attrs {
            if !attr.path().is_ident("miden_debug") {
                continue;
            }
            let option = attr.parse_args::<Ident>()?;
            if option != "raw_str" {
                return Err(syn::Error::new_spanned(option, "expected #[miden_debug(raw_str)]"));
            }
            if raw {
                return Err(syn::Error::new_spanned(attr, "duplicate #[miden_debug(raw_str)]"));
            }
            raw = true;
        }

        let ident = field.ident.as_ref().expect("named field");
        let name = ident.unraw().to_string();
        if raw {
            field_formatters.push(quote! {
                .field(#name, &#sdk::debug::RawString(&self.#ident))
            });
        } else {
            let ty = &field.ty;
            if bounded_types.insert(quote!(#ty).to_string()) {
                input.generics.make_where_clause().predicates.push(parse_quote! {
                    #ty: ::core::fmt::Debug
                });
            }
            field_formatters.push(quote! {
                .field(#name, &&self.#ident)
            });
        }
    }

    let ident = &input.ident;
    let name = ident.unraw().to_string();
    let (impl_generics, type_generics, where_clause) = input.generics.split_for_impl();
    Ok(quote! {
        impl #impl_generics ::core::fmt::Debug for #ident #type_generics #where_clause {
            fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                f.debug_struct(#name)
                    #(#field_formatters)*
                    .finish()
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use syn::{DeriveInput, parse_quote};

    use super::expand;

    fn error(input: DeriveInput) -> String {
        expand(input).unwrap_err().to_string()
    }

    #[test]
    fn adds_one_bound_per_field_type() {
        let expanded = expand(parse_quote! {
            struct Input<T, U> { first: T, second: T, other: U }
        })
        .unwrap();
        let implementation: syn::ItemImpl = syn::parse2(expanded).unwrap();
        let predicates = &implementation.generics.where_clause.unwrap().predicates;
        assert_eq!(predicates.len(), 2);
        assert_eq!(quote::quote!(#predicates).to_string(), quote::quote!(
            T: ::core::fmt::Debug, U: ::core::fmt::Debug
        ).to_string());
    }

    #[test]
    fn rejects_tuple_struct() {
        assert_eq!(
            error(parse_quote!(
                struct Input(u32);
            )),
            "MidenDebug supports only structs with named fields"
        );
    }

    #[test]
    fn rejects_unit_struct() {
        assert_eq!(
            error(parse_quote!(
                struct Input;
            )),
            "MidenDebug supports only structs with named fields"
        );
    }

    #[test]
    fn rejects_enum() {
        assert_eq!(
            error(parse_quote!(
                enum Input {
                    Value(u32),
                }
            )),
            "MidenDebug supports only structs with named fields"
        );
    }

    #[test]
    fn rejects_union() {
        assert_eq!(
            error(parse_quote!(union Input { value: u32 })),
            "MidenDebug supports only structs with named fields"
        );
    }

    #[test]
    fn rejects_struct_annotation() {
        assert_eq!(
            error(parse_quote! {
                #[miden_debug(raw_str)]
                struct Input { value: u32 }
            }),
            "#[miden_debug(raw_str)] is only supported on fields"
        );
    }

    #[test]
    fn rejects_unknown_option() {
        assert_eq!(
            error(parse_quote! {
                struct Input { #[miden_debug(skip)] value: u32 }
            }),
            "expected #[miden_debug(raw_str)]"
        );
    }

    #[test]
    fn rejects_duplicate_annotation() {
        assert_eq!(
            error(parse_quote! {
                struct Input {
                    #[miden_debug(raw_str)]
                    #[miden_debug(raw_str)]
                    value: &'static str,
                }
            }),
            "duplicate #[miden_debug(raw_str)]"
        );
    }

    #[test]
    fn rejects_empty_annotation() {
        assert!(
            expand(parse_quote! {
                struct Input { #[miden_debug()] value: &'static str }
            })
            .is_err()
        );
    }

    #[test]
    fn rejects_option_value() {
        assert!(
            expand(parse_quote! {
                struct Input { #[miden_debug(raw_str = true)] value: &'static str }
            })
            .is_err()
        );
    }
}
