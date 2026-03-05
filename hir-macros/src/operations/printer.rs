use darling::FromDeriveInput;
use quote::quote;
use syn::Ident;

use crate::operation::{OperationFormat, ParamShape, RegionInfo, RegionOptions, ResultShape};

pub fn derive_op_printer(input: &syn::DeriveInput) -> darling::Result<DeriveOpPrinter> {
    DeriveOpPrinter::from_derive_input(input)
}

/// Represents the parsed struct definition for the operation we wish to define
///
/// Only named structs are allowed at this time.
#[derive(Debug, FromDeriveInput)]
#[darling(
    attributes(operation, printer),
    supports(struct_named),
    forward_attrs(doc, cfg, allow, derive)
)]
pub struct DeriveOpPrinter {
    ident: Ident,
    generics: syn::Generics,
    data: darling::ast::Data<(), crate::operation::OperationField>,
    #[allow(unused)]
    dialect: Ident,
    #[allow(unused)]
    #[darling(default)]
    name: Option<Ident>,
    #[darling(default)]
    traits: darling::util::PathList,
    #[darling(default)]
    implements: darling::util::PathList,
}

impl quote::ToTokens for DeriveOpPrinter {
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

        let mut operands = quote! {};
        match &format.signature.params {
            ParamShape::None => {}
            ParamShape::Static(operand_group) => {
                let operand_group_index =
                    syn::Lit::Int(syn::LitInt::new(&operand_group.to_string(), self.ident.span()));
                operands.extend(quote! {
                    {
                        use ::midenc_hir::AsValueRange;
                        printer.print_space();
                        printer.print_value_uses(op.operands().group(#operand_group_index).as_value_range());
                    }
                });
            }
            ParamShape::TrailingVarArgs { fixed, varargs } => {
                let fixed_group_index =
                    syn::Lit::Int(syn::LitInt::new(&fixed.to_string(), self.ident.span()));
                let varargs_group_index =
                    syn::Lit::Int(syn::LitInt::new(&varargs.to_string(), self.ident.span()));
                operands.extend(quote! {
                    {
                        use ::midenc_hir::AsValueRange;
                        printer.print_space();
                        printer.print_value_uses(op.operands().group(#fixed_group_index).as_value_range());

                        if !op.operands().group(#varargs_group_index).is_empty() {
                            *printer += ::midenc_hir::formatter::const_text(", ");
                            printer.print_value_uses(op.operands().group(#varargs_group_index).as_value_range());
                        }
                    }
                });
            }
            ParamShape::Dynamic(operand_group_index) => {
                let operand_group = &format.operand_groups[*operand_group_index];
                let operand_group_index_lit = syn::Lit::Int(syn::LitInt::new(
                    &operand_group_index.to_string(),
                    self.ident.span(),
                ));

                operands.extend(if operand_group.requires_delimiter {
                    quote! {
                        if !op.operands().group(#operand_group_index_lit).is_empty() {
                            use ::midenc_hir::AsValueRange;
                            printer.print_space();
                            printer.print_operand_list(op.operands().group(#operand_group_index_lit));
                        }
                    }
                } else {
                    quote! {
                        if !op.operands().group(#operand_group_index_lit).is_empty() {
                            use ::midenc_hir::AsValueRange;
                            printer.print_space();
                            printer.print_value_uses(op.operands().group(#operand_group_index_lit).as_value_range());
                        }
                    }
                });
            }
        };

        let mut properties = quote! {};
        if !format.properties.is_empty() {
            properties.extend(quote! {
                *printer += midenc_hir::formatter::const_text(" <");
                printer.print_attribute_dictionary(op.properties());
                *printer += midenc_hir::formatter::const_text(">");
            });
        }

        let mut successors = quote! {};
        if !format.successor_groups.is_empty() {
            for (succ_group_index, succ_group) in format.successor_groups.iter().enumerate() {
                let field_name = &succ_group.field_name;
                if succ_group.keyed.is_some() {
                    successors.extend(quote! {
                        printer.print_space();
                        let mut p = ::midenc_hir::print::AsmPrinter::new(op.context_rc(), printer.flags());
                        for (i, keyed_succ) in self.#field_name().iter().enumerate() {
                            use ::midenc_hir::AsValueRange;
                            if i > 0 {
                                p += ::midenc_hir::formatter::const_text(", ");
                                p += ::midenc_hir::formatter::nl();
                            }
                            let dest = keyed_succ.block();
                            let operands = keyed_succ.arguments();
                            let key_attr_ref = keyed_succ.key_storage();
                            p.print_attribute_value(&*key_attr_ref.borrow());
                            p += ::midenc_hir::formatter::const_text(" -> ");
                            p += ::midenc_hir::formatter::display(dest.borrow().id());
                            if operands.is_empty() {
                                continue;
                            }
                            p += ::midenc_hir::formatter::const_text(":(");
                            p.print_value_uses(operands.as_value_range());
                            p += ::midenc_hir::formatter::const_text(")");
                        }
                        *printer += ::midenc_hir::formatter::indent(4, p.finish());
                    });
                } else if succ_group.successors.is_empty() && succ_group.requires_delimiter {
                    successors.extend(quote! {
                        printer.print_space();
                        printer.print_lbracket();
                        for (i, succ) in self.#field_name().iter().enumerate() {
                            if i > 0 {
                                *printer += ::midenc_hir::formatter::const_text(", ");
                            }
                            let target = succ.successor();
                            let target_operands = succ.successor_operands();
                            *printer += ::midenc_hir::formatter::display(target.borrow().id());
                            if target_operands.is_empty() {
                                continue;
                            }
                            *printer += ::midenc_hir::formatter::const_text(":(");
                            printer.print_value_uses(target_operands);
                            *printer += ::midenc_hir::formatter::const_text(")");
                        }
                        printer.print_rbracket();
                    });
                } else if succ_group.successors.is_empty() {
                    successors.extend(quote! {
                        printer.print_space();
                        for (i, succ) in self.#field_name().iter().enumerate() {
                            if i > 0 {
                                *printer += ::midenc_hir::formatter::const_text(", ");
                            }
                            let target = succ.successor();
                            let target_operands = succ.successor_operands();
                            *printer += ::midenc_hir::formatter::display(target.borrow().id());
                            if target_operands.is_empty() {
                                continue;
                            }
                            *printer += ::midenc_hir::formatter::const_text(":(");
                            printer.print_value_uses(target_operands);
                            *printer += ::midenc_hir::formatter::const_text(")");
                        }
                    });
                } else {
                    if succ_group_index > 0 || succ_group.index > 0 {
                        successors.extend(quote! {
                            *printer += ::midenc_hir::formatter::const_text(", ");
                        });
                    } else {
                        successors.extend(quote! {
                            printer.print_space();
                        });
                    }
                    for (i, succ) in succ_group.successors.iter().enumerate() {
                        let field_name = &succ.field;
                        if i > 0 {
                            successors.extend(quote! {
                                *printer += ::midenc_hir::formatter::const_text(", ");
                            });
                        }
                        successors.extend(quote! {
                            {
                                use ::midenc_hir::AsValueRange;
                                let succ = self.#field_name();
                                let target = succ.successor();
                                let target_operands = &succ.arguments;
                                *printer += ::midenc_hir::formatter::display(target.borrow().id());
                                if !target_operands.is_empty() {
                                    *printer += ::midenc_hir::formatter::const_text(":(");
                                    printer.print_value_uses(target_operands.as_value_range());
                                    *printer += ::midenc_hir::formatter::const_text(")");
                                }
                            }
                        });
                    }
                }
            }
        }

        let regions = RegionPrinter { format: &format };

        let mut signature = quote! {};
        if !format.signature.can_infer {
            let has_param_type = !matches!(&format.signature.params, ParamShape::None);
            match &format.signature.params {
                ParamShape::None => {}
                ParamShape::Static(group) | ParamShape::Dynamic(group) => {
                    let operand_group_index =
                        syn::Lit::Int(syn::LitInt::new(&group.to_string(), self.ident.span()));
                    signature.extend(quote! {
                        printer.print_space();
                        printer.print_colon_type_list(op.operands().group(#operand_group_index).iter().map(|r| ::alloc::borrow::Cow::Owned(r.borrow().ty().clone())));
                    });
                }
                ParamShape::TrailingVarArgs { fixed, varargs } => {
                    let fixed_group_index =
                        syn::Lit::Int(syn::LitInt::new(&fixed.to_string(), self.ident.span()));
                    let varargs_group_index =
                        syn::Lit::Int(syn::LitInt::new(&varargs.to_string(), self.ident.span()));
                    signature.extend(quote! {
                        {
                            let fixed_group = op.operands().group(#fixed_group_index).into_iter();
                            let varargs_group = op.operands().group(#varargs_group_index).into_iter();
                            printer.print_space();
                            printer.print_colon_type_list(fixed_group.chain(varargs_group).map(|r| ::alloc::borrow::Cow::Owned(r.borrow().ty().clone())));
                        }
                    });
                }
            }
            match &format.signature.results {
                ResultShape::None => {}
                ResultShape::Static(_) => {
                    signature.extend(if has_param_type {
                        quote! {
                            printer.print_space();
                            printer.print_arrow();
                            printer.print_space();
                            printer.print_type_list(op.results().all().iter().map(|r| ::alloc::borrow::Cow::Owned(r.borrow().ty().clone())));
                        }
                    } else {
                        quote! {
                            printer.print_space();
                            printer.print_colon_type_list(op.results().all().iter().map(|r| ::alloc::borrow::Cow::Owned(r.borrow().ty().clone())));
                        }
                    });
                }
                ResultShape::Dynamic(_) => {
                    signature.extend(if has_param_type {
                        quote! {
                            printer.print_space();
                            printer.print_arrow();
                            printer.print_space();
                            printer.print_type_list(op.results().all().iter().map(|r| ::alloc::borrow::Cow::Owned(r.borrow().ty().clone())));
                        }
                    } else {
                        quote! {
                            printer.print_space();
                            printer.print_colon_type_list(op.results().all().iter().map(|r| ::alloc::borrow::Cow::Owned(r.borrow().ty().clone())));
                        }
                    });
                }
            }
        }

        let op_type = &self.ident;
        let (impl_generics, ty_generics, where_clause) = self.generics.split_for_impl();
        tokens.extend(quote! {
            impl #impl_generics ::midenc_hir::OpPrinter for #op_type #ty_generics #where_clause {
                fn print(&self, printer: &mut ::midenc_hir::print::AsmPrinter<'_>) {
                    let op = <Self as ::midenc_hir::Op>::as_operation(self);
                    #operands
                    #properties
                    #successors
                    #regions
                    if op.has_attributes() {
                        printer.print_space();
                        printer.print_keyword("attributes");
                        printer.print_space();
                        printer.print_attribute_dictionary(op.attributes().iter().map(|attr| *attr.as_named_attribute()));
                    }
                    #signature
                }
            }
        });
    }
}

struct RegionPrinter<'a> {
    format: &'a OperationFormat,
}

impl quote::ToTokens for RegionPrinter<'_> {
    fn to_tokens(&self, tokens: &mut proc_macro2::TokenStream) {
        let elide_region_name = self.format.regions.len() == 1;
        for RegionInfo {
            name: region_field_name,
            options: RegionOptions { name: region_alias },
        } in self.format.regions.iter()
        {
            let region_name = if let Some(region_alias) = region_alias.as_deref() {
                syn::Lit::Str(syn::LitStr::new(region_alias, region_field_name.span()))
            } else {
                syn::Lit::Str(syn::LitStr::new(
                    &region_field_name.to_string(),
                    region_field_name.span(),
                ))
            };
            if elide_region_name {
                tokens.extend(quote! {
                    {
                        let region = self.#region_field_name();
                        if !region.is_empty() {
                            printer.print_space();
                            printer.print_region(&region);
                        }
                    }
                });
            } else {
                tokens.extend(quote! {
                    {
                        let region = self.#region_field_name();
                        if !region.is_empty() {
                            printer.print_space();
                            printer.print_keyword(#region_name);
                            printer.print_space();
                            printer.print_region(&region);
                        }
                    }
                });
            }
        }
    }
}
