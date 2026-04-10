use std::collections::BTreeMap;

use darling::{FromDeriveInput, FromField};
use quote::quote;
use syn::{Ident, Token, parse_quote, punctuated::Punctuated};

pub fn derive_effect_op_interface(input: &syn::DeriveInput) -> darling::Result<EffectOpInterface> {
    EffectOpInterface::from_derive_input(input)
}

#[derive(Debug, FromDeriveInput)]
#[darling(
    forward_attrs(doc, cfg, allow, derive, effects),
    supports(struct_named)
)]
pub struct EffectOpInterface {
    ident: Ident,
    generics: syn::Generics,
    attrs: Vec<syn::Attribute>,
    data: darling::ast::Data<(), FieldEffect>,
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

impl quote::ToTokens for EffectOpInterface {
    fn to_tokens(&self, tokens: &mut proc_macro2::TokenStream) {
        let struct_data = self.data.as_ref().take_struct().unwrap();
        let op_type = &self.ident;
        let (impl_generics, ty_generics, where_clause) = self.generics.split_for_impl();

        let global_effects = match parse_effects(&self.attrs) {
            Ok(effects) => effects,
            Err(err) => {
                tokens.extend(err.into_compile_error());
                return;
            }
        };

        let emit_no_memory_effects_impl = global_effects.is_empty()
            && struct_data
                .fields
                .iter()
                .all(|f| !f.attrs.iter().any(|attr| attr.path().is_ident("effects")));

        if emit_no_memory_effects_impl {
            tokens.extend(quote! {
                impl #impl_generics ::midenc_hir::effects::EffectOpInterface<::midenc_hir::effects::MemoryEffect> for #op_type #ty_generics #where_clause {
                    #[inline(always)]
                    fn has_no_effect(&self) -> bool {
                        true
                    }

                    fn effects(
                        &self,
                    ) -> ::midenc_hir::effects::EffectIterator<::midenc_hir::effects::MemoryEffect> {
                        ::midenc_hir::effects::EffectIterator::from_smallvec(::midenc_hir::SmallVec::new_const())
                    }
                }
            });
            return;
        }

        for Effect {
            kind: effect_kind,
            values,
        } in global_effects
        {
            /*
            let conflicting_effects = struct_data.fields.iter().find_map(|f| {
                f.effects.iter().find_map(|p| {
                    if &p.kind == effect_kind {
                        Some(p.kind.span())
                    } else {
                        None
                    }
                })
            });
            if let Some(span) = conflicting_effects {
                tokens.extend(
                    darling::Error::custom("conflicts with global effect")
                        .with_span(&span)
                        .write_errors(),
                );
                return;
            }
             */
            let effect_values =
                Punctuated::<syn::Expr, Token![,]>::from_iter(values.iter().map(|expr| {
                    let expr: syn::Expr = parse_quote! {
                        ::midenc_hir::effects::EffectInstance::new(#expr)
                    };
                    expr
                }));
            tokens.extend(quote! {
                impl #impl_generics ::midenc_hir::effects::EffectOpInterface<#effect_kind> for #op_type #ty_generics #where_clause {
                    #[inline(always)]
                    fn has_no_effect(&self) -> bool {
                        false
                    }

                    fn effects(
                        &self,
                    ) -> ::midenc_hir::effects::EffectIterator<#effect_kind> {
                        ::midenc_hir::effects::EffectIterator::from_smallvec(::midenc_hir::smallvec![
                            #effect_values
                        ])
                    }
                }
            });
        }

        let mut by_kind = BTreeMap::<
            String,
            (syn::Path, BTreeMap<Ident, Punctuated<syn::Expr, Token![,]>>),
        >::default();
        for field in struct_data.fields.iter() {
            let effects = match parse_effects(&field.attrs) {
                Ok(effects) if effects.is_empty() => continue,
                Ok(effects) => effects,
                Err(err) => {
                    tokens.extend(err.into_compile_error());
                    return;
                }
            };
            for Effect {
                kind: effect_kind,
                values,
            } in effects
            {
                let values_by_kind = by_kind
                    .entry(effect_kind.to_token_stream().to_string())
                    .or_insert_with(move || (effect_kind, BTreeMap::default()));
                values_by_kind.1.entry(field.ident.clone().unwrap()).or_default().extend(values);
            }
        }

        if by_kind.is_empty() {
            return;
        }

        for (_kind, (kind_path, values_by_field)) in by_kind.iter() {
            let effect_values = Punctuated::<_, Token![;]>::from_iter(values_by_field.iter().map(
                |(field, exprs)| {
                    let exprs = exprs.iter();
                    quote! {
                        {
                            values.extend([
                                #(
                                    ::midenc_hir::effects::EffectInstance::new_for_value(
                                        #exprs,
                                        self.#field(),
                                    ),
                                )*
                            ]);
                        }
                    }
                },
            ));
            tokens.extend(quote! {
                impl #impl_generics ::midenc_hir::effects::EffectOpInterface<#kind_path> for #op_type #ty_generics #where_clause {
                    #[inline(always)]
                    fn has_no_effect(&self) -> bool {
                        false
                    }

                    fn effects(
                        &self,
                    ) -> ::midenc_hir::effects::EffectIterator<#kind_path> {
                        let mut values = ::midenc_hir::SmallVec::<[::midenc_hir::effects::EffectInstance<#kind_path>; _]>::new_const();
                        #effect_values
                        ::midenc_hir::effects::EffectIterator::from_smallvec(values)
                    }
                }
            });
        }
    }
}

fn parse_effects(attrs: &[syn::Attribute]) -> Result<Vec<Effect>, syn::Error> {
    let effects = attrs.iter().find_map(|attr| {
        if attr.path().is_ident("effects") {
            Some(attr.meta.require_list().and_then(|list| {
                list.parse_args_with(Punctuated::<Effect, Token![,]>::parse_separated_nonempty)
            }))
        } else {
            None
        }
    });
    match effects {
        None => Ok(vec![]),
        Some(Ok(effects)) => Ok(effects.into_iter().collect()),
        Some(Err(err)) => Err(err),
    }
}
