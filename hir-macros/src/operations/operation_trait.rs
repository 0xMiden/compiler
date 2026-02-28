use darling::FromMeta;
use quote::quote;
use syn::spanned::Spanned;

/// Derives boilerplate for operation traits.
///
/// ```rust,ignore
/// #[operation_trait]
/// pub trait SameOperandsAndResultType: SameTypeOperands {
///     #[verifier]
///     #[inline]
///     fn operands_and_result_are_the_same_type(op: &Operation, context: &Context) -> Result<(), Report> {
///        ...
///     }
/// }
/// ```
///
/// Generates:
///
/// ```rust,ignore
/// pub trait SameOperandsAndResultType: SameTypeOperands {}
///
/// impl<T: Op + SameOperandsAndResultType> Verify<dyn SameOperandsAndResultType> for T {
///     #[inline]
///     fn verify(&self, context: &Context) -> Result<(), Report> {
///         let op = self.as_operation();
///         <Operation as Verify<dyn SameTypeOperands>>::verify(op, context)?;
///         <Operation as Verify<dyn SameOperandsAndResultType>>::verify(op, context)?;
///     }
/// }
///
/// impl Verify<dyn SameOperandsAndResultType> for Operation {
///     fn should_verify(&self, _context: &Context) -> bool {
///         self.implements::<dyn SameTypeOperands>() &&
///         self.implements::<dyn SameOperandsAndResultType>()
///     }
///
///     fn verify(&self, context: &Context) -> Result<(), Report> {
///         #[inline]
///         fn operands_and_result_are_the_same_type(op: &Operation, context: &Context) -> Result<(), Report> {
///            ...
///         }
///
///         operands_and_result_are_the_same_type(self, context)?;
///
///         Ok(())
///     }
/// }
/// ```
///
/// Notes:
///
/// * Super traits of the given trait are implied to be other operation traits, except for certain
///   auto-traits and builtin traits e.g. `Eq`. If there are exceptions, you can provide
///   `#[ignore(Trait)]` to exclude `Trait` from being treated as an operation trait for
///   verification purposes.
/// * Any number of `#[verifier]` functions can be provided
pub fn derive_operation_trait(
    meta: Vec<darling::ast::NestedMeta>,
    mut input: syn::ItemTrait,
) -> darling::Result<proc_macro2::TokenStream> {
    let span = input.span();
    let trait_name = input.ident.clone();

    let operation_trait_attr =
        input.attrs.iter().position(|attr| attr.path().is_ident("operation_trait"));
    if let Some(index) = operation_trait_attr {
        input.attrs.remove(index);
    }

    // Remove verifiers from the set of trait items
    let verifiers = input
        .items
        .extract_if(.., |item| {
            if let syn::TraitItem::Fn(fn_item) = item {
                fn_item
                    .attrs
                    .iter()
                    .any(|attr| attr.path().get_ident().is_some_and(|id| id == "verifier"))
            } else {
                false
            }
        })
        .map(|item| {
            let syn::TraitItem::Fn(mut fn_item) = item else {
                unreachable!()
            };
            let verifier_attr_pos = fn_item
                .attrs
                .iter()
                .position(|attr| attr.path().get_ident().is_some_and(|id| id == "verifier"))
                .unwrap();
            fn_item.attrs.remove(verifier_attr_pos);
            fn_item
        })
        .collect::<Vec<_>>();

    let verifier_fns = verifiers.iter().map(|item| item.sig.ident.clone()).collect::<Vec<_>>();

    let ignore_supertraits = match meta.iter().find_map(|meta| match meta {
        meta @ darling::ast::NestedMeta::Meta(syn::Meta::List(list))
            if list.path.is_ident("ignore") =>
        {
            Some(darling::util::PathList::from_nested_meta(meta))
        }
        _ => None,
    }) {
        Some(result) => result?,
        None => darling::util::PathList::default(),
    };
    let verifiable_supertraits = input
        .supertraits
        .iter()
        .filter_map(|st| match st {
            syn::TypeParamBound::Trait(t) => {
                if !ignore_supertraits.iter().any(|path| path == &t.path) {
                    Some(st.clone())
                } else {
                    None
                }
            }
            _ => None,
        })
        .collect::<Vec<_>>();

    let mut tokens = proc_macro2::TokenStream::new();

    // Define generic type: TOp: ::midenc_hir::Op + #trait_name
    let mut verify_generics = input.generics.clone();
    verify_generics.params.push(syn::GenericParam::Type(syn::TypeParam {
        attrs: Default::default(),
        ident: syn::Ident::new("TOp", span),
        colon_token: Some(syn::token::Colon(span)),
        bounds: syn::punctuated::Punctuated::from_iter([
            syn::TypeParamBound::Trait(syn::TraitBound {
                paren_token: None,
                modifier: syn::TraitBoundModifier::None,
                lifetimes: None,
                path: syn::Path {
                    leading_colon: Some(syn::token::PathSep(span)),
                    segments: syn::punctuated::Punctuated::from_iter([
                        syn::PathSegment {
                            ident: syn::Ident::new("midenc_hir", span),
                            arguments: syn::PathArguments::None,
                        },
                        syn::PathSegment {
                            ident: syn::Ident::new("Op", span),
                            arguments: syn::PathArguments::None,
                        },
                    ]),
                },
            }),
            syn::TypeParamBound::Trait(syn::TraitBound {
                paren_token: None,
                modifier: syn::TraitBoundModifier::None,
                lifetimes: None,
                path: syn::Path {
                    leading_colon: None,
                    segments: syn::punctuated::Punctuated::from_iter([syn::PathSegment {
                        ident: trait_name.clone(),
                        arguments: syn::PathArguments::AngleBracketed(
                            syn::AngleBracketedGenericArguments {
                                colon2_token: None,
                                lt_token: syn::token::Lt(span),
                                args: syn::punctuated::Punctuated::from_iter(
                                    input.generics.type_params().map(|tp| {
                                        syn::GenericArgument::Type(syn::Type::Path(syn::TypePath {
                                            qself: None,
                                            path: syn::Path {
                                                leading_colon: None,
                                                segments: syn::punctuated::Punctuated::from_iter([
                                                    syn::PathSegment {
                                                        ident: tp.ident.clone(),
                                                        arguments: syn::PathArguments::None,
                                                    },
                                                ]),
                                            },
                                        }))
                                    }),
                                ),
                                gt_token: syn::token::Gt(span),
                            },
                        ),
                    }]),
                },
            }),
        ]),
        eq_token: None,
        default: None,
    }));
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();
    let (impl_verify_generics, _verify_ty_generics, verify_where_clause) =
        verify_generics.split_for_impl();

    if verifier_fns.is_empty() {
        // If there are no verifiers, emit no-op Verify impls anyway so that other infra can rely
        // on operation traits providing this.
        tokens.extend(quote! {
            #input

            impl #impl_verify_generics ::midenc_hir::Verify<dyn #trait_name #ty_generics> for TOp #verify_where_clause {
                #[inline(always)]
                fn verify(&self, _context: &::midenc_hir::Context) -> Result<(), ::midenc_hir::diagnostics::Report> { Ok(()) }
            }

            impl #impl_generics ::midenc_hir::Verify<dyn #trait_name #ty_generics> for ::midenc_hir::Operation #where_clause {
                #[inline(always)]
                fn should_verify(&self, _context: &::midenc_hir::Context) -> bool { false }
                #[inline(always)]
                fn verify(&self, _context: &::midenc_hir::Context) -> Result<(), ::midenc_hir::diagnostics::Report> { Ok(()) }
            }
        });
    } else {
        let ty_generics_turbofish = ty_generics.as_turbofish();
        tokens.extend(quote! {
            #input

            impl #impl_verify_generics ::midenc_hir::Verify<dyn #trait_name #ty_generics> for TOp #verify_where_clause {
                #[allow(unused_variables)]
                fn verify(&self, context: &::midenc_hir::Context) -> Result<(), ::midenc_hir::diagnostics::Report> {
                    let op = <Self as ::midenc_hir::Op>::as_operation(self);
                    #(
                        <::midenc_hir::Operation as ::midenc_hir::Verify<dyn #verifiable_supertraits>>::verify(op, context)?;
                    )*
                    <::midenc_hir::Operation as ::midenc_hir::Verify<dyn #trait_name #ty_generics>>::verify(op, context)
                }
            }

            impl #impl_generics ::midenc_hir::Verify<dyn #trait_name #ty_generics> for ::midenc_hir::Operation #where_clause {
                #[allow(unused_variables)]
                fn should_verify(&self, _context: &::midenc_hir::Context) -> bool {
                    #(
                        self.implements::<dyn #verifiable_supertraits>() &&
                    )*
                    self.implements::<dyn #trait_name #ty_generics>()
                }

                #[allow(unused_variables)]
                fn verify(&self, context: &::midenc_hir::Context) -> Result<(), ::midenc_hir::diagnostics::Report> {
                    #(
                        #verifiers
                    )*

                    #(
                        #verifier_fns #ty_generics_turbofish (self, context)?;
                    )*

                    Ok(())
                }
            }
        });
    }

    Ok(tokens)
}
