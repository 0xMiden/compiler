use std::collections::BTreeMap;

use darling::{FromDeriveInput, FromField};
use quote::{ToTokens, quote};
use syn::{Ident, Token, parse_quote, punctuated::Punctuated};

pub fn derive_effect_op_interface(input: &syn::DeriveInput) -> darling::Result<EffectOpInterface> {
    let input = EffectOpInterfaceInput::from_derive_input(input)?;
    let fields = input.data.take_struct().unwrap().fields;
    let mut effects = BTreeMap::<String, EffectGroup>::new();
    let declarations = std::iter::once((None, input.attrs.as_slice()))
        .chain(fields.iter().map(|field| (field.ident.clone(), field.attrs.as_slice())));
    for (field, attrs) in declarations {
        for Effect { kind, values } in parse_effects(attrs)? {
            let group =
                effects
                    .entry(kind.to_token_stream().to_string())
                    .or_insert_with(|| EffectGroup {
                        kind,
                        instances: Vec::new(),
                    });
            group.instances.extend(values.into_iter().map(|value| EffectInstance {
                field: field.clone(),
                value,
            }));
        }
    }
    if effects.is_empty() {
        let kind: syn::Path = parse_quote!(::midenc_hir::effects::MemoryEffect);
        effects.insert(
            kind.to_token_stream().to_string(),
            EffectGroup {
                kind,
                instances: Vec::new(),
            },
        );
    }

    Ok(EffectOpInterface {
        ident: input.ident,
        generics: input.generics,
        effects,
    })
}

#[derive(Debug, FromDeriveInput)]
#[darling(
    forward_attrs(doc, cfg, allow, derive, effects),
    supports(struct_named)
)]
struct EffectOpInterfaceInput {
    ident: Ident,
    generics: syn::Generics,
    attrs: Vec<syn::Attribute>,
    data: darling::ast::Data<(), FieldEffect>,
}

pub struct EffectOpInterface {
    ident: Ident,
    generics: syn::Generics,
    effects: BTreeMap<String, EffectGroup>,
}

struct EffectGroup {
    kind: syn::Path,
    instances: Vec<EffectInstance>,
}

struct EffectInstance {
    field: Option<Ident>,
    value: syn::Expr,
}

struct Effect {
    kind: syn::Path,
    values: Punctuated<syn::Expr, Token![,]>,
}

impl syn::parse::Parse for Effect {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let kind = input.parse::<syn::Path>()?;
        let values;
        let _paren = syn::parenthesized!(values in input);
        Ok(Self {
            kind,
            values: values.parse_terminated(syn::Expr::parse, Token![,])?,
        })
    }
}

#[derive(Debug, FromField)]
#[darling(forward_attrs(doc, cfg, allow, effects))]
struct FieldEffect {
    ident: Option<Ident>,
    attrs: Vec<syn::Attribute>,
}

impl ToTokens for EffectOpInterface {
    fn to_tokens(&self, tokens: &mut proc_macro2::TokenStream) {
        let op_type = &self.ident;
        let (impl_generics, ty_generics, where_clause) = self.generics.split_for_impl();
        for EffectGroup { kind, instances } in self.effects.values() {
            let no_effects = instances.is_empty();
            let values = instances.iter().map(|EffectInstance { field, value }| match field {
                Some(field) => quote! {
                    ::midenc_hir::effects::EffectInstance::new_for_value(#value, self.#field())
                },
                None => quote! {
                    ::midenc_hir::effects::EffectInstance::new(#value)
                },
            });
            tokens.extend(quote! {
                impl #impl_generics ::midenc_hir::effects::EffectOpInterface<#kind> for #op_type #ty_generics #where_clause {
                    #[inline(always)]
                    fn has_no_effect(&self) -> bool {
                        #no_effects
                    }

                    fn effects(&self) -> ::midenc_hir::effects::EffectIterator<#kind> {
                        ::midenc_hir::effects::EffectIterator::from_smallvec(
                            ::midenc_hir::smallvec![#(#values),*]
                        )
                    }
                }
            });
        }
    }
}

fn parse_effects(attrs: &[syn::Attribute]) -> syn::Result<Vec<Effect>> {
    let mut effects = Vec::new();
    for attr in attrs.iter().filter(|attr| attr.path().is_ident("effects")) {
        effects.extend(
            attr.meta
                .require_list()?
                .parse_args_with(Punctuated::<Effect, Token![,]>::parse_terminated)?,
        );
    }
    Ok(effects)
}
