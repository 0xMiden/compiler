mod attribute;

use darling::FromDeriveInput;

use self::attribute::Attribute;

pub fn derive_attribute(input: &syn::DeriveInput) -> darling::Result<Attribute> {
    Attribute::from_derive_input(input)
}

pub fn derive_dialect_registration(
    input: &syn::DeriveInput,
) -> darling::Result<DialectRegistration> {
    DialectRegistration::from_derive_input(input)
}

pub fn derive_dialect(input: &syn::DeriveInput) -> darling::Result<Dialect> {
    Dialect::from_derive_input(input)
}

use inflector::Inflector;
use quote::{format_ident, quote_spanned};
use syn::Ident;

/// Represents the parsed struct definition for the dialect we wish to register
///
/// Only named structs are allowed at this time.
#[derive(Debug, FromDeriveInput)]
#[darling(
    attributes(dialect),
    forward_attrs(doc, cfg, allow, derive),
    supports(struct_named)
)]
pub struct Dialect {
    ident: Ident,
    generics: syn::Generics,
    data: darling::ast::Data<(), DialectField>,
    #[darling(default)]
    name: Option<Ident>,
}

/// Represents the parsed struct definition for the dialect we wish to register
///
/// Only named structs are allowed at this time.
#[derive(Debug, FromDeriveInput)]
#[darling(
    attributes(dialect),
    forward_attrs(doc, cfg, allow, derive),
    supports(struct_any)
)]
pub struct DialectRegistration {
    ident: Ident,
    generics: syn::Generics,
    #[darling(default)]
    name: Option<Ident>,
}

#[derive(Debug, darling::FromField)]
#[darling(attributes(dialect), forward_attrs(doc, cfg, allow))]
struct DialectField {
    ident: Option<Ident>,
    #[darling(default)]
    info: darling::util::SpannedValue<bool>,
}

impl quote::ToTokens for Dialect {
    fn to_tokens(&self, tokens: &mut proc_macro2::TokenStream) {
        let dialect_name = self.name.clone().unwrap_or_else(|| {
            let name = self.ident.to_string();
            let name = name.strip_suffix("Dialect").unwrap_or(&name);
            let name = name.to_snake_case();
            format_ident!("{name}", span = self.ident.span())
        });
        let dialect_struct_name = &self.ident;
        let dialect_struct_fields = self.data.as_ref().take_struct().unwrap();
        let Some(dialect_info_field) =
            dialect_struct_fields.fields.iter().enumerate().find_map(|(i, f)| {
                let span = f.info.span();
                if *f.info {
                    Some(if let Some(id) = f.ident.clone() {
                        syn::Member::Named(id)
                    } else {
                        syn::Member::Unnamed(syn::Index {
                            index: i as u32,
                            span,
                        })
                    })
                } else {
                    None
                }
            })
        else {
            tokens.extend(
                syn::Error::new(
                    dialect_name.span(),
                    "expected struct to contain a DialectInfo field marked with #[info]",
                )
                .into_compile_error(),
            );
            return;
        };

        let has_non_info_fields = dialect_struct_fields.len() > 1;

        let (impl_generics, ty_generics, where_clause) = self.generics.split_for_impl();

        // impl Dialect
        tokens.extend(quote_spanned! { dialect_name.span() =>
            impl #impl_generics ::midenc_hir::Dialect for #dialect_struct_name #ty_generics #where_clause {
                #[inline(always)]
                fn info(&self) -> &DialectInfo {
                    &self.#dialect_info_field
                }
            }
        });

        // impl From<DialectInfo>
        if !has_non_info_fields {
            tokens.extend(quote_spanned! { dialect_name.span() =>
                impl #impl_generics From<::midenc_hir::DialectInfo> for #dialect_struct_name #ty_generics #where_clause {
                    #[allow(clippy::redundant_field_names)]
                    fn from(info: ::midenc_hir::DialectInfo) -> Self {
                        Self {
                            #dialect_info_field: info,
                        }
                    }
                }
            });
        }
    }
}

impl quote::ToTokens for DialectRegistration {
    fn to_tokens(&self, tokens: &mut proc_macro2::TokenStream) {
        let dialect_name = self.name.clone().unwrap_or_else(|| {
            let name = self.ident.to_string();
            let name = name.strip_suffix("Dialect").unwrap_or(&name);
            let name = name.to_snake_case();
            format_ident!("{name}", span = self.ident.span())
        });
        let dialect_struct_name = &self.ident;

        let dialect_namespace =
            syn::Lit::Str(syn::LitStr::new(&dialect_name.to_string(), dialect_name.span()));

        let (impl_generics, ty_generics, where_clause) = self.generics.split_for_impl();

        // impl DialectRegistration
        tokens.extend(quote_spanned! { dialect_name.span() =>
            impl #impl_generics ::midenc_hir::DialectRegistration for #dialect_struct_name #ty_generics #where_clause {
                const NAMESPACE: &'static str = #dialect_namespace;

                #[inline]
                fn init(info: ::midenc_hir::DialectInfo) -> Self {
                    Self::from(info)
                }

                fn register_operations(_info: &mut ::midenc_hir::DialectInfo) {
                }

                fn register_attributes(_info: &mut ::midenc_hir::DialectInfo) {
                }
            }

            ::midenc_hir::inventory::submit!(::midenc_hir::DialectRegistrationInfo::new::<#dialect_struct_name #ty_generics>());
        });
    }
}
