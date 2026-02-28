use darling::FromDeriveInput;
use quote::quote;
use syn::Ident;

use crate::operation::OperationField;

pub fn derive_op_printer(input: &syn::DeriveInput) -> darling::Result<DeriveOpPrinter> {
    DeriveOpPrinter::from_derive_input(input)
}

/// Represents the parsed struct definition for the operation we wish to define
///
/// Only named structs are allowed at this time.
#[derive(Debug, FromDeriveInput)]
#[darling(
    attributes(printer),
    supports(struct_named),
    forward_attrs(doc, cfg, allow, derive)
)]
pub struct DeriveOpPrinter {
    ident: Ident,
    generics: syn::Generics,
    data: darling::ast::Data<(), crate::operation::OperationField>,
}

impl quote::ToTokens for DeriveOpPrinter {
    fn to_tokens(&self, tokens: &mut proc_macro2::TokenStream) {
        let struct_data = self.data.as_ref().take_struct().unwrap();
        let has_successors = struct_data
            .fields
            .iter()
            .any(|f| f.attrs.successor.is_present() || f.attrs.successors.is_some());
        let has_properties = struct_data
            .fields
            .iter()
            .any(|f| f.attrs.attr.is_some() || f.attrs.symbol.is_some());
        let has_results = struct_data
            .fields
            .iter()
            .any(|f| f.attrs.result.is_present() || f.attrs.results.is_present());
        let min_result_count = struct_data
            .fields
            .iter()
            .map(|f| if f.attrs.result.is_present() { 1 } else { 0 })
            .sum::<usize>();

        let properties = if has_properties {
            quote! {
                *printer += midenc_hir::formatter::const_text(" <");
                printer.print_attribute_dictionary(op.properties());
                *printer += midenc_hir::formatter::const_text(">");
            }
        } else {
            quote! {}
        };

        let regions = RegionPrinter {
            fields: &struct_data.fields,
        };

        let successors = if has_successors {
            let keyed_successors = struct_data.fields.iter().find(|s| s.attrs.successors.is_some());
            if let Some(field) = keyed_successors {
                let field_name = field.ident.as_ref().unwrap();
                quote! {
                    printer.print_space();
                    let mut p = ::midenc_hir::print::AsmPrinter::new(op.context_rc(), printer.flags());
                    for (i, keyed_succ) in self.#field_name().iter().enumerate() {
                        use ::midenc_hir::AsValueRange;
                        if i > 0 {
                            p += ::midenc_hir::formatter::nl();
                        }
                        let dest = keyed_succ.block();
                        let operands = keyed_succ.arguments();
                        if let Some(key_attr_ref) = keyed_succ.key_storage() {
                            p.print_attribute_value(&*key_attr_ref.borrow());
                        } else {
                            p.print_keyword("default");
                        }
                        p += ::midenc_hir::formatter::const_text(" => ");
                        p += ::midenc_hir::formatter::display(dest.borrow().id());
                        if operands.is_empty() {
                            continue;
                        }
                        p += ::midenc_hir::formatter::const_text(":(");
                        p.print_value_uses(operands.as_value_range());
                        p += ::midenc_hir::formatter::const_text(")");
                    }
                    *printer += ::midenc_hir::formatter::indent(4, p.finish());
                }
            } else {
                quote! {
                    printer.print_space();
                    for (i, succ) in op.successors().iter().enumerate() {
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
                }
            }
        } else {
            quote! {}
        };

        let results = if has_results {
            if min_result_count > 1 {
                quote! {
                    if !op.implements::<dyn ::midenc_hir::traits::InferTypeOpInterface>() {
                        printer.print_space();
                        printer.print_colon_type_list(op.results().all().iter().map(|r| ::alloc::borrow::Cow::Owned(r.borrow().ty().clone())));
                    }
                }
            } else {
                let result_field = struct_data
                    .fields
                    .iter()
                    .find_map(|f| {
                        if f.attrs.result.is_present() || f.attrs.results.is_present() {
                            f.ident.clone()
                        } else {
                            None
                        }
                    })
                    .unwrap();
                quote! {
                    if !op.implements::<dyn ::midenc_hir::traits::InferTypeOpInterface>() {
                        printer.print_space();
                        printer.print_colon_type(&self.#result_field().ty());
                    }
                }
            }
        } else {
            quote! {}
        };

        let mut num_non_successor_operand_groups = 0usize;
        let mut current_group_size = 0usize;
        let mut next_group_index = 0usize;
        let operand_groups_to_print = struct_data
            .fields
            .iter()
            .filter_map(|f| {
                if f.attrs.operand.is_present() {
                    if current_group_size == 0 {
                        let index = next_group_index;
                        next_group_index += 1;
                        num_non_successor_operand_groups += 1;
                        current_group_size = 2;
                        Some(syn::Lit::Int(syn::LitInt::new(
                            &index.to_string(),
                            f.attrs.operands.span(),
                        )))
                    } else if current_group_size == 1 {
                        // Previous group was an #[operands] group, so we start a new group
                        let index = next_group_index;
                        next_group_index += 1;
                        num_non_successor_operand_groups += 1;
                        current_group_size = 2;
                        Some(syn::Lit::Int(syn::LitInt::new(
                            &index.to_string(),
                            f.attrs.operands.span(),
                        )))
                    } else {
                        current_group_size += 1;
                        None
                    }
                } else if f.attrs.operands.is_present() {
                    let index = next_group_index;
                    next_group_index += 1;
                    num_non_successor_operand_groups += 1;
                    current_group_size = 1;
                    Some(syn::Lit::Int(syn::LitInt::new(
                        &index.to_string(),
                        f.attrs.operands.span(),
                    )))
                } else if f.attrs.successor.is_present() || f.attrs.successors.is_some() {
                    next_group_index += 1;
                    current_group_size = 1;
                    None
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();

        let operands = if num_non_successor_operand_groups == 1 {
            quote! {
                #(
                    if !op.operands().group(#operand_groups_to_print).is_empty() {
                        use ::midenc_hir::AsValueRange;
                        printer.print_space();
                        printer.print_value_uses(op.operands().group(#operand_groups_to_print).as_value_range());
                    }
                )*
            }
        } else {
            quote! {
                #(
                    printer.print_space();
                    printer.print_operand_list(op.operands().group(#operand_groups_to_print));
                )*
            }
        };

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
                    #results
                }
            }
        });
    }
}

struct RegionPrinter<'a, 'f: 'a> {
    fields: &'a Vec<&'f OperationField>,
}

impl quote::ToTokens for RegionPrinter<'_, '_> {
    fn to_tokens(&self, tokens: &mut proc_macro2::TokenStream) {
        for region_field in self.fields.iter().filter(|f| f.attrs.region.is_present()) {
            let region_field_name = region_field.ident.as_ref().unwrap();
            let region_name = syn::Lit::Str(syn::LitStr::new(
                &region_field_name.to_string(),
                region_field_name.span(),
            ));
            tokens.extend(quote! {
                printer.print_space();
                printer.print_keyword(#region_name);
                printer.print_space();
                printer.print_region(&self.#region_field_name());
            });
        }
    }
}
