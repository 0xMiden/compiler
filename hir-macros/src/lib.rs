extern crate proc_macro;

//mod op;
mod spanned;

use inflector::cases::kebabcase::to_kebab_case;
use quote::quote;
use syn::{parse_macro_input, spanned::Spanned, Data, DeriveInput, Error, Ident, Token};

#[proc_macro_derive(Spanned, attributes(span))]
pub fn derive_spanned(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    // Parse into syntax tree
    let derive = parse_macro_input!(input as DeriveInput);
    // Structure name
    let name = derive.ident;
    let result = match derive.data {
        Data::Struct(data) => spanned::derive_spanned_struct(name, data, derive.generics),
        Data::Enum(data) => spanned::derive_spanned_enum(name, data, derive.generics),
        Data::Union(_) => {
            Err(Error::new(name.span(), "deriving Spanned on unions is not currently supported"))
        }
    };
    match result {
        Ok(ts) => ts,
        Err(err) => err.into_compile_error().into(),
    }
}

/// #[derive(Op)]
/// #[op(name = "select", interfaces(BranchOpInterface))]
/// pub struct Select {
///     #[operation]
///     op: Operation,
///     #[operand]
///     selector: OpOperand,
///
/// }
/*
#[proc_macro_derive(Op, attributes(op, operation, operand, result, successor, region, interfaces))]
pub fn derive_op(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    // Parse into syntax tree
    let derive = parse_macro_input!(input as DeriveInput);
    let op = match op::Op::from_derive_input(derive) {
        Ok(op) => op,
        Err(err) => err.to_compile_error().into(),
    };
    quote!(#op).into()
}
 */

#[proc_macro_derive(PassInfo)]
pub fn derive_pass_info(item: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let derive_input = parse_macro_input!(item as DeriveInput);
    let derive_span = derive_input.span();
    let id = derive_input.ident.clone();
    let generics = derive_input.generics;
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();
    let name = derive_input.ident.to_string();
    let pass_name = to_kebab_case(&name);
    let pass_name_lit = syn::Lit::Str(syn::LitStr::new(&pass_name, id.span()));

    let doc_ident = syn::Ident::new("doc", derive_span);
    let docs = derive_input
        .attrs
        .iter()
        .filter_map(|attr| match attr.meta {
            syn::Meta::NameValue(ref nv) => {
                if nv.path.get_ident()? == &doc_ident {
                    match nv.value {
                        syn::Expr::Lit(syn::ExprLit {
                            lit: syn::Lit::Str(ref lit),
                            ..
                        }) => Some(lit.value()),
                        _ => None,
                    }
                } else {
                    None
                }
            }
            syn::Meta::Path(_) | syn::Meta::List(_) => None,
        })
        .collect::<Vec<_>>();
    let pass_summary = match docs.first() {
        Some(line) => syn::Lit::Str(syn::LitStr::new(line, derive_span)),
        None => syn::Lit::Str(syn::LitStr::new("", derive_span)),
    };
    let description = docs.into_iter().collect::<String>();
    let pass_description = syn::Lit::Str(syn::LitStr::new(&description, derive_span));

    let quoted = quote! {
        impl #impl_generics PassInfo for #id #ty_generics #where_clause {
            const FLAG: &'static str = #pass_name_lit;
            const SUMMARY: &'static str = #pass_summary;
            const DESCRIPTION: &'static str = #pass_description;
        }
    };

    proc_macro::TokenStream::from(quoted)
}

#[proc_macro_derive(AnalysisKey, attributes(analysis_key))]
pub fn derive_analysis_key(item: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let derive_input = parse_macro_input!(item as DeriveInput);
    let derive_span = derive_input.span();
    let id = derive_input.ident.clone();
    let generics = derive_input.generics;
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

    let found = match &derive_input.data {
        syn::Data::Struct(ref data) => match &data.fields {
            syn::Fields::Named(ref fields) => {
                let mut found = None;
                for field in fields.named.iter() {
                    if field.attrs.iter().any(is_analysis_key_attr) {
                        if found.is_some() {
                            return syn::Error::new(
                                field.span(),
                                "duplicate #[analysis_key] field",
                            )
                            .into_compile_error()
                            .into();
                        }
                        found = Some((field.ident.as_ref().cloned().unwrap(), field.ty.clone()));
                    }
                }
                found
            }
            syn::Fields::Unnamed(ref fields) => {
                let mut found = None;
                for (i, field) in fields.unnamed.iter().enumerate() {
                    if field.attrs.iter().any(is_analysis_key_attr) {
                        if found.is_some() {
                            return syn::Error::new(
                                field.span(),
                                "duplicate #[analysis_key] field",
                            )
                            .into_compile_error()
                            .into();
                        }
                        found = Some((Ident::new(&i.to_string(), field.span()), field.ty.clone()));
                    }
                }
                found
            }
            syn::Fields::Unit => {
                return syn::Error::new(
                    derive_span,
                    "structs with unit fields cannot derive AnalysisKey",
                )
                .into_compile_error()
                .into()
            }
        },
        syn::Data::Enum(_) => {
            return syn::Error::new(derive_span, "enums cannot derive AnalysisKey")
                .into_compile_error()
                .into()
        }
        syn::Data::Union(_) => {
            return syn::Error::new(derive_span, "unions cannot derive AnalysisKey")
                .into_compile_error()
                .into()
        }
    };

    let (field_id, field_ty) = match found {
        Some(found) => found,
        None => {
            return syn::Error::new(derive_span, "missing #[analysis_key] attribute")
                .into_compile_error()
                .into()
        }
    };

    let quoted = quote! {
        impl #impl_generics AnalysisKey for #id #ty_generics #where_clause {
            type Key = #field_ty;

            fn key(&self) -> Self::Key { self.#field_id }
        }
    };

    proc_macro::TokenStream::from(quoted)
}

#[proc_macro_derive(RewritePassRegistration)]
pub fn derive_rewrite_pass_registration(item: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let derive_input = parse_macro_input!(item as DeriveInput);
    let id = derive_input.ident.clone();
    let generics = derive_input.generics;
    let mut params = syn::punctuated::Punctuated::<_, Token![,]>::new();
    for gp in generics.params.iter() {
        match gp {
            syn::GenericParam::Lifetime(ref lt) => {
                if !lt.bounds.empty_or_trailing() {
                    return syn::Error::new(
                        gp.span(),
                        "cannot derive RewritePassRegistration on a type with lifetime bounds",
                    )
                    .into_compile_error()
                    .into();
                }
                params.push(syn::GenericArgument::Lifetime(syn::Lifetime {
                    apostrophe: lt.span(),
                    ident: Ident::new("_", lt.span()),
                }));
            }
            syn::GenericParam::Type(ref ty) => {
                if !ty.bounds.empty_or_trailing() {
                    return syn::Error::new(
                        gp.span(),
                        "cannot derive RewritePassRegistration on a generic type with type bounds",
                    )
                    .into_compile_error()
                    .into();
                }
                let param_ty: syn::Type = syn::parse_quote_spanned! { ty.span() => () };
                params.push(syn::GenericArgument::Type(param_ty));
            }
            syn::GenericParam::Const(_) => {
                return syn::Error::new(
                    gp.span(),
                    "cannot derive RewritePassRegistration on a generic type with const arguments",
                )
                .into_compile_error()
                .into();
            }
        }
    }

    let quoted = if params.empty_or_trailing() {
        quote! {
            inventory::submit!(midenc_hir::pass::RewritePassRegistration::new::<#id>());
            inventory::submit! {
                midenc_session::CompileFlag::new(<#id as PassInfo>::FLAG)
                    .long(<#id as PassInfo>::FLAG)
                    .help(<#id as PassInfo>::SUMMARY)
                    .help_heading("Rewrites")
                    .action(midenc_session::FlagAction::SetTrue)
                    .hide(true)
            }
        }
    } else {
        quote! {
            inventory::submit!(midenc_hir::pass::RewritePassRegistration::new::<#id<#params>>());
            inventory::submit! {
                midenc_session::CompileFlag::new(<#id<#params> as PassInfo>::FLAG)
                    .long(<#id<#params> as PassInfo>::FLAG)
                    .help(<#id<#params> as PassInfo>::SUMMARY)
                    .help_heading("Rewrites")
                    .action(midenc_session::FlagAction::SetTrue)
                    .hide(true)
            }
        }
    };

    proc_macro::TokenStream::from(quoted)
}

#[proc_macro_derive(ModuleRewritePassAdapter)]
pub fn derive_module_rewrite_pass_adapter(
    item: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    let derive_input = parse_macro_input!(item as DeriveInput);
    let id = derive_input.ident.clone();

    let quoted = quote! {
        inventory::submit!(midenc_hir::pass::RewritePassRegistration::new::<midenc_hir::pass::ModuleRewritePassAdapter::<#id>>());
    };

    proc_macro::TokenStream::from(quoted)
}

#[proc_macro_derive(ConversionPassRegistration)]
pub fn derive_conversion_pass_registration(
    item: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    let derive_input = parse_macro_input!(item as DeriveInput);
    let id = derive_input.ident.clone();
    let generics = derive_input.generics;
    let mut params = syn::punctuated::Punctuated::<_, Token![,]>::new();
    for gp in generics.params.iter() {
        match gp {
            syn::GenericParam::Lifetime(ref lt) => {
                if !lt.bounds.empty_or_trailing() {
                    return syn::Error::new(
                        gp.span(),
                        "cannot derive ConversionPassRegistration on a type with lifetime bounds",
                    )
                    .into_compile_error()
                    .into();
                }
                params.push(syn::GenericArgument::Lifetime(syn::Lifetime {
                    apostrophe: lt.span(),
                    ident: Ident::new("_", lt.span()),
                }));
            }
            syn::GenericParam::Type(ref ty) => {
                if !ty.bounds.empty_or_trailing() {
                    return syn::Error::new(
                        gp.span(),
                        "cannot derive ConversionPassRegistration on a generic type with type \
                         bounds",
                    )
                    .into_compile_error()
                    .into();
                }
                let param_ty: syn::Type = syn::parse_quote_spanned! { ty.span() => () };
                params.push(syn::GenericArgument::Type(param_ty));
            }
            syn::GenericParam::Const(_) => {
                return syn::Error::new(
                    gp.span(),
                    "cannot derive ConversionPassRegistration on a generic type with const \
                     arguments",
                )
                .into_compile_error()
                .into();
            }
        }
    }

    let quoted = if params.empty_or_trailing() {
        quote! {
            inventory::submit! {
                midenc_session::CompileFlag::new(<#id as PassInfo>::FLAG)
                    .long(<#id as PassInfo>::FLAG)
                    .help(<#id as PassInfo>::SUMMARY)
                    .help_heading("Conversions")
                    .action(midenc_session::FlagAction::SetTrue)
                    .hide(true)
            }
        }
    } else {
        quote! {
            inventory::submit! {
                midenc_session::CompileFlag::new(<#id<#params> as PassInfo>::FLAG)
                    .long(<#id<#params> as PassInfo>::FLAG)
                    .help(<#id<#params> as PassInfo>::SUMMARY)
                    .help_heading("Conversions")
                    .action(midenc_session::FlagAction::SetTrue)
                    .hide(true)
            }
        }
    };

    proc_macro::TokenStream::from(quoted)
}

fn is_analysis_key_attr(attr: &syn::Attribute) -> bool {
    if let syn::Meta::Path(ref path) = attr.meta {
        path.is_ident("analysis_key")
    } else {
        false
    }
}
