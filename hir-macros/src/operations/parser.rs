use darling::FromDeriveInput;
use proc_macro2::Span;
use quote::quote;
use syn::{Ident, parse_quote};

use crate::operation::{OperationFormat, ParamShape, RegionInfo, RegionOptions, ResultShape};

pub fn derive_op_parser(input: &syn::DeriveInput) -> darling::Result<DeriveOpParser> {
    DeriveOpParser::from_derive_input(input)
}

/// Represents the parsed struct definition for the operation we wish to define
///
/// Only named structs are allowed at this time.
#[derive(Debug, FromDeriveInput)]
#[darling(
    attributes(operation, parser),
    supports(struct_named),
    forward_attrs(doc, cfg, allow, derive)
)]
pub struct DeriveOpParser {
    ident: Ident,
    generics: syn::Generics,
    data: darling::ast::Data<(), crate::operation::OperationField>,
    #[allow(unused)]
    dialect: Ident,
    #[darling(default)]
    #[allow(unused)]
    name: Option<Ident>,
    #[darling(default)]
    traits: darling::util::PathList,
    #[darling(default)]
    implements: darling::util::PathList,
}

impl quote::ToTokens for DeriveOpParser {
    fn to_tokens(&self, tokens: &mut proc_macro2::TokenStream) {
        let struct_data = self.data.as_ref().take_struct().unwrap();

        let format = match OperationFormat::from_struct(
            &self.ident,
            &struct_data.fields,
            &self.traits,
            &self.implements,
        ) {
            Ok(format) => format,
            Err(err) => {
                tokens.extend(err.write_errors());
                return;
            }
        };

        let total_operand_groups_lit = syn::Lit::Int(syn::LitInt::new(
            &format.operand_groups.len().to_string(),
            self.ident.span(),
        ));

        let mut gather_operands = quote! {
            #[allow(unused_variables, unused_mut)]
            let mut gathered_operands = ::midenc_hir::SmallVec::<[::midenc_hir::SmallVec<[::midenc_hir::parse::UnresolvedOperand; 2]>; 2]>::new_const();
            #[allow(unused_variables, unused_mut, unused_assignments)]
            let mut next_operand_group = 0usize;
        };
        if !format.operand_groups.is_empty() {
            let mut operand_group_parsers = quote! {};
            for group in format.operand_groups.iter() {
                if group.successor_operands {
                    continue;
                }

                let group_index_lit =
                    syn::Lit::Int(syn::LitInt::new(&group.index.to_string(), self.ident.span()));
                if group.requires_delimiter {
                    if let Some(size) = group.size {
                        let size_lit =
                            syn::Lit::Int(syn::LitInt::new(&size.to_string(), self.ident.span()));
                        operand_group_parsers.extend(quote! {
                            {
                                let operands = &mut gathered_operands[#group_index_lit];
                                parser.parse_operand_list(operands, Delimiter::Paren, /*allow_result_number=*/true, ::core::num::NonZeroU8::new(#size_lit))?;
                            }
                        });
                    } else {
                        operand_group_parsers.extend(quote! {
                            {
                                let operands = &mut gathered_operands[#group_index_lit];
                                parser.parse_operand_list(operands, Delimiter::Paren, /*allow_result_number=*/true, None)?;
                            }
                        });
                    }
                } else if let Some(size) = group.size {
                    let size_lit =
                        syn::Lit::Int(syn::LitInt::new(&size.to_string(), self.ident.span()));
                    operand_group_parsers.extend(quote! {
                        {
                            let operands = &mut gathered_operands[#group_index_lit];
                            parser.parse_operand_list(operands, Delimiter::None, /*allow_result_number=*/true, ::core::num::NonZeroU8::new(#size_lit))?;
                        }
                    });
                } else {
                    operand_group_parsers.extend(quote! {
                        {
                            let operands = &mut gathered_operands[#group_index_lit];
                            parser.parse_operand_list(operands, Delimiter::None, /*allow_result_number=*/true, None)?;
                        }
                    });
                }
            }

            gather_operands.extend(quote! {
                gathered_operands.resize(#total_operand_groups_lit, ::midenc_hir::SmallVec::default());
                state.operands.resize(#total_operand_groups_lit, ::midenc_hir::SmallVec::default());

                #operand_group_parsers
            });
        };

        let mut properties = quote! {};
        if !format.properties.is_empty() {
            properties.extend(quote! {
                if parser.token_stream_mut().next_if_eq(Token::Langle)? {
                    let mut props = ::midenc_hir::SmallVec::default();
                    parser.parse_attribute_dict(&mut props)?;
                    parser.parse_rangle()?;
                    state.attrs.extend(props);
                }
            });
        }

        let mut successors = quote! {};
        if !format.successor_groups.is_empty() {
            for succ_group in format.successor_groups.iter() {
                let field_name = &succ_group.field_name;
                let field_name_str =
                    syn::Lit::Str(syn::LitStr::new(&format!("{field_name}"), field_name.span()));
                let base_operand_group_lit = syn::Lit::Int(syn::LitInt::new(
                    &format!("{}", succ_group.base_operand_group),
                    field_name.span(),
                ));
                let delimiter: syn::Expr = if succ_group.requires_delimiter {
                    parse_quote!(Delimiter::None)
                } else {
                    parse_quote!(Delimiter::Bracket)
                };
                if let Some(key_ty) = succ_group.keyed.as_ref() {
                    successors.extend(quote! {
                            next_operand_group = 0;
                            parser.parse_comma_separated_list(#delimiter, Some(#field_name_str), |parser| {
                                let key_ty = <<#key_ty as ::midenc_hir::KeyedSuccessor>::KeyStorage as ::midenc_hir::attributes::MaybeInferAttributeType>::maybe_infer_type()
                                    .unwrap_or(::midenc_hir::Type::Unknown);
                                let key = parser.parse_typed_attribute::<<#key_ty as ::midenc_hir::KeyedSuccessor>::KeyStorage>(&key_ty)?.into_inner();
                                parser.parse_arrow()?;
                                let operand_group = #base_operand_group_lit + next_operand_group;
                                next_operand_group += 1;
                                if gathered_operands.len() == operand_group {
                                    gathered_operands.push(Default::default());
                                    state.operands.push(Default::default());
                                } else {
                                    assert!(gathered_operands.len() < operand_group);
                                    gathered_operands.insert(operand_group, Default::default());
                                    state.operands.insert(operand_group, Default::default());
                                }
                                let block_ref = parser.parse_successor_and_use_list(&mut state.operands[operand_group])?;
                                state.add_keyed_successor(key, block_ref.into_inner(), operand_group as u8);

                                Ok(true)
                            })?;
                        });
                } else if succ_group.successors.is_empty() {
                    successors.extend(quote! {
                            next_operand_group = 0;
                            parser.parse_comma_separated_list(#delimiter, Some(#field_name_str), |parser| {
                                let operand_group = #base_operand_group_lit + next_operand_group;
                                next_operand_group += 1;
                                if gathered_operands.len() == operand_group {
                                    gathered_operands.push(Default::default());
                                    state.operands.push(Default::default());
                                } else {
                                    assert!(gathered_operands.len() < operand_group);
                                    gathered_operands.insert(operand_group, Default::default());
                                    state.operands.insert(operand_group, Default::default());
                                }
                                let block_ref = parser.parse_successor_and_use_list(&mut state.operands[operand_group])?;
                                state.add_successor(block_ref.into_inner(), operand_group as u8);

                                Ok(true)
                            })?;
                        });
                } else {
                    for _ in succ_group.successors.iter() {
                        successors.extend(quote! {
                            {
                                let block_ref = parser.parse_successor_and_use_list(&mut state.operands[next_operand_group])?;
                                state.add_successor(block_ref.into_inner(), next_operand_group as u8);
                                next_operand_group += 1;
                            }
                        });
                    }
                }
            }
        }

        let regions = RegionParser { format: &format };

        let mut signature = proc_macro2::TokenStream::new();
        signature.extend(quote! {
            #[allow(unused_mut)]
            let mut sig_params = ::midenc_hir::SmallVec::<[::midenc_hir::Type; 4]>::default();
            #[allow(unused_mut)]
            let mut sig_results = ::midenc_hir::SmallVec::<[::midenc_hir::Type; 4]>::default();
        });
        if format.signature.can_infer {
            match &format.signature.results {
                ResultShape::None | ResultShape::Dynamic(_) => {}
                ResultShape::Static(results) => {
                    let len_lit = syn::Lit::Int(syn::LitInt::new(
                        &results.len().to_string(),
                        self.ident.span(),
                    ));
                    signature.extend(quote! {
                        sig_results.resize(#len_lit, ::midenc_hir::Type::Unknown);
                    });
                }
            }
        } else {
            let has_param_type = !matches!(&format.signature.params, ParamShape::None);
            match &format.signature.params {
                ParamShape::None => {}
                ParamShape::Static(_)
                | ParamShape::TrailingVarArgs { .. }
                | ParamShape::Dynamic(_) => {
                    signature.extend(quote! {
                        parser.parse_colon_type_list(&mut sig_params)?;
                    });
                }
            };
            match &format.signature.results {
                ResultShape::None => {}
                ResultShape::Static(_) => {
                    signature.extend(if has_param_type {
                        quote! {
                            parser.parse_arrow_type_list(&mut sig_results)?;
                        }
                    } else {
                        quote! {
                            parser.parse_colon_type_list(&mut sig_results)?;
                        }
                    });
                }
                ResultShape::Dynamic(_) => {
                    signature.extend(if has_param_type {
                        quote! {
                            parser.parse_optional_arrow_type_list(&mut sig_results)?;
                        }
                    } else {
                        quote! {
                            parser.parse_colon_type_list(&mut sig_results)?;
                        }
                    });
                }
            }
        };

        let mut resolve_operands = quote! {};
        if format.signature.can_infer {
            resolve_operands.extend(quote! {
                for (i, operand_group) in gathered_operands.iter_mut().enumerate() {
                    parser.resolve_operands_of_uniform_type(operand_group, &::midenc_hir::Type::Unknown, &mut state.operands[i])?;
                }
            });
        } else {
            resolve_operands.extend(quote! {
                let total_operand_types = sig_params.len();
                let total_operands = gathered_operands.iter().map(|group| group.len()).sum::<usize>() + state.operands.iter().map(|group| group.len()).sum::<usize>();
                if total_operand_types != total_operands {
                    return Err(ParserError::OperandAndTypeListMismatch {
                        span: state.span,
                        num_operands: total_operands,
                        num_types: total_operand_types,
                    });
                }
                let mut sig_params_iter = sig_params.into_iter();
                for (i, operand_group) in gathered_operands.iter_mut().enumerate() {
                    if operand_group.is_empty() {
                        continue;
                    }
                    for operand in operand_group.drain(..) {
                        let ty = sig_params_iter.next().unwrap();
                        let operand = parser.resolve_operand(operand, ty)?;
                        state.operands[i].push(operand);
                    }
                }
            });
        }
        resolve_operands.extend(quote! {
            state.results.extend(sig_results);
        });

        let op_type = &self.ident;
        let (impl_generics, ty_generics, where_clause) = self.generics.split_for_impl();
        tokens.extend(quote! {
            impl #impl_generics ::midenc_hir::OpParser for #op_type #ty_generics #where_clause {
                fn parse(state: &mut ::midenc_hir::OperationState, parser: &mut dyn ::midenc_hir::parse::OpAsmParser<'_>) -> ::midenc_hir::parse::ParseResult {
                    use ::midenc_hir::parse::*;
                    #gather_operands
                    #properties
                    #successors
                    #regions
                    let mut attrs = ::midenc_hir::SmallVec::default();
                    parser.parse_optional_attribute_dict_with_keyword(&mut attrs)?;
                    state.attrs.extend(attrs);
                    #signature
                    #resolve_operands

                    Ok(())
                }
            }
        });
    }
}

struct RegionParser<'a> {
    format: &'a OperationFormat,
}

impl quote::ToTokens for RegionParser<'_> {
    fn to_tokens(&self, tokens: &mut proc_macro2::TokenStream) {
        let isolated =
            syn::Lit::Bool(syn::LitBool::new(self.format.isolated_from_above, Span::call_site()));

        let mut args = quote! {};
        match &self.format.signature.params {
            ParamShape::None => {
                args.extend(quote! {
                    #[allow(unused_variables, unused_mut)]
                    let mut args = ::midenc_hir::SmallVec::<[::midenc_hir::parse::Argument; 1]>::new_const();
                });
            }
            ParamShape::Static(group_index) => {
                let group_index = *group_index;
                assert!(
                    self.format
                        .successor_groups
                        .iter()
                        .all(|group| group.base_operand_group > group_index
                            || !group.successors.is_empty()),
                    "dynamically-sized successor groups must be defined after all other operand \
                     groups"
                );
                let group = &self.format.operand_groups[group_index];
                let group_size = syn::Lit::Int(syn::LitInt::new(
                    &group.operands.len().to_string(),
                    group.field_name.span(),
                ));
                args.extend(quote! {
                    let mut args = ::midenc_hir::SmallVec::<[::midenc_hir::parse::Argument; #group_size]>::new_const();
                });

                let operand_group = syn::Lit::Int(syn::LitInt::new(
                    &group_index.to_string(),
                    group.field_name.span(),
                ));
                for operand in group.operands.iter() {
                    let operand_index = syn::Lit::Int(syn::LitInt::new(
                        &operand.index.to_string(),
                        group.field_name.span(),
                    ));
                    args.extend(quote! {
                        args.push(::midenc_hir::parse::Argument {
                            name: gathered_operands[#operand_group][#operand_index],
                            ty: ::midenc_hir::Type::Unknown,
                            attrs: Default::default(),
                            loc: ::midenc_hir::dialects::builtin::attributes::Location::Unknown,
                        });
                    });
                }
            }
            ParamShape::Dynamic(group_index) => {
                let group_index = *group_index;
                assert!(
                    self.format
                        .successor_groups
                        .iter()
                        .all(|group| group.base_operand_group > group_index
                            || !group.successors.is_empty()),
                    "dynamically-sized successor groups must be defined after all other operand \
                     groups"
                );
                let group = &self.format.operand_groups[group_index];
                args.extend(quote! {
                    let mut args = ::midenc_hir::SmallVec::<[::midenc_hir::parse::Argument; 1]>::new_const();
                });

                let operand_group = syn::Lit::Int(syn::LitInt::new(
                    &group_index.to_string(),
                    group.field_name.span(),
                ));
                args.extend(quote! {
                    args.extend(gathered_operands[#operand_group].iter().copied().map(|operand| {
                        ::midenc_hir::parse::Argument {
                            name: operand,
                            ty: ::midenc_hir::Type::Unknown,
                            attrs: Default::default(),
                            loc: ::midenc_hir::dialects::builtin::attributes::Location::Unknown,
                        }
                    }));
                });
            }
            ParamShape::TrailingVarArgs { fixed, varargs } => {
                let fixed_group_index = *fixed;
                let varargs_group_index = *varargs;
                assert!(
                    self.format
                        .successor_groups
                        .iter()
                        .all(|group| ((group.base_operand_group > fixed_group_index)
                            && (group.base_operand_group > varargs_group_index))
                            || !group.successors.is_empty()),
                    "dynamically-sized successor groups must be defined after all other operand \
                     groups"
                );
                let fixed_group = &self.format.operand_groups[fixed_group_index];
                let group_size = syn::Lit::Int(syn::LitInt::new(
                    &fixed_group.operands.len().to_string(),
                    fixed_group.field_name.span(),
                ));

                args.extend(quote! {
                    let mut args = ::midenc_hir::SmallVec::<[::midenc_hir::parse::Argument; #group_size]>::new_const();
                });

                let operand_group = syn::Lit::Int(syn::LitInt::new(
                    &fixed_group_index.to_string(),
                    fixed_group.field_name.span(),
                ));
                for operand in fixed_group.operands.iter() {
                    let operand_index = syn::Lit::Int(syn::LitInt::new(
                        &operand.index.to_string(),
                        fixed_group.field_name.span(),
                    ));
                    args.extend(quote! {
                        args.push(::midenc_hir::parse::Argument {
                            name: gathered_operands[#operand_group][#operand_index],
                            ty: ::midenc_hir::Type::Unknown,
                            attrs: Default::default(),
                            loc: ::midenc_hir::dialects::builtin::attributes::Location::Unknown,
                        });
                    });
                }
                let varargs_group = &self.format.operand_groups[varargs_group_index];
                let operand_group = syn::Lit::Int(syn::LitInt::new(
                    &varargs_group_index.to_string(),
                    varargs_group.field_name.span(),
                ));
                args.extend(quote! {
                    args.extend(gathered_operands[#operand_group].iter().copied().map(|operand| {
                        ::midenc_hir::parse::Argument {
                            name: operand,
                            ty: ::midenc_hir::Type::Unknown,
                            attrs: Default::default(),
                            loc: ::midenc_hir::dialects::builtin::attributes::Location::Unknown,
                        }
                    }));
                });
            }
        }
        let elide_entry_region_name = self.format.regions.len() == 1;
        for (
            i,
            RegionInfo {
                name: region_field_name,
                options: RegionOptions { name: region_alias },
            },
        ) in self.format.regions.iter().enumerate()
        {
            let is_entry_region = i == 0;
            let region_name = if let Some(region_alias) = region_alias.as_deref() {
                syn::Lit::Str(syn::LitStr::new(region_alias, region_field_name.span()))
            } else {
                syn::Lit::Str(syn::LitStr::new(
                    &region_field_name.to_string(),
                    region_field_name.span(),
                ))
            };

            let mut parse_region = quote! {};
            if is_entry_region {
                if elide_entry_region_name {
                    parse_region.extend(quote! {
                        if let Some(region) = parser.parse_optional_region(&args, #isolated)? {
                            state.add_region(region);
                        } else {
                            let region = parser.context().create_region();
                            state.add_region(region);
                        }
                    });
                } else {
                    parse_region.extend(quote! {
                        if let Some(region) = parser.parse_optional_region_with_token(#region_name, &args, #isolated)? {
                            state.add_region(region);
                        } else {
                            let region = parser.context().create_region();
                            state.add_region(region);
                        }
                    });
                }
                tokens.extend(quote! {
                    // #region_name
                    {
                        #args
                        #parse_region
                    }
                });
            } else {
                tokens.extend(quote! {
                    if let Some(region) = parser.parse_optional_region_with_token(#region_name, &[], #isolated)? {
                        state.add_region(region);
                    } else {
                        let region = parser.context().create_region();
                        state.add_region(region);
                    }
                });
            }
        }
    }
}
