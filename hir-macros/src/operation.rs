use std::{num::NonZeroU8, rc::Rc};

use darling::{
    Error, FromDeriveInput, FromField, FromMeta,
    util::{Flag, SpannedValue},
};
use inflector::Inflector;
use quote::{ToTokens, format_ident, quote, quote_spanned};
use syn::{Ident, Token, parse_quote, parse_quote_spanned, spanned::Spanned};

pub fn derive_operation(input: syn::DeriveInput) -> darling::Result<proc_macro2::TokenStream> {
    let op = OpDefinition::from_derive_input(&input)?;

    Ok(op.into_token_stream())
}

/// This struct represents the fully parsed and prepared definition of an operation, along with all
/// of its associated items, trait impls, etc.
pub struct OpDefinition {
    /// The span of the original item decorated with `#[operation]`
    span: proc_macro2::Span,
    /// The name of the dialect type corresponding to the dialect this op belongs to
    dialect: Ident,
    /// The type name of the concrete `Op` implementation, i.e. the item with `#[operation]` on it
    name: Ident,
    /// The name of the operation in the textual form of the IR, e.g. `Add` would be `add`.
    opcode: Ident,
    /// The set of paths corresponding to the op traits we need to generate impls for
    traits: darling::util::PathList,
    /// The set of paths corresponding to the op traits manually implemented by this op
    implements: darling::util::PathList,
    /// The named regions declared for this op
    regions: Vec<Ident>,
    /// The inherent attributes declared for this op
    attrs: Vec<OpAttribute>,
    /// The named operands, and operand groups, declared for this op
    ///
    /// Sequential individually named operands are collected into an "unnamed" operand group, i.e.
    /// the group is not named, only the individual operands. Conversely, each "named" operand group
    /// can refer to the group by name, but not the individual operands.
    operands: Vec<OpOperandGroup>,
    /// The named results of this operation
    ///
    /// An operation can have no results, one or more individually named results, or a single named
    /// result group, but not a combination.
    results: Option<OpResultGroup>,
    /// The named successors, and successor groups, declared for this op.
    ///
    /// This is represented almost identically to `operands`, except we also support successor
    /// groups with "keyed" items represented by an implementation of the `KeyedSuccessor` trait.
    /// Keyed successor groups are handled a bit differently than "normal" successor groups in terms
    /// of the types expected by the op builder for this type.
    successors: Vec<SuccessorGroup>,
    /// The symbolic references held by this op
    symbols: Vec<Symbol>,
    /// The struct definition
    op: syn::ItemStruct,
    /// The implementation of `{Op}Builder` for this op.
    op_builder_impl: OpBuilderImpl,
    /// The implementation of `OpVerifier` for this op.
    op_verifier_impl: OpVerifierImpl,
}
impl OpDefinition {
    /// Initialize an [OpDefinition] from the parsed [Operation] received as input
    fn from_operation(span: proc_macro2::Span, op: &mut Operation) -> darling::Result<Self> {
        let dialect = op.dialect.clone();
        let name = op.ident.clone();
        let opcode = op.name.clone().unwrap_or_else(|| {
            let name = name.to_string().to_snake_case();
            let name = name.strip_suffix("Op").unwrap_or(name.as_str());
            format_ident!("{name}", span = name.span())
        });
        let traits = core::mem::take(&mut op.traits);
        let implements = core::mem::take(&mut op.implements);

        let fields = core::mem::replace(
            &mut op.data,
            darling::ast::Data::Struct(darling::ast::Fields::new(
                darling::ast::Style::Struct,
                vec![],
            )),
        )
        .take_struct()
        .unwrap();

        let mut named_fields = syn::punctuated::Punctuated::<syn::Field, Token![,]>::new();
        // Add the `op` field (which holds the underlying Operation)
        named_fields.push(syn::Field {
            attrs: vec![],
            vis: syn::Visibility::Inherited,
            mutability: syn::FieldMutability::None,
            ident: Some(format_ident!("op")),
            colon_token: Some(syn::token::Colon(span)),
            ty: make_type("::midenc_hir::Operation"),
        });

        let op = syn::ItemStruct {
            attrs: core::mem::take(&mut op.attrs),
            vis: op.vis.clone(),
            struct_token: syn::token::Struct(span),
            ident: name.clone(),
            generics: core::mem::take(&mut op.generics),
            fields: syn::Fields::Named(syn::FieldsNamed {
                brace_token: syn::token::Brace(span),
                named: named_fields,
            }),
            semi_token: None,
        };

        let op_builder_impl = OpBuilderImpl::empty(name.clone());
        let op_verifier_impl =
            OpVerifierImpl::new(name.clone(), traits.clone(), implements.clone());

        let mut this = Self {
            span,
            dialect,
            name,
            opcode,
            traits,
            implements,
            regions: vec![],
            attrs: vec![],
            operands: vec![],
            results: None,
            successors: vec![],
            symbols: vec![],
            op,
            op_builder_impl,
            op_verifier_impl,
        };

        this.hydrate(fields)?;

        Ok(this)
    }

    fn hydrate(&mut self, fields: darling::ast::Fields<OperationField>) -> darling::Result<()> {
        let named_fields = match &mut self.op.fields {
            syn::Fields::Named(syn::FieldsNamed { named, .. }) => named,
            _ => unreachable!(),
        };
        let mut create_params = vec![];
        let (_, mut fields) = fields.split();

        // Compute the absolute ordering of op parameters as follows:
        //
        // * By default, the ordering is implied by the order of field declarations in the struct
        // * A field can be decorated with #[order(N)], where `N` is an absolute index
        // * If all fields have an explicit order, then the sort following that order is used
        // * If a mix of fields have explicit ordering, so as to acheive a particular struct layout,
        //   then the implicit order given to a field ensures that it appears after the highest
        //   ordered field which comes before it in the struct. For example, if I have the following
        //   pseudo-struct definition: `{ #[order(2)] a, b, #[order(1)] c, d }`, then the actual
        //   order of the parameters corresponding to those fields will be `c`, `a`, `b`, `d`. This
        //   is due to the fact that a.) `b` is assigned an index of `3` because it is the next
        //   available index following `2`, which was assigned to `a` before it in the struct, and
        //   2.) `d` is assigned an index of `4`, as it is the next highest available index after
        //   `2`, which is the highest explicitly ordered field that is defined before it in the
        //   struct.
        let mut assigned_highwater = 0;
        let mut highwater = 0;
        let mut claimed_indices = fields.iter().filter_map(|f| f.attrs.order).collect::<Vec<_>>();
        claimed_indices.sort();
        claimed_indices.dedup();
        for field in fields.iter_mut() {
            match field.attrs.order {
                // If this order precedes a previous #[order] field, skip it
                Some(order) if highwater > order => continue,
                Some(order) => {
                    // Move high water mark to `order`
                    highwater = order;
                }
                None => {
                    // Find the next unused index > `highwater` && `assigned_highwater`
                    assigned_highwater = core::cmp::max(assigned_highwater, highwater);
                    let mut next_index = assigned_highwater + 1;
                    while claimed_indices.contains(&next_index) {
                        next_index += 1;
                    }
                    assigned_highwater = next_index;
                    field.attrs.order = Some(next_index);
                }
            }
        }
        fields.sort_by_key(|field| field.attrs.order);

        for field in fields {
            let field_name = field.ident.clone().unwrap();
            let field_span = field_name.span();
            let field_ty = field.ty.clone();

            let op_field_ty = field.attrs.pseudo_type();
            match op_field_ty.as_deref() {
                // Forwarded field
                None => {
                    create_params.push(OpCreateParam {
                        param_ty: OpCreateParamType::CustomField(field_name.clone(), field_ty),
                        r#default: field.attrs.default.is_present(),
                    });
                    named_fields.push(syn::Field {
                        attrs: field.attrs.forwarded,
                        vis: field.vis,
                        mutability: syn::FieldMutability::None,
                        ident: Some(field_name),
                        colon_token: Some(syn::token::Colon(field_span)),
                        ty: field.ty,
                    });
                }
                Some(OperationFieldType::Attr(kind)) => {
                    let value_ty = parse_quote_spanned! { field_span =>
                        <#field_ty as ::midenc_hir::AttributeRegistration>::Value
                    };
                    let ty = parse_quote_spanned! { field_span =>
                        ::midenc_hir::UnsafeIntrusiveEntityRef<#field_ty>
                    };
                    let attr = OpAttribute {
                        name: field_name.clone(),
                        ty: field_ty.clone(),
                        value_ty,
                        kind: *kind,
                    };
                    create_params.push(OpCreateParam {
                        param_ty: OpCreateParamType::Attr(attr.clone()),
                        r#default: field.attrs.default.is_present(),
                    });
                    named_fields.push(syn::Field {
                        attrs: field.attrs.forwarded,
                        vis: field.vis,
                        mutability: syn::FieldMutability::None,
                        ident: Some(field_name),
                        colon_token: Some(syn::token::Colon(field_span)),
                        ty,
                    });
                    self.attrs.push(attr);
                }
                Some(OperationFieldType::Operand) => {
                    let operand = Operand {
                        name: field_name.clone(),
                        constraint: field_ty,
                    };
                    create_params.push(OpCreateParam {
                        param_ty: OpCreateParamType::Operand(operand.clone()),
                        r#default: field.attrs.default.is_present(),
                    });
                    match self.operands.last_mut() {
                        None => {
                            self.operands.push(OpOperandGroup::Unnamed(vec![operand]));
                        }
                        Some(OpOperandGroup::Unnamed(operands)) => {
                            operands.push(operand);
                        }
                        Some(OpOperandGroup::Named(..)) => {
                            // Start a new group
                            self.operands.push(OpOperandGroup::Unnamed(vec![operand]));
                        }
                    }
                }
                Some(OperationFieldType::Operands) => {
                    create_params.push(OpCreateParam {
                        param_ty: OpCreateParamType::OperandGroup(
                            field_name.clone(),
                            field_ty.clone(),
                        ),
                        r#default: field.attrs.default.is_present(),
                    });
                    self.operands.push(OpOperandGroup::Named(field_name, field_ty));
                }
                Some(OperationFieldType::Result) => {
                    let result = OpResult {
                        name: field_name.clone(),
                        constraint: field_ty,
                    };
                    match self.results.as_mut() {
                        None => {
                            self.results = Some(OpResultGroup::Unnamed(vec![result]));
                        }
                        Some(OpResultGroup::Unnamed(results)) => {
                            results.push(result);
                        }
                        Some(OpResultGroup::Named(..)) => {
                            return Err(Error::custom("#[result] and #[results] cannot be mixed")
                                .with_span(&field_name));
                        }
                    }
                }
                Some(OperationFieldType::Results) => match self.results.as_mut() {
                    None => {
                        self.results = Some(OpResultGroup::Named(field_name, field_ty));
                    }
                    Some(OpResultGroup::Unnamed(_)) => {
                        return Err(Error::custom("#[result] and #[results] cannot be mixed")
                            .with_span(&field_name));
                    }
                    Some(OpResultGroup::Named(..)) => {
                        return Err(Error::custom("#[results] may only appear on a single field")
                            .with_span(&field_name));
                    }
                },
                Some(OperationFieldType::Region(_options)) => {
                    self.regions.push(field_name);
                }
                Some(OperationFieldType::Successor) => {
                    create_params.push(OpCreateParam {
                        param_ty: OpCreateParamType::Successor(field_name.clone()),
                        r#default: field.attrs.default.is_present(),
                    });
                    match self.successors.last_mut() {
                        None => {
                            self.successors.push(SuccessorGroup::Unnamed(vec![field_name]));
                        }
                        Some(SuccessorGroup::Unnamed(ids)) => {
                            ids.push(field_name);
                        }
                        Some(SuccessorGroup::Named(_) | SuccessorGroup::Keyed(..)) => {
                            // Start a new group
                            self.successors.push(SuccessorGroup::Unnamed(vec![field_name]));
                        }
                    }
                }
                Some(OperationFieldType::Successors(SuccessorsType::Default)) => {
                    match self.successors.last() {
                        None => {
                            self.successors.push(SuccessorGroup::Named(field_name.clone()));
                        }
                        Some(SuccessorGroup::Unnamed(_)) if self.successors.len() == 1 => {
                            self.successors.push(SuccessorGroup::Named(field_name.clone()));
                        }
                        Some(
                            SuccessorGroup::Unnamed(_)
                            | SuccessorGroup::Named(_)
                            | SuccessorGroup::Keyed(..),
                        ) => {
                            return Err(Error::custom(
                                "#[successors] may only appear on a single field",
                            )
                            .with_span(&field_name));
                        }
                    }
                    create_params.push(OpCreateParam {
                        param_ty: OpCreateParamType::SuccessorGroupNamed(field_name),
                        r#default: field.attrs.default.is_present(),
                    });
                }
                Some(OperationFieldType::Successors(SuccessorsType::Keyed)) => {
                    match self.successors.last() {
                        None => {
                            self.successors
                                .push(SuccessorGroup::Keyed(field_name.clone(), field_ty.clone()));
                        }
                        Some(SuccessorGroup::Unnamed(_)) if self.successors.len() == 1 => {
                            self.successors
                                .push(SuccessorGroup::Keyed(field_name.clone(), field_ty.clone()));
                        }
                        Some(
                            SuccessorGroup::Unnamed(_)
                            | SuccessorGroup::Named(_)
                            | SuccessorGroup::Keyed(..),
                        ) => {
                            return Err(Error::custom(
                                "#[successors] may only appear on a single field",
                            )
                            .with_span(&field_name));
                        }
                    }
                    create_params.push(OpCreateParam {
                        param_ty: OpCreateParamType::SuccessorGroupKeyed(field_name, field_ty),
                        r#default: field.attrs.default.is_present(),
                    });
                }
                Some(OperationFieldType::Symbol(None)) => {
                    let symbol_path_attr_path: syn::Path = parse_quote_spanned! { field_span =>
                        ::midenc_hir::dialects::builtin::attributes::SymbolRefAttr
                    };
                    let ty = parse_quote_spanned! { field_span =>
                        ::midenc_hir::UnsafeIntrusiveEntityRef<#symbol_path_attr_path>
                    };
                    let symbol = Symbol {
                        name: field_name.clone(),
                        ty: SymbolType::Concrete(field_ty),
                    };
                    create_params.push(OpCreateParam {
                        param_ty: OpCreateParamType::Symbol(symbol.clone()),
                        r#default: field.attrs.default.is_present(),
                    });
                    named_fields.push(syn::Field {
                        attrs: field.attrs.forwarded,
                        vis: field.vis,
                        mutability: syn::FieldMutability::None,
                        ident: Some(field_name),
                        colon_token: Some(syn::token::Colon(field_span)),
                        ty,
                    });
                    self.symbols.push(symbol);
                }
                Some(OperationFieldType::Symbol(Some(symbol_ty))) => {
                    let symbol_path_attr_path: syn::Path = parse_quote_spanned! { field_span =>
                        ::midenc_hir::dialects::builtin::attributes::SymbolRefAttr
                    };
                    let ty = parse_quote_spanned! { field_span =>
                        ::midenc_hir::UnsafeIntrusiveEntityRef<#symbol_path_attr_path>
                    };
                    let symbol = Symbol {
                        name: field_name.clone(),
                        ty: symbol_ty.clone(),
                    };
                    create_params.push(OpCreateParam {
                        param_ty: OpCreateParamType::Symbol(symbol.clone()),
                        r#default: field.attrs.default.is_present(),
                    });
                    named_fields.push(syn::Field {
                        attrs: field.attrs.forwarded,
                        vis: field.vis,
                        mutability: syn::FieldMutability::None,
                        ident: Some(field_name),
                        colon_token: Some(syn::token::Colon(field_span)),
                        ty,
                    });
                    self.symbols.push(symbol);
                }
            }
        }

        self.op_builder_impl.set_create_params(&self.op.generics, create_params);

        Ok(())
    }
}
impl FromDeriveInput for OpDefinition {
    fn from_derive_input(input: &syn::DeriveInput) -> darling::Result<Self> {
        let span = input.span();
        let mut operation = Operation::from_derive_input(input)?;
        Self::from_operation(span, &mut operation)
    }
}

struct OpCreateFn<'a> {
    op: &'a OpDefinition,
    generics: syn::Generics,
}
impl<'a> OpCreateFn<'a> {
    pub fn new(op: &'a OpDefinition) -> Self {
        // Op::create generic parameters
        let mut generics = syn::Generics {
            lt_token: Some(syn::token::Lt(op.span)),
            params: syn::punctuated::Punctuated::from_iter(
                [syn::parse_str("B: ?Sized + ::midenc_hir::Builder").unwrap()]
                    .into_iter()
                    .chain(op.op.generics.params.iter().cloned())
                    .chain(
                        op.op_builder_impl
                            .create_params
                            .iter()
                            .flat_map(|p| p.all_generic_types(true)),
                    ),
            ),
            gt_token: Some(syn::token::Gt(op.span)),
            where_clause: op.op.generics.where_clause.clone(),
        };
        let where_clause = generics.make_where_clause();
        for param in op.op_builder_impl.create_params.iter() {
            param.extend_full_where_clause(where_clause, true);
        }

        Self { op, generics }
    }
}

struct WithAttrs<'a>(&'a OpDefinition);
impl quote::ToTokens for WithAttrs<'_> {
    fn to_tokens(&self, tokens: &mut proc_macro2::TokenStream) {
        for param in self.0.op_builder_impl.create_params.iter() {
            if let OpCreateParamType::Attr(OpAttribute { name, ty, .. }) = &param.param_ty {
                let span = name.span();
                tokens.extend(quote_spanned! { span =>
                    op_builder.with_property::<#ty, _>(#field_name, #name)?;
                });
            }
        }
    }
}

struct WithAttrInfos<'a>(&'a OpDefinition);
impl quote::ToTokens for WithAttrInfos<'_> {
    fn to_tokens(&self, tokens: &mut proc_macro2::TokenStream) {
        let op_ty = &self.0.name;
        for param in self.0.op_builder_impl.create_params.iter() {
            match &param.param_ty {
                OpCreateParamType::Attr(OpAttribute { name, kind, ty, .. }) => {
                    let span = name.span();
                    let field_name =
                        syn::Lit::Str(syn::LitStr::new(&format!("{name}"), name.span()));
                    let hidden =
                        syn::Lit::Bool(syn::LitBool::new(matches!(kind, AttrKind::Hidden), span));
                    tokens.extend(quote_spanned! { span =>
                        unsafe {
                            ::midenc_hir::AttrInfo::new::<#ty>(
                                #field_name.into(),
                                ::core::mem::offset_of!(#op_ty, #name) as u16,
                                #hidden,
                            )
                        },
                    });
                }
                OpCreateParamType::Symbol(Symbol { name, .. }) => {
                    let span = name.span();
                    let field_name =
                        syn::Lit::Str(syn::LitStr::new(&format!("{name}"), name.span()));
                    tokens.extend(quote_spanned! { span =>
                        unsafe {
                            ::midenc_hir::AttrInfo::new::<::midenc_hir::dialects::builtin::attributes::SymbolRefAttr>(
                                #field_name.into(),
                                ::core::mem::offset_of!(#op_ty, #name) as u16,
                                false,
                            )
                        },
                    });
                }
                _ => (),
            }
        }
    }
}

struct WithSymbols<'a>(&'a OpDefinition);
impl quote::ToTokens for WithSymbols<'_> {
    fn to_tokens(&self, tokens: &mut proc_macro2::TokenStream) {
        for param in self.0.op_builder_impl.create_params.iter() {
            if let OpCreateParamType::Symbol(Symbol { name, ty }) = &param.param_ty {
                let span = name.span();
                let field_name = syn::Lit::Str(syn::LitStr::new(&format!("{name}"), name.span()));
                match ty {
                    SymbolType::Any | SymbolType::Concrete(_) | SymbolType::Trait(_) => {
                        tokens.extend(quote_spanned! { span =>
                            op_builder.with_symbol(#field_name, #name);
                        });
                    }
                    SymbolType::Callable => {
                        tokens.extend(quote_spanned! { span =>
                            op_builder.with_callable_symbol(#field_name, #name);
                        });
                    }
                }
            }
        }
    }
}

struct WithOperands<'a>(&'a OpDefinition);
impl quote::ToTokens for WithOperands<'_> {
    fn to_tokens(&self, tokens: &mut proc_macro2::TokenStream) {
        for (group_index, group) in self.0.operands.iter().enumerate() {
            match group {
                OpOperandGroup::Unnamed(operands) => {
                    let group_span = operands[0].name.span();
                    let group_index =
                        syn::Lit::Int(syn::LitInt::new(&format!("{group_index}usize"), group_span));
                    let operand_name = operands.iter().map(|o| &o.name).collect::<Vec<_>>();
                    let operand_constraint = operands.iter().map(|o| &o.constraint);
                    let constraint_violation = operands.iter().map(|o| {
                        syn::Lit::Str(syn::LitStr::new(
                            &format!("type constraint violation for '{}'", &o.name),
                            o.name.span(),
                        ))
                    });
                    tokens.extend(quote_spanned! { group_span =>
                        #(
                            {
                                let value = #operand_name.borrow();
                                let value_ty = value.ty();
                                let constraint = <#operand_constraint as ::midenc_hir::traits::TypeConstraint>::get();
                                if !<#operand_constraint as ::midenc_hir::traits::TypeConstraint>::matches(&constraint, value_ty) {
                                    let expected = <#operand_constraint as ::midenc_hir::traits::TypeConstraint>::description(&constraint);
                                    return Err(builder.context()
                                        .session()
                                        .diagnostics
                                        .diagnostic(::midenc_hir::diagnostics::Severity::Error)
                                        .with_message("invalid operand")
                                        .with_primary_label(span, #constraint_violation)
                                        .with_secondary_label(value.span(), ::alloc::format!("this value has type '{value_ty}', but expected '{expected}'"))
                                        .into_report());
                                }
                            }
                        )*
                        op_builder.with_operands_in_group(#group_index, [#(#operand_name),*]);
                    });
                }
                OpOperandGroup::Named(group_name, group_constraint) => {
                    let group_span = group_name.span();
                    let group_index =
                        syn::Lit::Int(syn::LitInt::new(&format!("{group_index}usize"), group_span));
                    let constraint_violation = syn::Lit::Str(syn::LitStr::new(
                        &format!("type constraint violation for operand in '{group_name}'"),
                        group_span,
                    ));
                    tokens.extend(quote_spanned! { group_span =>
                        let #group_name = #group_name.into_iter().collect::<::alloc::vec::Vec<_>>();
                        for operand in #group_name.iter() {
                            let value = operand.borrow();
                            let value_ty = value.ty();
                            let constraint = <#group_constraint as ::midenc_hir::traits::TypeConstraint>::get();
                            if !<#group_constraint as ::midenc_hir::traits::TypeConstraint>::matches(&constraint, value_ty) {
                                let expected = <#group_constraint as ::midenc_hir::traits::TypeConstraint>::description(&constraint);
                                return Err(builder.context()
                                    .session()
                                    .diagnostics
                                    .diagnostic(::midenc_hir::diagnostics::Severity::Error)
                                    .with_message("invalid operand")
                                    .with_primary_label(span, #constraint_violation)
                                    .with_secondary_label(value.span(), ::alloc::format!("this value has type '{value_ty}', but expected '{expected}'"))
                                    .into_report());
                            }
                        }
                        op_builder.with_operands_in_group(#group_index, #group_name);
                    });
                }
            }
        }
    }
}

struct InitializeCustomFields<'a>(&'a OpDefinition);
impl quote::ToTokens for InitializeCustomFields<'_> {
    fn to_tokens(&self, tokens: &mut proc_macro2::TokenStream) {
        for param in self.0.op_builder_impl.create_params.iter() {
            match &param.param_ty {
                OpCreateParamType::CustomField(id, ..) => {
                    let span = id.span();
                    tokens.extend(quote_spanned! { span =>
                        core::ptr::addr_of_mut!((*__ptr).#id).write(#id);
                    });
                }
                OpCreateParamType::Attr(OpAttribute { name, ty, .. }) => {
                    let span = name.span();
                    tokens.extend(quote_spanned! { span =>
                        core::ptr::addr_of_mut!((*__ptr).#name).write(
                            ::midenc_hir::UnsafeIntrusiveEntityRef::<#ty>::dangling()
                        );
                    });
                }
                OpCreateParamType::Symbol(sym) => {
                    let span = sym.name.span();
                    let id = &sym.name;
                    tokens.extend(quote_spanned! { span =>
                        core::ptr::addr_of_mut!((*__ptr).#id).write(
                            ::midenc_hir::UnsafeIntrusiveEntityRef::<::midenc_hir::dialects::builtin::attributes::SymbolRefAttr>::dangling()
                        );
                    });
                }
                _ => continue,
            }
        }
    }
}

struct DefaultInitializeCustomFields<'a>(&'a OpDefinition);
impl quote::ToTokens for DefaultInitializeCustomFields<'_> {
    fn to_tokens(&self, tokens: &mut proc_macro2::TokenStream) {
        for param in self.0.op_builder_impl.create_params.iter() {
            match &param.param_ty {
                OpCreateParamType::CustomField(id, ..) => {
                    let span = id.span();
                    tokens.extend(quote_spanned! { span =>
                        core::ptr::addr_of_mut!((*__ptr).#id).write(Default::default());
                    });
                }
                OpCreateParamType::Attr(attr) => {
                    let span = attr.name.span();
                    let id = &attr.name;
                    let attr_type = &attr.ty;
                    tokens.extend(quote_spanned! { span =>
                        {
                            core::ptr::addr_of_mut!((*__ptr).#id).write(
                                ::midenc_hir::UnsafeIntrusiveEntityRef::<#attr_type>::dangling()
                            );
                        }
                    });
                }
                OpCreateParamType::Symbol(sym) => {
                    let span = sym.name.span();
                    let id = &sym.name;
                    tokens.extend(quote_spanned! { span =>
                        core::ptr::addr_of_mut!((*__ptr).#id).write(
                            ::midenc_hir::UnsafeIntrusiveEntityRef::<::midenc_hir::dialects::builtin::attributes::SymbolRefAttr>::dangling()
                        );
                    });
                }
                _ => continue,
            }
        }
    }
}

struct WithResults<'a>(&'a OpDefinition);
impl quote::ToTokens for WithResults<'_> {
    fn to_tokens(&self, tokens: &mut proc_macro2::TokenStream) {
        match self.0.results.as_ref() {
            None => (),
            Some(OpResultGroup::Unnamed(results)) => {
                let group_span = results[0].name.span();
                let num_results =
                    syn::Lit::Int(syn::LitInt::new(&format!("{}usize", results.len()), group_span));
                tokens.extend(quote_spanned! { group_span =>
                    op_builder.with_results(::alloc::vec![::midenc_hir::Type::Unknown; #num_results]);
                });
            }
            // Named result groups can have an arbitrary number of results
            Some(OpResultGroup::Named(..)) => (),
        }
    }
}

struct WithSuccessors<'a>(&'a OpDefinition);
impl quote::ToTokens for WithSuccessors<'_> {
    fn to_tokens(&self, tokens: &mut proc_macro2::TokenStream) {
        for group in self.0.successors.iter() {
            match group {
                SuccessorGroup::Unnamed(successors) => {
                    let successor_args = successors.iter().map(|s| format_ident!("{s}_args"));
                    tokens.extend(quote! {
                        op_builder.with_successors([
                            #((
                                #successors,
                                #successor_args.into_iter().collect::<::alloc::vec::Vec<_>>(),
                            ),)*
                        ]);
                    });
                }
                SuccessorGroup::Named(name) => {
                    let span = name.span();
                    tokens.extend(quote_spanned! { span =>
                        op_builder.with_successors(#name);
                    });
                }
                SuccessorGroup::Keyed(name, _) => {
                    let span = name.span();
                    tokens.extend(quote_spanned! { span =>
                        op_builder.with_keyed_successors(#name);
                    });
                }
            }
        }
    }
}

struct BuildOp<'a>(&'a OpDefinition);
impl quote::ToTokens for BuildOp<'_> {
    fn to_tokens(&self, tokens: &mut proc_macro2::TokenStream) {
        match self.0.results.as_ref() {
            None => {
                tokens.extend(quote! {
                    op_builder.build()
                });
            }
            Some(group) => {
                let verify_result_constraints = match group {
                    OpResultGroup::Unnamed(results) => {
                        let verify_result = results.iter().map(|result| {
                            let result_name = &result.name;
                            let result_span = result_name.span();
                            let result_constraint = &result.constraint;
                            let constraint_violation = syn::Lit::Str(syn::LitStr::new(
                                &format!("type constraint violation for result '{result_name}'"),
                                result_name.span(),
                            ));
                            quote_spanned! { result_span =>
                                {
                                    let op_result = op.#result_name();
                                    let value_ty = op_result.ty();
                                    let constraint = <#result_constraint as ::midenc_hir::traits::TypeConstraint>::get();
                                    if !<#result_constraint as ::midenc_hir::traits::TypeConstraint>::matches(&constraint, value_ty) {
                                        let expected = <#result_constraint as ::midenc_hir::traits::TypeConstraint>::description(&constraint);
                                        return Err(builder.context()
                                            .session()
                                            .diagnostics
                                            .diagnostic(::midenc_hir::diagnostics::Severity::Error)
                                            .with_message(::alloc::format!("invalid operation {}", op.name()))
                                            .with_primary_label(span, #constraint_violation)
                                            .with_secondary_label(op_result.span(), ::alloc::format!("this value has type '{value_ty}', but expected '{expected}'"))
                                            .into_report());
                                    }
                                }
                            }
                        });
                        quote! {
                            #(
                                #verify_result
                            )*
                        }
                    }
                    OpResultGroup::Named(name, constraint) => {
                        let span = name.span();
                        let constraint_violation = syn::Lit::Str(syn::LitStr::new(
                            &format!("type constraint violation for result in '{name}'"),
                            span,
                        ));
                        quote_spanned! { span =>
                            {
                                let results = op.#name();
                                for result in results.iter() {
                                    let value = result.borrow();
                                    let value_ty = value.ty();
                                    let constraint = <#constraint as ::midenc_hir::traits::TypeConstraint>::get();
                                    if !<#constraint as ::midenc_hir::traits::TypeConstraint>::matches(&constraint, value_ty) {
                                        let expected = <#constraint as ::midenc_hir::traits::TypeConstraint>::description(&constraint);
                                        return Err(builder.context()
                                            .session()
                                            .diagnostics
                                            .diagnostic(::midenc_hir::diagnostics::Severity::Error)
                                            .with_message("invalid operation")
                                            .with_primary_label(span, #constraint_violation)
                                            .with_secondary_label(value.span(), ::alloc::format!("this value has type '{value_ty}', but expected '{expected}'"))
                                            .into_report());
                                    }
                                }
                            }
                        }
                    }
                };

                tokens.extend(quote! {
                    let op = op_builder.build()?;

                    {
                        let op = op.borrow();
                        #verify_result_constraints
                    }

                    Ok(op)
                })
            }
        }
    }
}

impl quote::ToTokens for OpCreateFn<'_> {
    fn to_tokens(&self, tokens: &mut proc_macro2::TokenStream) {
        let dialect = &self.op.dialect;
        let (impl_generics_all, _ty_generics_all, where_clause_all) =
            self.generics.split_for_impl();
        let all_param_names = self
            .op
            .op_builder_impl
            .create_params
            .iter()
            .flat_map(OpCreateParam::all_bindings);
        let all_param_types = self
            .op
            .op_builder_impl
            .create_params
            .iter()
            .flat_map(|p| p.all_binding_types(true));
        let initialize_custom_fields = InitializeCustomFields(self.op);
        let default_initialize_custom_fields = DefaultInitializeCustomFields(self.op);
        let with_symbols = WithSymbols(self.op);
        let with_attrs = WithAttrs(self.op);
        let with_operands = WithOperands(self.op);
        let with_results = WithResults(self.op);
        let with_regions = self.op.regions.iter().map(|r| {
            let region_span = r.span();
            quote_spanned! { region_span =>
                op_builder.create_region();
            }
        });
        let with_successors = WithSuccessors(self.op);
        let build_op = BuildOp(self.op);

        let create_doc = syn::Lit::Str(syn::LitStr::new(
            &format!("Manually construct a new `{}`", &self.op.name),
            self.op.span,
        ));
        let alloc_default_doc = syn::Lit::Str(syn::LitStr::new(
            &format!("Allocate a new, default-initialized `{}`", &self.op.name),
            self.op.span,
        ));

        let op_span = self.op.span;
        tokens.extend(quote_spanned! { op_span =>
            #[doc = #create_doc]
            ///
            /// It is generally preferable to use [`::midenc_hir::BuilderExt::create`] instead.
            #[allow(clippy::too_many_arguments)]
            pub fn create #impl_generics_all(
                builder: &mut B,
                span: ::midenc_hir::diagnostics::SourceSpan,
                #(
                    #all_param_names: #all_param_types,
                )*
            ) -> Result<::midenc_hir::UnsafeIntrusiveEntityRef<Self>, ::midenc_hir::diagnostics::Report>
            #where_clause_all
            {
                #![allow(clippy::all)]
                use ::midenc_hir::{Builder, Op};
                let mut __this = {
                    let __operation_name = {
                        let context = builder.context();
                        let dialect = context.get_or_register_dialect::<#dialect>();
                        dialect.expect_registered_name::<Self>()
                    };
                    let __context = builder.context_rc();
                    let mut __op = __context.alloc_uninit_tracked::<Self>();
                    unsafe {
                        let __assumed_init = ::midenc_hir::RawEntityRef::assume_init(__op);
                        {
                            let mut __uninit = __op.borrow_mut();
                            let __ptr = (*__uninit).as_mut_ptr();
                            let __offset = core::mem::offset_of!(Self, op);
                            let __op_ptr = core::ptr::addr_of_mut!((*__ptr).op);
                            __op_ptr.write(::midenc_hir::Operation::uninit::<Self>(__context, __operation_name, __offset));
                            #initialize_custom_fields
                        }
                        let mut __assumed_init = __assumed_init;
                        __assumed_init.borrow_mut().set_span(span);
                        __assumed_init
                    }
                };

                let mut op_builder = ::midenc_hir::OperationBuilder::new(builder, __this);
                #with_attrs
                #with_symbols
                #with_operands
                #(
                    #with_regions
                )*
                #with_successors
                #with_results

                // Finalize construction of this op
                #build_op
            }

            #[doc = #alloc_default_doc]
            ///
            /// This method is used as part of op definition implementation, and should not be
            /// used directly if at all possible.
            pub fn alloc_default(context: ::alloc::rc::Rc<::midenc_hir::Context>) -> ::midenc_hir::UnsafeIntrusiveEntityRef<Self> {
                #![allow(clippy::all)]
                let __operation_name = {
                    let dialect = context.get_or_register_dialect::<#dialect>();
                    dialect.expect_registered_name::<Self>()
                };
                let mut __op = context.alloc_uninit_tracked::<Self>();
                unsafe {
                    let __assumed_init = ::midenc_hir::RawEntityRef::assume_init(__op);
                    {
                        let mut __uninit = __op.borrow_mut();
                        let __ptr = (*__uninit).as_mut_ptr();
                        let __offset = core::mem::offset_of!(Self, op);
                        let __op_ptr = core::ptr::addr_of_mut!((*__ptr).op);
                        __op_ptr.write(::midenc_hir::Operation::uninit::<Self>(context.clone(), __operation_name, __offset));
                        #default_initialize_custom_fields
                    }
                    __assumed_init
                }
            }
        });
    }
}

impl quote::ToTokens for OpDefinition {
    fn to_tokens(&self, tokens: &mut proc_macro2::TokenStream) {
        let op_ident = &self.name;
        let op_span = op_ident.span();
        let (impl_generics, ty_generics, where_clause) = self.op.generics.split_for_impl();

        // struct $Op
        #[allow(clippy::type_complexity)]
        self.op.to_tokens(tokens);

        // impl Spanned
        tokens.extend(quote_spanned! { op_span =>
            impl #impl_generics ::midenc_hir::diagnostics::Spanned for #op_ident #ty_generics #where_clause {
                fn span(&self) -> ::midenc_hir::diagnostics::SourceSpan {
                    ::midenc_hir::diagnostics::Spanned::span(&self.op)
                }
            }
        });

        // impl AsRef<Operation>/AsMut<Operation>
        tokens.extend(quote_spanned! { op_span =>
            impl #impl_generics AsRef<::midenc_hir::Operation> for #op_ident #ty_generics #where_clause {
                #[inline(always)]
                fn as_ref(&self) -> &::midenc_hir::Operation {
                    &self.op
                }
            }

            impl #impl_generics AsMut<::midenc_hir::Operation> for #op_ident #ty_generics #where_clause {
                #[inline(always)]
                fn as_mut(&mut self) -> &mut ::midenc_hir::Operation {
                    &mut self.op
                }
            }
        });

        // impl Op
        // impl OpRegistration
        let dialect = &self.dialect;
        let opcode = &self.opcode;
        let opcode_str = syn::Lit::Str(syn::LitStr::new(&opcode.to_string(), opcode.span()));
        let traits = &self.traits;
        let implements = &self.implements;
        let attrs = WithAttrInfos(self);
        tokens.extend(quote_spanned! { op_span =>
            impl #impl_generics ::midenc_hir::Op for #op_ident #ty_generics #where_clause {
                #[inline]
                fn name(&self) -> ::midenc_hir::OperationName {
                    self.op.name()
                }

                #[inline(always)]
                fn as_operation(&self) -> &::midenc_hir::Operation {
                    &self.op
                }

                #[inline(always)]
                fn as_operation_mut(&mut self) -> &mut ::midenc_hir::Operation {
                    &mut self.op
                }
            }

            impl #impl_generics ::midenc_hir::OpRegistration for #op_ident #ty_generics #where_clause {
                type Dialect = #dialect;

                fn name() -> ::midenc_hir::interner::Symbol {
                    ::midenc_hir::interner::Symbol::intern(#opcode_str)
                }

                fn attrs() -> ::alloc::boxed::Box<[::midenc_hir::AttrInfo]> {
                    ::alloc::boxed::Box::from([
                        #attrs
                    ])
                }

                fn traits() -> ::alloc::boxed::Box<[::midenc_hir::traits::TraitInfo]> {
                    ::alloc::boxed::Box::from([
                        ::midenc_hir::traits::TraitInfo::new::<Self, dyn core::any::Any>(),
                        ::midenc_hir::traits::TraitInfo::new::<Self, dyn ::midenc_hir::Op>(),
                        #(
                            ::midenc_hir::traits::TraitInfo::new::<Self, dyn #traits>(),
                        )*
                        #(
                            ::midenc_hir::traits::TraitInfo::new::<Self, dyn #implements>(),
                        )*
                    ])
                }

                fn alloc_uninit(context: ::alloc::rc::Rc<::midenc_hir::Context>) -> ::midenc_hir::OperationRef {
                    Self::alloc_default(context).as_operation_ref()
                }
            }

            ::midenc_hir::inventory::submit!(::midenc_hir::DialectOpRegistrationInfo::new::<#op_ident #ty_generics>());
        });

        // impl $OpBuilder
        // impl BuildableOp
        self.op_builder_impl.to_tokens(tokens);

        // impl $Op
        {
            let create_fn = OpCreateFn::new(self);
            let custom_field_fns = OpCustomFieldFns(self);
            let attr_fns = OpAttrFns(self);
            let symbol_fns = OpSymbolFns(self);
            let operand_fns = OpOperandFns(self);
            let result_fns = OpResultFns(self);
            let region_fns = OpRegionFns(self);
            let successor_fns = OpSuccessorFns(self);
            tokens.extend(quote_spanned! { op_span =>
                /// Construction
                #[allow(unused)]
                impl #impl_generics #op_ident #ty_generics #where_clause {
                    #create_fn
                }

                /// User-defined Fields
                #[allow(unused)]
                impl #impl_generics #op_ident #ty_generics #where_clause {
                    #custom_field_fns
                }

                /// Attributes
                #[allow(unused)]
                impl #impl_generics #op_ident #ty_generics #where_clause {
                    #attr_fns
                }

                /// Symbols
                #[allow(unused)]
                impl #impl_generics #op_ident #ty_generics #where_clause {
                    #symbol_fns
                }

                /// Operands
                #[allow(unused)]
                impl #impl_generics #op_ident #ty_generics #where_clause {
                    #operand_fns
                }

                /// Results
                #[allow(unused)]
                impl #impl_generics #op_ident #ty_generics #where_clause {
                    #result_fns
                }

                /// Regions
                #[allow(unused)]
                impl #impl_generics #op_ident #ty_generics #where_clause {
                    #region_fns
                }

                /// Successors
                #[allow(unused)]
                impl #impl_generics #op_ident #ty_generics #where_clause {
                    #successor_fns
                }
            });
        }

        // impl $DerivedTrait
        for derived_trait in self.traits.iter() {
            let span = derived_trait.span();
            tokens.extend(quote_spanned! { span =>
                impl #impl_generics #derived_trait for #op_ident #ty_generics #where_clause {}
            });
        }

        // impl OpVerifier
        self.op_verifier_impl.to_tokens(tokens);
    }
}

struct OpCustomFieldFns<'a>(&'a OpDefinition);
impl quote::ToTokens for OpCustomFieldFns<'_> {
    fn to_tokens(&self, tokens: &mut proc_macro2::TokenStream) {
        // User-defined fields
        for field in self.0.op.fields.iter() {
            let field_name = field.ident.as_ref().unwrap();
            let field_span = field_name.span();
            // Do not generate field functions for custom fields with private visibility
            if matches!(field.vis, syn::Visibility::Inherited) {
                continue;
            }
            let field_name_mut = format_ident!("{field_name}_mut");
            let set_field_name = format_ident!("set_{field_name}");
            let field_doc = syn::Lit::Str(syn::LitStr::new(
                &format!(" Get a reference to the value of `{field_name}`"),
                field_name.span(),
            ));
            let field_mut_doc = syn::Lit::Str(syn::LitStr::new(
                &format!(" Get a mutable reference to the value of `{field_name}`"),
                field_name.span(),
            ));
            let set_field_doc = syn::Lit::Str(syn::LitStr::new(
                &format!(" Set the value of `{field_name}`"),
                field_name.span(),
            ));
            let field_ty = &field.ty;
            tokens.extend(quote_spanned! { field_span =>
                #[doc = #field_doc]
                #[inline]
                pub fn #field_name(&self) -> &#field_ty {
                    &self.#field_name
                }

                #[doc = #field_mut_doc]
                #[inline]
                pub fn #field_name_mut(&mut self) -> &mut #field_ty {
                    &mut self.#field_name
                }

                #[doc = #set_field_doc]
                #[inline]
                pub fn #set_field_name(&mut self, #field_name: #field_ty) {
                    self.#field_name = #field_name;
                }
            });
        }
    }
}

struct OpSymbolFns<'a>(&'a OpDefinition);
impl quote::ToTokens for OpSymbolFns<'_> {
    fn to_tokens(&self, tokens: &mut proc_macro2::TokenStream) {
        // Symbols
        for Symbol {
            name: symbol,
            ty: symbol_kind,
        } in self.0.symbols.iter()
        {
            let span = symbol.span();
            let symbol_mut = format_ident!("{symbol}_mut");
            let get_symbol = format_ident!("get_{symbol}");
            let set_symbol = format_ident!("set_{symbol}");
            let symbol_name = syn::Lit::Str(syn::LitStr::new(&format!("{symbol}"), span));
            let symbol_doc = syn::Lit::Str(syn::LitStr::new(
                &format!(" Get a reference to the `{symbol}` attribute."),
                span,
            ));
            let symbol_mut_doc = syn::Lit::Str(syn::LitStr::new(
                &format!(" Get a mutable reference to the `{symbol}` attribute."),
                span,
            ));
            let get_symbol_doc = syn::Lit::Str(syn::LitStr::new(
                &format!(" Get a reference to the value of the `{symbol}` attribute."),
                span,
            ));
            let set_symbol_doc_lines = [
                syn::Lit::Str(syn::LitStr::new(
                    &format!(" Set the value of the `{symbol}` symbol."),
                    span,
                )),
                syn::Lit::Str(syn::LitStr::new("", span)),
                syn::Lit::Str(syn::LitStr::new(
                    " Returns `Err` if the symbol cannot be resolved in the nearest symbol table.",
                    span,
                )),
            ];

            tokens.extend(quote_spanned! { span =>
                #[doc = #symbol_doc]
                pub fn #symbol(&self) -> ::midenc_hir::EntityRef<'_, ::midenc_hir::dialects::builtin::attributes::SymbolRefAttr> {
                    self.#symbol.borrow()
                }

                #[doc = #symbol_mut_doc]
                pub fn #symbol_mut(&mut self) -> ::midenc_hir::EntityMut<'_, ::midenc_hir::dialects::builtin::attributes::SymbolRefAttr> {
                    self.#symbol.borrow_mut()
                }
            });

            let is_concrete_ty = match symbol_kind {
                SymbolType::Concrete(ty) => [quote_spanned! { span =>
                    // The way we check the type depends on whether `symbol` is a reference to `self`
                    let (data_ptr, _) = ::midenc_hir::SymbolRef::as_ptr(&symbol).to_raw_parts();
                    if core::ptr::addr_eq(data_ptr, (self as *const Self as *const ())) {
                        if !self.op.is::<#ty>() {
                            return Err(::midenc_hir::InvalidSymbolRefError::InvalidType {
                                symbol: self.op.span(),
                                expected: stringify!(#ty),
                                got: self.op.name(),
                            });
                        }
                    } else if !symbol.borrow().is::<#ty>() {
                        let symbol = symbol.borrow();
                        let symbol_op = symbol.as_symbol_operation();
                        return Err(::midenc_hir::InvalidSymbolRefError::InvalidType {
                            symbol: symbol_op.span(),
                            expected: stringify!(#ty),
                            got: symbol_op.name(),
                        });
                    }
                }],
                _ => [quote! {}],
            };

            match symbol_kind {
                SymbolType::Any | SymbolType::Trait(_) | SymbolType::Concrete(_) => {
                    tokens.extend(quote_spanned! { span =>
                        #[doc = #get_symbol_doc]
                        pub fn #get_symbol(&self) -> ::midenc_hir::EntityRef<'_, ::midenc_hir::dialects::builtin::attributes::SymbolRef> {
                            ::midenc_hir::EntityRef::map(self.#symbol.borrow(), ::midenc_hir::AttributeRegistration::underlying_value)
                        }

                        #(
                            #[doc = #set_symbol_doc_lines]
                        )*
                        pub fn #set_symbol(&mut self, symbol: impl ::midenc_hir::AsSymbolRef) -> Result<(), ::midenc_hir::InvalidSymbolRefError> {
                            let symbol = symbol.as_symbol_ref();
                            #(#is_concrete_ty)*
                            self.op.set_symbol_attribute(#symbol_name, symbol);

                            Ok(())
                        }
                    });
                }
                SymbolType::Callable => {
                    tokens.extend(quote_spanned! { span =>
                        #(
                            #[doc = #set_symbol_doc_lines]
                        )*
                        pub fn #set_symbol(&mut self, symbol: impl ::midenc_hir::AsCallableSymbolRef) -> Result<(), ::midenc_hir::InvalidSymbolRefError> {
                            use ::midenc_hir::Spanned;
                            let symbol = symbol.as_callable_symbol_ref();
                            let (data_ptr, _) = ::midenc_hir::SymbolRef::as_ptr(&symbol).to_raw_parts();
                            if core::ptr::addr_eq(data_ptr, (self as *const Self as *const ())) {
                                if !self.op.implements::<dyn ::midenc_hir::CallableOpInterface>() {
                                    return Err(::midenc_hir::InvalidSymbolRefError::NotCallable {
                                        symbol: self.span(),
                                    });
                                }
                            } else {
                                let symbol = symbol.borrow();
                                let symbol_op = symbol.as_symbol_operation();
                                if !symbol_op.implements::<dyn ::midenc_hir::CallableOpInterface>() {
                                    return Err(::midenc_hir::InvalidSymbolRefError::NotCallable {
                                        symbol: symbol_op.span(),
                                    });
                                }
                            }
                            self.op.set_symbol_attribute(#symbol_name, symbol);

                            Ok(())
                        }
                    });
                }
            }
        }
    }
}

struct OpAttrFns<'a>(&'a OpDefinition);
impl quote::ToTokens for OpAttrFns<'_> {
    fn to_tokens(&self, tokens: &mut proc_macro2::TokenStream) {
        // Attributes
        for OpAttribute {
            name: attr,
            ty: attr_ty,
            value_ty: attr_value_ty,
            ..
        } in self.0.attrs.iter()
        {
            let span = attr.span();
            let attr_mut = format_ident!("{attr}_mut");
            let get_attr = format_ident!("get_{attr}");
            let get_attr_mut = format_ident!("get_{attr}_mut");
            let set_attr = format_ident!("set_{attr}");
            let attr_doc = syn::Lit::Str(syn::LitStr::new(
                &format!(" Get a reference to the `{attr}` attribute."),
                attr.span(),
            ));
            let attr_mut_doc = syn::Lit::Str(syn::LitStr::new(
                &format!(" Get a mutable reference to the `{attr}` attribute."),
                attr.span(),
            ));
            let get_attr_doc = syn::Lit::Str(syn::LitStr::new(
                &format!(" Get a reference to the value of the `{attr}` attribute."),
                attr.span(),
            ));
            let get_attr_mut_doc = syn::Lit::Str(syn::LitStr::new(
                &format!(" Get a mutable reference to the value of the `{attr}` attribute."),
                attr.span(),
            ));
            let set_attr_doc = syn::Lit::Str(syn::LitStr::new(
                &format!(" Set the value of the `{attr}` attribute."),
                attr.span(),
            ));
            tokens.extend(quote_spanned! { span =>
                #[doc = #attr_doc]
                #[inline]
                pub fn #attr(&self) -> ::midenc_hir::EntityRef<'_, #attr_ty> {
                    self.#attr.borrow()
                }

                #[doc = #get_attr_doc]
                #[inline]
                pub fn #get_attr(&self) -> ::midenc_hir::EntityRef<'_, #attr_value_ty> {
                    ::midenc_hir::EntityRef::map(self.#attr.borrow(), ::midenc_hir::AttributeRegistration::underlying_value)
                }

                #[doc = #attr_mut_doc]
                #[inline]
                pub fn #attr_mut(&mut self) -> ::midenc_hir::EntityMut<'_, #attr_ty> {
                    self.#attr.borrow_mut()
                }

                #[doc = #get_attr_mut_doc]
                #[inline]
                pub fn #get_attr_mut(&mut self) -> ::midenc_hir::EntityMut<'_, #attr_value_ty> {
                    ::midenc_hir::EntityMut::map(self.#attr.borrow_mut(), ::midenc_hir::AttributeRegistration::underlying_value_mut)
                }

                #[doc = #set_attr_doc]
                #[inline]
                pub fn #set_attr<AttrValue>(&mut self, value: AttrValue)
                where
                    #attr_value_ty: From<AttrValue>,
                {
                    let attr = self.op.context_rc().create_attribute::<#attr_ty, AttrValue>(value);
                    self.#attr = attr;
                }
            });
        }
    }
}

struct OpOperandFns<'a>(&'a OpDefinition);
impl quote::ToTokens for OpOperandFns<'_> {
    fn to_tokens(&self, tokens: &mut proc_macro2::TokenStream) {
        for (group_index, operand_group) in self.0.operands.iter().enumerate() {
            let group_index = syn::Lit::Int(syn::LitInt::new(
                &format!("{group_index}usize"),
                proc_macro2::Span::call_site(),
            ));
            match operand_group {
                // Operands
                OpOperandGroup::Unnamed(operands) => {
                    for (operand_index, Operand { name: operand, .. }) in
                        operands.iter().enumerate()
                    {
                        let span = operand.span();
                        let operand_index = syn::Lit::Int(syn::LitInt::new(
                            &format!("{operand_index}usize"),
                            proc_macro2::Span::call_site(),
                        ));
                        let operand_mut = format_ident!("{operand}_mut");
                        let operand_doc = syn::Lit::Str(syn::LitStr::new(
                            &format!(" Get a reference to the `{operand}` operand."),
                            operand.span(),
                        ));
                        let operand_mut_doc = syn::Lit::Str(syn::LitStr::new(
                            &format!(" Get a mutable reference to the `{operand}` operand."),
                            operand.span(),
                        ));
                        tokens.extend(quote_spanned! { span =>
                            #[doc = #operand_doc]
                            #[inline]
                            pub fn #operand(&self) -> ::midenc_hir::EntityRef<'_, ::midenc_hir::OpOperandImpl> {
                                self.op.operands().group(#group_index)[#operand_index].borrow()
                            }

                            #[doc = #operand_mut_doc]
                            #[inline]
                            pub fn #operand_mut(&mut self) -> ::midenc_hir::EntityMut<'_, ::midenc_hir::OpOperandImpl> {
                                self.op.operands_mut().group_mut(#group_index)[#operand_index].borrow_mut()
                            }
                        });
                    }
                }
                // User-defined operand groups
                OpOperandGroup::Named(group_name, _) => {
                    let span = group_name.span();
                    let group_name_mut = format_ident!("{group_name}_mut");
                    let group_doc = syn::Lit::Str(syn::LitStr::new(
                        &format!(" Get a reference to the `{group_name}` operand group."),
                        group_name.span(),
                    ));
                    let group_mut_doc = syn::Lit::Str(syn::LitStr::new(
                        &format!(" Get a mutable reference to the `{group_name}` operand group."),
                        group_name.span(),
                    ));
                    tokens.extend(quote_spanned! { span =>
                        #[doc = #group_doc]
                        #[inline]
                        pub fn #group_name(&self) -> ::midenc_hir::OpOperandRange<'_> {
                            self.op.operands().group(#group_index)
                        }

                        #[doc = #group_mut_doc]
                        #[inline]
                        pub fn #group_name_mut(&mut self) -> ::midenc_hir::OpOperandRangeMut<'_> {
                            self.op.operands_mut().group_mut(#group_index)
                        }
                    });
                }
            }
        }
    }
}

struct OpResultFns<'a>(&'a OpDefinition);
impl quote::ToTokens for OpResultFns<'_> {
    fn to_tokens(&self, tokens: &mut proc_macro2::TokenStream) {
        if let Some(group) = self.0.results.as_ref() {
            match group {
                OpResultGroup::Unnamed(results) => {
                    for (index, OpResult { name: result, .. }) in results.iter().enumerate() {
                        let span = result.span();
                        let index = syn::Lit::Int(syn::LitInt::new(
                            &format!("{index}usize"),
                            result.span(),
                        ));
                        let result_mut = format_ident!("{result}_mut");
                        let result_doc = syn::Lit::Str(syn::LitStr::new(
                            &format!(" Get a reference to the `{result}` result."),
                            result.span(),
                        ));
                        let result_mut_doc = syn::Lit::Str(syn::LitStr::new(
                            &format!(" Get a mutable reference to the `{result}` result."),
                            result.span(),
                        ));
                        tokens.extend(quote_spanned! { span =>
                            #[doc = #result_doc]
                            #[inline]
                            pub fn #result(&self) -> ::midenc_hir::EntityRef<'_, ::midenc_hir::OpResult> {
                                self.op.results()[#index].borrow()
                            }

                            #[doc = #result_mut_doc]
                            #[inline]
                            pub fn #result_mut(&mut self) -> ::midenc_hir::EntityMut<'_, ::midenc_hir::OpResult> {
                                self.op.results_mut()[#index].borrow_mut()
                            }
                        });
                    }
                }
                OpResultGroup::Named(group, _) => {
                    let span = group.span();
                    let group_mut = format_ident!("{group}_mut");
                    let group_doc = syn::Lit::Str(syn::LitStr::new(
                        &format!(" Get a reference to the `{group}` result group."),
                        group.span(),
                    ));
                    let group_mut_doc = syn::Lit::Str(syn::LitStr::new(
                        &format!(" Get a mutable reference to the `{group}` result group."),
                        group.span(),
                    ));
                    tokens.extend(quote_spanned! { span =>
                        #[doc = #group_doc]
                        #[inline]
                        pub fn #group(&self) -> ::midenc_hir::OpResultRange<'_> {
                            self.results().group(0)
                        }

                        #[doc = #group_mut_doc]
                        #[inline]
                        pub fn #group_mut(&mut self) -> ::midenc_hir::OpResultRangeMut<'_> {
                            self.op.results_mut().group_mut(0)
                        }
                    });
                }
            }
        }
    }
}

struct OpRegionFns<'a>(&'a OpDefinition);
impl quote::ToTokens for OpRegionFns<'_> {
    fn to_tokens(&self, tokens: &mut proc_macro2::TokenStream) {
        // Regions
        for (index, region) in self.0.regions.iter().enumerate() {
            let span = region.span();
            let index = syn::Lit::Int(syn::LitInt::new(&format!("{index}usize"), region.span()));
            let region_mut = format_ident!("{region}_mut");
            let region_doc = syn::Lit::Str(syn::LitStr::new(
                &format!(" Get a reference to the `{region}` region."),
                region.span(),
            ));
            let region_mut_doc = syn::Lit::Str(syn::LitStr::new(
                &format!(" Get a mutable reference to the `{region}` region."),
                region.span(),
            ));
            tokens.extend(quote_spanned! { span =>
                #[doc = #region_doc]
                #[inline]
                pub fn #region(&self) -> ::midenc_hir::EntityRef<'_, ::midenc_hir::Region> {
                    self.op.region(#index)
                }

                #[doc = #region_mut_doc]
                #[inline]
                pub fn #region_mut(&mut self) -> ::midenc_hir::EntityMut<'_, ::midenc_hir::Region> {
                    self.op.region_mut(#index)
                }
            });
        }
    }
}

struct OpSuccessorFns<'a>(&'a OpDefinition);
impl quote::ToTokens for OpSuccessorFns<'_> {
    fn to_tokens(&self, tokens: &mut proc_macro2::TokenStream) {
        for (group_index, group) in self.0.successors.iter().enumerate() {
            let group_index = syn::Lit::Int(syn::LitInt::new(
                &format!("{group_index}usize"),
                proc_macro2::Span::call_site(),
            ));
            match group {
                // Successors
                SuccessorGroup::Unnamed(successors) => {
                    for (index, successor) in successors.iter().enumerate() {
                        let span = successor.span();
                        let index = syn::Lit::Int(syn::LitInt::new(
                            &format!("{index}usize"),
                            proc_macro2::Span::call_site(),
                        ));
                        let successor_mut = format_ident!("{successor}_mut");
                        let successor_doc = syn::Lit::Str(syn::LitStr::new(
                            &format!(" Get a reference to the `{successor}` successor."),
                            successor.span(),
                        ));
                        let successor_mut_doc = syn::Lit::Str(syn::LitStr::new(
                            &format!(" Get a mutable reference to the `{successor}` successor."),
                            successor.span(),
                        ));
                        tokens.extend(quote_spanned! { span =>
                            #[doc = #successor_doc]
                            #[inline]
                            pub fn #successor(&self) -> ::midenc_hir::OpSuccessor<'_> {
                                self.op.successor_in_group(#group_index, #index)
                            }

                            #[doc = #successor_mut_doc]
                            #[inline]
                            pub fn #successor_mut(&mut self) -> ::midenc_hir::OpSuccessorMut<'_> {
                                self.op.successor_in_group_mut(#group_index, #index)
                            }
                        });
                    }
                }
                // Variadic successor groups
                SuccessorGroup::Named(group) => {
                    let span = group.span();
                    let group_mut = format_ident!("{group}_mut");
                    let group_doc = syn::Lit::Str(syn::LitStr::new(
                        &format!(" Get a reference to the `{group}` successor group."),
                        group.span(),
                    ));
                    let group_mut_doc = syn::Lit::Str(syn::LitStr::new(
                        &format!(" Get a mutable reference to the `{group}` successor group."),
                        group.span(),
                    ));
                    tokens.extend(quote_spanned! { span =>
                        #[doc = #group_doc]
                        #[inline]
                        pub fn #group(&self) -> ::midenc_hir::OpSuccessorRange<'_> {
                            self.op.successor_group(#group_index)
                        }

                        #[doc = #group_mut_doc]
                        #[inline]
                        pub fn #group_mut(&mut self) -> ::midenc_hir::OpSuccessorRangeMut<'_> {
                            self.op.successor_group(#group_index)
                        }
                    });
                }
                // User-defined successor groups
                SuccessorGroup::Keyed(group, group_ty) => {
                    let span = group.span();
                    let group_mut = format_ident!("{group}_mut");
                    let group_doc = syn::Lit::Str(syn::LitStr::new(
                        &format!(" Get a reference to the `{group}` successor group."),
                        group.span(),
                    ));
                    let group_mut_doc = syn::Lit::Str(syn::LitStr::new(
                        &format!(" Get a mutable reference to the `{group}` successor group."),
                        group.span(),
                    ));
                    tokens.extend(quote_spanned! { span =>
                        #[doc = #group_doc]
                        #[inline]
                        pub fn #group(&self) -> ::midenc_hir::KeyedSuccessorRange<'_, #group_ty> {
                            self.op.keyed_successor_group::<#group_ty>(#group_index)
                        }

                        #[doc = #group_mut_doc]
                        #[inline]
                        pub fn #group_mut(&mut self) -> ::midenc_hir::KeyedSuccessorRangeMut<'_, #group_ty> {
                            self.op.keyed_successor_group_mut::<#group_ty>(#group_index)
                        }
                    });
                }
            }
        }
    }
}

/// Represents a field decorated with `#[attr]`
///
/// The type associated with an `#[attr]` field represents the concrete value type of the attribute,
/// and thus must implement the `AttributeValue` trait.
#[derive(Debug, Clone)]
pub struct OpAttribute {
    /// The attribute name
    pub name: Ident,
    /// The attribute type for this field
    pub ty: syn::Type,
    /// The derived value type for the attribute this field contains
    pub value_ty: syn::Type,
    /// The attribute kind
    pub kind: AttrKind,
}

/// Represents the type of a symbol
#[derive(Default, Debug, darling::FromMeta, Copy, Clone)]
#[darling(default)]
pub enum AttrKind {
    /// A normal attribute
    #[default]
    Default,
    /// A hidden attribute
    Hidden,
}

/// An abstraction over named vs unnamed groups of some IR entity
#[allow(clippy::large_enum_variant)]
pub enum EntityGroup<T> {
    /// An unnamed group consisting of individual named items
    Unnamed(Vec<T>),
    /// A named group consisting of unnamed items
    Named(Ident, syn::Type),
}

/// A type representing a type constraint applied to a `Value` impl
pub type Constraint = syn::Type;

#[derive(Debug, Clone)]
pub struct Operand {
    pub name: Ident,
    pub constraint: Constraint,
}

pub type OpOperandGroup = EntityGroup<Operand>;

#[derive(Debug, Clone)]
pub struct OpResult {
    pub name: Ident,
    pub constraint: Constraint,
}

pub type OpResultGroup = EntityGroup<OpResult>;

#[derive(Debug)]
#[allow(clippy::large_enum_variant)]
pub enum SuccessorGroup {
    /// An unnamed group consisting of individual named successors
    Unnamed(Vec<Ident>),
    /// A named group consisting of unnamed successors
    Named(Ident),
    /// A named group consisting of unnamed successors with an associated key
    Keyed(Ident, syn::Type),
}

/// Represents the generated `$OpBuilder` type used to create instances of `$Op`
///
/// The implementation of the type requires us to know the type signature specific to this op,
/// so that we can emit an implementation matching that signature.
pub struct OpBuilderImpl {
    /// The `$Op` we're building
    op: Ident,
    /// The `$OpBuilder` type name
    name: Ident,
    /// The doc string for `$OpBuilder`
    doc: DocString,
    /// The doc string for `$OpBuilder::new`
    new_doc: DocString,
    /// The set of parameters expected by `$Op::create`
    ///
    /// The order of these parameters is determined by:
    ///
    /// 1. The `order = N` property of the corresponding attribute type, e.g. `#[attr(order = 1)]`
    /// 2. The default "kind" ordering of: symbols, required user-defined fields, operands, successors, attributes
    /// 3. The order of appearance of the fields in the struct
    create_params: Rc<[OpCreateParam]>,
    /// The implementation of the `BuildableOp` trait for `$Op` via `$OpBuilder`
    buildable_op_impl: BuildableOpImpl,
    /// The implementation of the `FnOnce` trait for `$OpBuilder`
    fn_once_impl: OpBuilderFnOnceImpl,
}
impl OpBuilderImpl {
    pub fn empty(op: Ident) -> Self {
        let name = format_ident!("{}Builder", &op);
        let doc = DocString::new(
            op.span(),
            format!(
                " A specialized builder for [struct@{op}], which is used by calling it like a \
                 function."
            ),
        );
        let new_doc = DocString::new(
            op.span(),
            format!(
                " Get a new [struct@{name}] from the provided [::midenc_hir::Builder] impl and \
                 span."
            ),
        );
        let create_params = Rc::<[OpCreateParam]>::from([]);
        let buildable_op_impl = BuildableOpImpl {
            op: op.clone(),
            op_builder: name.clone(),
            op_generics: Default::default(),
            generics: Default::default(),
            minimal_generics: None,
            params: Rc::clone(&create_params),
        };
        let fn_once_impl = OpBuilderFnOnceImpl {
            op: op.clone(),
            op_builder: name.clone(),
            op_generics: Default::default(),
            all_generics: Default::default(),
            minimal_generics: None,
            params: Rc::clone(&create_params),
        };
        Self {
            op,
            name,
            doc,
            new_doc,
            create_params,
            buildable_op_impl,
            fn_once_impl,
        }
    }

    pub fn set_create_params(&mut self, op_generics: &syn::Generics, params: Vec<OpCreateParam>) {
        let span = self.op.span();

        let create_params = Rc::from(params.into_boxed_slice());
        self.create_params = Rc::clone(&create_params);

        let mut all_generics = syn::Generics {
            lt_token: Some(syn::token::Lt(op_generics.span())),
            params: syn::punctuated::Punctuated::from_iter(
                op_generics
                    .params
                    .iter()
                    .cloned()
                    .chain(create_params.iter().flat_map(|p| p.all_generic_types(false))),
            ),
            gt_token: Some(syn::token::Gt(op_generics.span())),
            where_clause: op_generics.where_clause.clone(),
        };
        {
            let full_where_clause = all_generics.make_where_clause();
            for param in create_params.iter() {
                param.extend_full_where_clause(full_where_clause, false);
            }
        }

        let has_minimal_variant = self.create_params.iter().any(|param| param.default);
        let minimal_generics = if has_minimal_variant {
            let mut min_generics =
                syn::Generics {
                    lt_token: Some(syn::token::Lt(op_generics.span())),
                    params: syn::punctuated::Punctuated::from_iter(
                        op_generics.params.iter().cloned().chain(
                            create_params.iter().flat_map(|p| p.minimal_generic_types(false)),
                        ),
                    ),
                    gt_token: Some(syn::token::Gt(op_generics.span())),
                    where_clause: op_generics.where_clause.clone(),
                };
            {
                let minimal_where_clause = min_generics.make_where_clause();
                for param in create_params.iter() {
                    param.extend_minimal_where_clause(minimal_where_clause, false);
                }
            }
            Some(min_generics)
        } else {
            None
        };

        // BuildableOp generic parameters
        self.buildable_op_impl.params = Rc::clone(&create_params);
        self.buildable_op_impl.op_generics = op_generics.clone();
        self.buildable_op_impl.minimal_generics = minimal_generics;
        self.buildable_op_impl.generics = all_generics;

        let minimal_fn_once_generics = if has_minimal_variant {
            let mut min_generics = syn::Generics {
                lt_token: Some(syn::token::Lt(span)),
                params: syn::punctuated::Punctuated::from_iter(
                    [
                        syn::GenericParam::Lifetime(syn::LifetimeParam {
                            attrs: vec![],
                            lifetime: syn::Lifetime::new("'a", proc_macro2::Span::call_site()),
                            colon_token: None,
                            bounds: Default::default(),
                        }),
                        syn::parse_str("B: ?Sized + ::midenc_hir::Builder").unwrap(),
                    ]
                    .into_iter()
                    .chain(op_generics.params.iter().cloned())
                    .chain(create_params.iter().flat_map(|p| p.minimal_generic_types(true))),
                ),
                gt_token: Some(syn::token::Gt(span)),
                where_clause: op_generics.where_clause.clone(),
            };
            {
                let minimal_where_clause = min_generics.make_where_clause();
                for param in create_params.iter() {
                    param.extend_minimal_where_clause(minimal_where_clause, true);
                }
            }
            Some(min_generics)
        } else {
            None
        };

        let mut all_fn_once_generics = syn::Generics {
            lt_token: Some(syn::token::Lt(span)),
            params: syn::punctuated::Punctuated::from_iter(
                [
                    syn::GenericParam::Lifetime(syn::LifetimeParam {
                        attrs: vec![],
                        lifetime: syn::Lifetime::new("'a", proc_macro2::Span::call_site()),
                        colon_token: None,
                        bounds: Default::default(),
                    }),
                    syn::parse_str("B: ?Sized + ::midenc_hir::Builder").unwrap(),
                ]
                .into_iter()
                .chain(op_generics.params.iter().cloned())
                .chain(create_params.iter().flat_map(|p| p.all_generic_types(true))),
            ),
            gt_token: Some(syn::token::Gt(span)),
            where_clause: op_generics.where_clause.clone(),
        };
        {
            let where_clause = all_fn_once_generics.make_where_clause();
            for param in create_params.iter() {
                param.extend_full_where_clause(where_clause, true);
            }
        }

        // FnOnce generic parameters
        self.fn_once_impl.params = create_params;
        self.fn_once_impl.op_generics = op_generics.clone();
        self.fn_once_impl.minimal_generics = minimal_fn_once_generics;
        self.fn_once_impl.all_generics = all_fn_once_generics;
    }
}
impl quote::ToTokens for OpBuilderImpl {
    fn to_tokens(&self, tokens: &mut proc_macro2::TokenStream) {
        // Emit `$OpBuilder`
        tokens.extend({
            let span = self.name.span();
            let op_builder = &self.name;
            let op_builder_doc = &self.doc;
            let op_builder_new_doc = &self.new_doc;
            quote_spanned! { span =>
                #op_builder_doc
                pub struct #op_builder <'a, B: ?Sized> {
                    builder: &'a mut B,
                    span: ::midenc_hir::diagnostics::SourceSpan,
                }

                impl<'a, B> #op_builder <'a, B>
                where
                    B: ?Sized + ::midenc_hir::Builder,
                {
                    #op_builder_new_doc
                    #[inline(always)]
                    pub fn new(builder: &'a mut B, span: ::midenc_hir::diagnostics::SourceSpan) -> Self {
                        Self {
                            builder,
                            span,
                        }
                    }
                }
            }
        });

        // Emit `impl BuildableOp for $OpBuilder`
        self.buildable_op_impl.to_tokens(tokens);

        // Emit `impl FnOnce for $OpBuilder`
        self.fn_once_impl.to_tokens(tokens);
    }
}

pub struct BuildableOpImpl {
    op: Ident,
    op_builder: Ident,
    op_generics: syn::Generics,
    generics: syn::Generics,
    minimal_generics: Option<syn::Generics>,
    params: Rc<[OpCreateParam]>,
}
impl quote::ToTokens for BuildableOpImpl {
    fn to_tokens(&self, tokens: &mut proc_macro2::TokenStream) {
        let op = &self.op;
        let op_builder = &self.op_builder;
        let op_span = op.span();

        // Minimal builder (specify only required parameters)
        //
        // NOTE: This is only emitted if there are `default` parameters
        if let Some(min_generics) = self.minimal_generics.as_ref() {
            let required_params = self.params.iter().flat_map(|p| p.minimal_binding_types(false));
            let (_, required_ty_generics, _) = self.op_generics.split_for_impl();
            let (required_impl_generics, _, required_where_clause) = min_generics.split_for_impl();
            let required_params_ty = syn::TypeTuple {
                paren_token: syn::token::Paren(op_span),
                elems: syn::punctuated::Punctuated::from_iter(required_params),
            };
            let quoted = quote_spanned! { op_span =>
                impl #required_impl_generics ::midenc_hir::BuildableOp<#required_params_ty> for #op #required_ty_generics #required_where_clause {
                    type Builder<'a, T: ?Sized + ::midenc_hir::Builder + 'a> = #op_builder <'a, T>;

                    #[inline(always)]
                    fn builder<'b, B>(builder: &'b mut B, span: ::midenc_hir::diagnostics::SourceSpan) -> Self::Builder<'b, B>
                    where
                        B: ?Sized + ::midenc_hir::Builder + 'b,
                    {
                        #op_builder {
                            builder,
                            span,
                        }
                    }
                }
            };
            tokens.extend(quoted);
        }

        // Maximal builder (specify all parameters)
        let params = self.params.iter().flat_map(|p| p.all_binding_types(false));
        let (_, ty_generics, _) = self.op_generics.split_for_impl();
        let (impl_generics, _, where_clause) = self.generics.split_for_impl();
        let params_ty = syn::TypeTuple {
            paren_token: syn::token::Paren(op.span()),
            elems: syn::punctuated::Punctuated::from_iter(params),
        };
        let quoted = quote_spanned! { op_span =>
            impl #impl_generics ::midenc_hir::BuildableOp<#params_ty> for #op #ty_generics #where_clause {
                type Builder<'a, T: ?Sized + ::midenc_hir::Builder + 'a> = #op_builder <'a, T>;

                #[inline(always)]
                fn builder<'b, B>(builder: &'b mut B, span: ::midenc_hir::diagnostics::SourceSpan) -> Self::Builder<'b, B>
                where
                    B: ?Sized + ::midenc_hir::Builder + 'b,
                {
                    #op_builder {
                        builder,
                        span,
                    }
                }
            }
        };
        tokens.extend(quoted);
    }
}

pub struct OpBuilderFnOnceImpl {
    op: Ident,
    op_builder: Ident,
    all_generics: syn::Generics,
    minimal_generics: Option<syn::Generics>,
    op_generics: syn::Generics,
    params: Rc<[OpCreateParam]>,
}
impl quote::ToTokens for OpBuilderFnOnceImpl {
    fn to_tokens(&self, tokens: &mut proc_macro2::TokenStream) {
        let op = &self.op;
        let op_span = op.span();
        let op_builder = &self.op_builder;

        let all_param_names =
            self.params.iter().flat_map(OpCreateParam::all_bindings).collect::<Vec<_>>();
        let (_, op_ty_generics, _) = self.op_generics.split_for_impl();

        // Minimal builder (specify only required parameters)
        //
        // NOTE: This is only emitted if there are `default` parameters
        if let Some(min_generics) = self.minimal_generics.as_ref() {
            let required_param_names = self.params.iter().flat_map(|p| p.minimal_bindings());
            let required_param_types =
                self.params.iter().flat_map(|p| p.minimal_binding_types(true));
            let defaulted_param_names = self.params.iter().flat_map(|param| {
                if param.default {
                    param.all_bindings()
                } else {
                    vec![]
                }
            });
            let defaulted_param_types = self.params.iter().flat_map(|param| {
                if param.default {
                    param.all_binding_types(false)
                } else {
                    vec![]
                }
            });
            let defaulted_param_stmts =
                defaulted_param_names.zip(defaulted_param_types).map(|(name, ty)| {
                    let stmt: syn::Stmt = parse_quote_spanned! { name.span() =>
                        let #name: #ty = Default::default();
                    };
                    stmt
                });
            let (min_impl_generics, _, min_where_clause) = min_generics.split_for_impl();
            let required_params_ty = syn::TypeTuple {
                paren_token: syn::token::Paren(op_span),
                elems: syn::punctuated::Punctuated::from_iter(required_param_types),
            };
            let required_params_bound = syn::PatTuple {
                attrs: Default::default(),
                paren_token: syn::token::Paren(op_span),
                elems: syn::punctuated::Punctuated::from_iter(
                    required_param_names.into_iter().map(|id| {
                        syn::Pat::Ident(syn::PatIdent {
                            attrs: Default::default(),
                            by_ref: None,
                            mutability: None,
                            ident: id,
                            subpat: None,
                        })
                    }),
                ),
            };
            tokens.extend(quote_spanned! { op_span =>
                #[allow(clippy::type_complexity)]
                impl #min_impl_generics ::core::ops::FnOnce<#required_params_ty> for #op_builder<'a, B> #min_where_clause {
                    type Output = Result<::midenc_hir::UnsafeIntrusiveEntityRef<#op #op_ty_generics>, ::midenc_hir::diagnostics::Report>;

                    #[inline]
                    extern "rust-call" fn call_once(self, args: #required_params_ty) -> Self::Output {
                        let #required_params_bound = args;
                        #(
                            #defaulted_param_stmts
                        )*
                        <#op #op_ty_generics>::create(self.builder, self.span, #(#all_param_names),*)
                    }
                }
            });
        }

        // Maximal builder (specify all parameters)
        let all_param_types = self.params.iter().flat_map(|p| p.all_binding_types(true));
        let (impl_generics, _, where_clause) = self.all_generics.split_for_impl();
        let params_ty = syn::TypeTuple {
            paren_token: syn::token::Paren(op_span),
            elems: syn::punctuated::Punctuated::from_iter(all_param_types),
        };
        let params_bound = syn::PatTuple {
            attrs: Default::default(),
            paren_token: syn::token::Paren(op_span),
            elems: syn::punctuated::Punctuated::from_iter(all_param_names.iter().map(|id| {
                syn::Pat::Ident(syn::PatIdent {
                    attrs: Default::default(),
                    by_ref: None,
                    mutability: None,
                    ident: id.clone(),
                    subpat: None,
                })
            })),
        };
        tokens.extend(quote_spanned! { op_span =>
            #[allow(clippy::type_complexity)]
            impl #impl_generics ::core::ops::FnOnce<#params_ty> for #op_builder<'a, B> #where_clause {
                type Output = Result<::midenc_hir::UnsafeIntrusiveEntityRef<#op #op_ty_generics>, ::midenc_hir::diagnostics::Report>;

                #[inline]
                extern "rust-call" fn call_once(self, args: #params_ty) -> Self::Output {
                    let #params_bound = args;
                    <#op #op_ty_generics>::create(self.builder, self.span, #(#all_param_names),*)
                }
            }
        });
    }
}

pub struct OpVerifierImpl {
    op: Ident,
    traits: darling::util::PathList,
    implements: darling::util::PathList,
}
impl OpVerifierImpl {
    pub fn new(
        op: Ident,
        traits: darling::util::PathList,
        implements: darling::util::PathList,
    ) -> Self {
        Self {
            op,
            traits,
            implements,
        }
    }
}
impl quote::ToTokens for OpVerifierImpl {
    fn to_tokens(&self, tokens: &mut proc_macro2::TokenStream) {
        let op = &self.op;
        let span = op.span();
        if self.traits.is_empty() && self.implements.is_empty() {
            tokens.extend(quote_spanned! { span =>
                /// No-op verifier implementation generated via `#[operation]` derive
                ///
                /// This implementation was chosen as no op traits were indicated as being derived _or_
                /// manually implemented by this type.
                impl ::midenc_hir::OpVerifier for #op {
                    #[inline(always)]
                    fn verify(&self, _context: &::midenc_hir::Context) -> Result<(), ::midenc_hir::diagnostics::Report> {
                        Ok(())
                    }
                }
            });
            return;
        }

        let op_verifier_doc_lines = {
            let mut lines = vec![
                syn::Lit::Str(syn::LitStr::new(
                    " Generated verifier implementation via `#[operation]` attribute",
                    span,
                )),
                syn::Lit::Str(syn::LitStr::new("", span)),
                syn::Lit::Str(syn::LitStr::new(" Traits verified by this implementation:", span)),
                syn::Lit::Str(syn::LitStr::new("", span)),
            ];
            for derived_trait in self.traits.iter() {
                lines.push(syn::Lit::Str(syn::LitStr::new(
                    &format!(" * `{}`", derived_trait.get_ident().unwrap()),
                    span,
                )));
            }
            for implemented_trait in self.implements.iter() {
                lines.push(syn::Lit::Str(syn::LitStr::new(
                    &format!(" * `{}`", implemented_trait.get_ident().unwrap()),
                    span,
                )));
            }
            lines.push(syn::Lit::Str(syn::LitStr::new("", span)));
            lines.push(syn::Lit::Str(syn::LitStr::new(
                " Use `cargo-expand` to view the generated code if you suspect verification is \
                 broken.",
                span,
            )));
            lines
        };

        let derived_traits = &self.traits;
        let implemented_traits = &self.implements;
        tokens.extend(quote_spanned! { span =>
            #(
                #[doc = #op_verifier_doc_lines]
            )*
            impl ::midenc_hir::OpVerifier for #op {
                fn verify(&self, context: &::midenc_hir::Context) -> Result<(), ::midenc_hir::diagnostics::Report> {
                    /// This type represents the concrete set of derived traits for some op `T`, paired with a
                    /// type-erased [::midenc_hir::Operation] reference for an instance of that op.
                    ///
                    /// This is used for two purposes:
                    ///
                    /// 1. To generate a specialized [::midenc_hir::OpVerifier] for `T` which contains all of the type and
                    ///    trait-specific validation logic for that `T`.
                    /// 2. To apply the specialized verifier for `T` using the wrapped [::midenc_hir::Operation] reference.
                    struct OpVerifierImpl<'a, T> {
                        op: &'a ::midenc_hir::Operation,
                        _t: ::core::marker::PhantomData<T>,
                        #[allow(unused_parens, clippy::type_complexity)]
                        _derived: ::core::marker::PhantomData<(#(&'a dyn #derived_traits,)* #(&'a dyn #implemented_traits),*)>,
                    }
                    impl<'a, T> OpVerifierImpl<'a, T> {
                        const fn new(op: &'a ::midenc_hir::Operation) -> Self {
                            Self {
                                op,
                                _t: ::core::marker::PhantomData,
                                _derived: ::core::marker::PhantomData,
                            }
                        }
                    }
                    impl<'a, T> ::core::ops::Deref for OpVerifierImpl<'a, T> {
                        type Target = ::midenc_hir::Operation;

                        fn deref(&self) -> &Self::Target {
                            self.op
                        }
                    }

                    #[allow(unused_parens, clippy::type_complexity)]
                    impl<'a> ::midenc_hir::OpVerifier for OpVerifierImpl<'a, #op>
                    where
                        #(
                            #op: ::midenc_hir::verifier::Verifier<dyn #derived_traits>,
                        )*
                        #(
                            #op: ::midenc_hir::verifier::Verifier<dyn #implemented_traits>,
                        )*
                    {
                        #[inline]
                        fn verify(&self, context: &::midenc_hir::Context) -> Result<(), ::midenc_hir::diagnostics::Report> {
                            let op = self.downcast_ref::<#op>().unwrap();
                            #(
                                if const { !<#op as ::midenc_hir::verifier::Verifier<dyn #derived_traits>>::VACUOUS } {
                                    <#op as ::midenc_hir::verifier::Verifier<dyn #derived_traits>>::maybe_verify(op, context)?;
                                }
                            )*
                            #(
                                if const { !<#op as ::midenc_hir::verifier::Verifier<dyn #implemented_traits>>::VACUOUS } {
                                    <#op as ::midenc_hir::verifier::Verifier<dyn #implemented_traits>>::maybe_verify(op, context)?;
                                }
                            )*

                            Ok(())
                        }
                    }

                    let verifier = OpVerifierImpl::<#op>::new(&self.op);
                    verifier.verify(context)
                }
            }
        });
    }
}

/// Represents the parsed struct definition for the operation we wish to define
///
/// Only named structs are allowed at this time.
#[derive(Debug, FromDeriveInput)]
#[darling(
    attributes(operation),
    supports(struct_named),
    forward_attrs(doc, cfg, allow, derive)
)]
pub struct Operation {
    ident: Ident,
    vis: syn::Visibility,
    generics: syn::Generics,
    attrs: Vec<syn::Attribute>,
    data: darling::ast::Data<(), OperationField>,
    dialect: Ident,
    #[darling(default)]
    name: Option<Ident>,
    #[darling(default)]
    traits: darling::util::PathList,
    #[darling(default)]
    implements: darling::util::PathList,
}

/// Represents a field in the input struct
#[derive(Debug, FromField)]
#[darling(forward_attrs(
    doc, cfg, allow, attr, operand, operands, region, successor, successors, result, results,
    default, order, symbol
))]
pub struct OperationField {
    /// The name of this field.
    ///
    /// This will always be `Some`, as we do not support any types other than structs
    pub ident: Option<Ident>,
    /// The visibility assigned to this field
    pub vis: syn::Visibility,
    /// The type assigned to this field
    pub ty: syn::Type,
    /// The processed attributes of this field
    #[darling(with = OperationFieldAttrs::new)]
    pub attrs: OperationFieldAttrs,
}

#[derive(Default, Debug)]
pub struct OperationFieldAttrs {
    /// Attributes we don't care about, and are forwarding along untouched
    pub forwarded: Vec<syn::Attribute>,
    /// Whether or not to create instances of this op using the `Default` impl for this field
    pub r#default: Flag,
    /// Whether or not to assign an explicit order to this field.
    ///
    /// Once an explicit order has been assigned to a field, all subsequent fields must either have
    /// an explicit order, or they will be assigned the next largest unallocated index in the order.
    pub order: Option<u32>,
    /// Was this an `#[attr]` field?
    pub attr: Option<SpannedValue<Option<AttrKind>>>,
    /// Was this an `#[operand]` field?
    pub operand: Flag,
    /// Was this an `#[operands]` field?
    pub operands: Flag,
    /// Was this a `#[result]` field?
    pub result: Flag,
    /// Was this a `#[results]` field?
    pub results: Flag,
    /// Was this a `#[region]` field?
    pub region: Option<SpannedValue<Option<RegionOptions>>>,
    /// Was this a `#[successor]` field?
    pub successor: Flag,
    /// Was this a `#[successors]` field?
    pub successors: Option<SpannedValue<SuccessorsType>>,
    /// Was this a `#[symbol]` field?
    pub symbol: Option<SpannedValue<Option<SymbolType>>>,
}

#[derive(Default, Debug, Clone)]
pub struct RegionOptions {
    pub name: Option<String>,
}

impl OperationFieldAttrs {
    pub fn new(attrs: Vec<syn::Attribute>) -> darling::Result<Self> {
        let mut result = Self::default();
        let mut prev_decorator = None;
        for attr in attrs {
            if let Some(name) = attr.path().get_ident().map(|id| id.to_string()) {
                match name.as_str() {
                    "attr" => {
                        if let Some(prev) = prev_decorator.replace("attr") {
                            return Err(Error::custom(format!(
                                "#[attr] conflicts with a previous #[{prev}] decorator"
                            ))
                            .with_span(&attr));
                        }
                        let span = attr.span();
                        let mut kind = None;
                        match &attr.meta {
                            // A bare #[attr], nothing to do
                            syn::Meta::Path(_) => (),
                            syn::Meta::List(list) => {
                                list.parse_nested_meta(|meta| {
                                    if meta.path.is_ident("hidden") {
                                        kind = Some(AttrKind::Hidden);
                                        Ok(())
                                    } else {
                                        Err(meta.error(format!(
                                            "invalid #[attr] decorator: unrecognized key '{}'",
                                            meta.path.get_ident().unwrap()
                                        )))
                                    }
                                })
                                .map_err(Error::from)?;
                            }
                            meta @ syn::Meta::NameValue(_) => {
                                return Err(Error::custom(
                                    "invalid #[attr] decorator: invalid format, expected either \
                                     bare 'attr' or a meta list",
                                )
                                .with_span(meta));
                            }
                        }
                        result.attr = Some(SpannedValue::new(kind, span));
                    }
                    "operand" => {
                        if let Some(prev) = prev_decorator.replace("operand") {
                            return Err(Error::custom(format!(
                                "#[operand] conflicts with a previous #[{prev}] decorator"
                            ))
                            .with_span(&attr));
                        }
                        result.operand = Flag::from_meta(&attr.meta).unwrap();
                    }
                    "operands" => {
                        if let Some(prev) = prev_decorator.replace("operands") {
                            return Err(Error::custom(format!(
                                "#[operands] conflicts with a previous #[{prev}] decorator"
                            ))
                            .with_span(&attr));
                        }
                        result.operands = Flag::from_meta(&attr.meta).unwrap();
                    }
                    "result" => {
                        if let Some(prev) = prev_decorator.replace("result") {
                            return Err(Error::custom(format!(
                                "#[result] conflicts with a previous #[{prev}] decorator"
                            ))
                            .with_span(&attr));
                        }
                        result.result = Flag::from_meta(&attr.meta).unwrap();
                    }
                    "results" => {
                        if let Some(prev) = prev_decorator.replace("results") {
                            return Err(Error::custom(format!(
                                "#[results] conflicts with a previous #[{prev}] decorator"
                            ))
                            .with_span(&attr));
                        }
                        result.results = Flag::from_meta(&attr.meta).unwrap();
                    }
                    "region" => {
                        if let Some(prev) = prev_decorator.replace("region") {
                            return Err(Error::custom(format!(
                                "#[region] conflicts with a previous #[{prev}] decorator"
                            ))
                            .with_span(&attr));
                        }
                        let span = attr.span();
                        let mut region_options = None::<RegionOptions>;
                        if attr.meta.require_path_only().is_ok() {
                            result.region = Some(SpannedValue::new(region_options, span));
                        } else {
                            match attr.parse_nested_meta(|meta| {
                                if meta.path.is_ident("name") {
                                    let value = meta.value()?;
                                    let name: syn::LitStr = value.parse()?;
                                    region_options.get_or_insert_default().name =
                                        Some(name.value());
                                    Ok(())
                                } else {
                                    Err(meta.error(format!(
                                        "invalid #[region] option '{}'",
                                        meta.path.to_token_stream()
                                    )))
                                }
                            }) {
                                Ok(_) => {
                                    result.region = Some(SpannedValue::new(region_options, span));
                                }
                                Err(err) => return Err(Error::from(err)),
                            }
                        }
                    }
                    "successor" => {
                        if let Some(prev) = prev_decorator.replace("successor") {
                            return Err(Error::custom(format!(
                                "#[successor] conflicts with a previous #[{prev}] decorator"
                            ))
                            .with_span(&attr));
                        }
                        result.successor = Flag::from_meta(&attr.meta).unwrap();
                    }
                    "successors" => {
                        if let Some(prev) = prev_decorator.replace("successors") {
                            return Err(Error::custom(format!(
                                "#[successors] conflicts with a previous #[{prev}] decorator"
                            ))
                            .with_span(&attr));
                        }
                        let span = attr.span();
                        let mut succ_ty = SuccessorsType::Default;
                        match attr.parse_nested_meta(|meta| {
                            if meta.path.is_ident("keyed") {
                                succ_ty = SuccessorsType::Keyed;
                                Ok(())
                            } else {
                                Err(meta.error(format!(
                                    "invalid #[successors] decorator: unrecognized key '{}'",
                                    meta.path.get_ident().unwrap()
                                )))
                            }
                        }) {
                            Ok(_) => {
                                result.successors = Some(SpannedValue::new(succ_ty, span));
                            }
                            Err(err) => {
                                return Err(Error::from(err));
                            }
                        }
                    }
                    "symbol" => {
                        if let Some(prev) = prev_decorator.replace("symbol") {
                            return Err(Error::custom(format!(
                                "#[symbol] conflicts with a previous #[{prev}] decorator"
                            ))
                            .with_span(&attr));
                        }
                        let span = attr.span();
                        let mut symbol_ty = None;
                        match &attr.meta {
                            // A bare #[symbol], nothing to do
                            syn::Meta::Path(_) => (),
                            syn::Meta::List(list) => {
                                list.parse_nested_meta(|meta| {
                                    if meta.path.is_ident("callable") {
                                        symbol_ty = Some(SymbolType::Callable);
                                        Ok(())
                                    } else if meta.path.is_ident("any") {
                                        symbol_ty = Some(SymbolType::Any);
                                        Ok(())
                                    } else if meta.path.is_ident("bounds") {
                                        let symbol_bound = meta
                                            .input
                                            .parse::<SymbolTraitBound>()
                                            .map_err(Error::from)?;
                                        symbol_ty = Some(symbol_bound.into());
                                        Ok(())
                                    } else {
                                        Err(meta.error(format!(
                                            "invalid #[symbol] decorator: unrecognized key '{}'",
                                            meta.path.get_ident().unwrap()
                                        )))
                                    }
                                })
                                .map_err(Error::from)?;
                            }
                            meta @ syn::Meta::NameValue(_) => {
                                return Err(Error::custom(
                                    "invalid #[symbol] decorator: invalid format, expected either \
                                     bare 'symbol' or a meta list",
                                )
                                .with_span(meta));
                            }
                        }
                        result.symbol = Some(SpannedValue::new(symbol_ty, span));
                    }
                    "default" => {
                        result.default = Flag::present();
                    }
                    "order" => {
                        result.order = Some(
                            attr.parse_args::<syn::LitInt>()
                                .map_err(Error::from)
                                .and_then(|n| n.base10_parse::<u32>().map_err(Error::from))?,
                        );
                    }
                    _ => {
                        result.forwarded.push(attr);
                    }
                }
            } else {
                result.forwarded.push(attr);
            }
        }

        Ok(result)
    }
}

impl OperationFieldAttrs {
    pub fn pseudo_type(&self) -> Option<darling::util::SpannedValue<OperationFieldType>> {
        use darling::util::SpannedValue;
        if self.attr.is_some() {
            self.attr
                .as_ref()
                .map(|kind| kind.map_ref(|kind| OperationFieldType::Attr(kind.unwrap_or_default())))
        } else if self.operand.is_present() {
            Some(SpannedValue::new(OperationFieldType::Operand, self.operand.span()))
        } else if self.operands.is_present() {
            Some(SpannedValue::new(OperationFieldType::Operands, self.operands.span()))
        } else if self.result.is_present() {
            Some(SpannedValue::new(OperationFieldType::Result, self.result.span()))
        } else if self.results.is_present() {
            Some(SpannedValue::new(OperationFieldType::Results, self.results.span()))
        } else if self.region.is_some() {
            self.region.as_ref().map(|region| {
                region.map_ref(|s| OperationFieldType::Region(s.clone().unwrap_or_default()))
            })
        } else if self.successor.is_present() {
            Some(SpannedValue::new(OperationFieldType::Successor, self.successor.span()))
        } else if self.successors.is_some() {
            self.successors.map(|succ| succ.map_ref(|s| OperationFieldType::Successors(*s)))
        } else if self.symbol.is_some() {
            self.symbol
                .as_ref()
                .map(|sym| sym.map_ref(|sym| OperationFieldType::Symbol(sym.clone())))
        } else {
            None
        }
    }
}

#[derive(Debug, Clone)]
#[allow(clippy::large_enum_variant)]
pub enum OperationFieldType {
    /// An operation attribute
    Attr(AttrKind),
    /// A named operand
    Operand,
    /// A named variadic operand group (zero or more operands)
    Operands,
    /// A named result
    Result,
    /// A named variadic result group (zero or more results)
    Results,
    /// A named region
    Region(RegionOptions),
    /// A named successor
    Successor,
    /// A named variadic successor group (zero or more successors)
    Successors(SuccessorsType),
    /// A symbol operand
    ///
    /// Symbols are handled differently than regular operands, as they are not SSA values, and
    /// are tracked using a different use/def graph than normal values.
    ///
    /// If the symbol type is `None`, it implies we should use the concrete field type as the
    /// expected symbol type. Otherwise, use the provided symbol type to derive bounds for that
    /// field.
    Symbol(Option<SymbolType>),
}
impl core::fmt::Display for OperationFieldType {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Attr(AttrKind::Default) => f.write_str("attr"),
            Self::Attr(AttrKind::Hidden) => f.write_str("attr(hidden)"),
            Self::Operand => f.write_str("operand"),
            Self::Operands => f.write_str("operands"),
            Self::Result => f.write_str("result"),
            Self::Results => f.write_str("results"),
            Self::Region(RegionOptions { name: None }) => f.write_str("region"),
            Self::Region(RegionOptions { name: Some(name) }) => {
                write!(f, "region(name = {name:?})")
            }
            Self::Successor => f.write_str("successor"),
            Self::Successors(SuccessorsType::Default) => f.write_str("successors"),
            Self::Successors(SuccessorsType::Keyed) => f.write_str("successors(keyed)"),
            Self::Symbol(None) => f.write_str("symbol"),
            Self::Symbol(Some(SymbolType::Any)) => f.write_str("symbol(any)"),
            Self::Symbol(Some(SymbolType::Callable)) => f.write_str("symbol(callable)"),
            Self::Symbol(Some(SymbolType::Concrete(_))) => write!(f, "symbol(concrete)"),
            Self::Symbol(Some(SymbolType::Trait(_))) => write!(f, "symbol(trait)"),
        }
    }
}

/// The type of successor group
#[derive(Default, Debug, darling::FromMeta, Copy, Clone)]
#[darling(default)]
pub enum SuccessorsType {
    /// The default successor type consists of a `BlockRef` and an iterable of `ValueRef`
    #[default]
    Default,
    /// A keyed successor is a custom type that implements the `KeyedSuccessor` trait
    Keyed,
}

/// Represents parameter information for `$Op::create` and the associated builder infrastructure.
#[derive(Debug)]
pub struct OpCreateParam {
    /// The actual parameter type and payload
    param_ty: OpCreateParamType,
    /// Is this value initialized using `Default::default` when `Op::create` is called?
    r#default: bool,
}

#[derive(Debug)]
#[allow(clippy::large_enum_variant)]
pub enum OpCreateParamType {
    Attr(OpAttribute),
    Operand(Operand),
    #[allow(dead_code)]
    OperandGroup(Ident, syn::Type),
    CustomField(Ident, syn::Type),
    Successor(Ident),
    SuccessorGroupNamed(Ident),
    SuccessorGroupKeyed(Ident, syn::Type),
    Symbol(Symbol),
}
impl OpCreateParam {
    /// Returns the names of all bindings implied by this parameter.
    pub fn all_bindings(&self) -> Vec<Ident> {
        match &self.param_ty {
            OpCreateParamType::Attr(OpAttribute { name, .. })
            | OpCreateParamType::CustomField(name, _)
            | OpCreateParamType::Operand(Operand { name, .. })
            | OpCreateParamType::OperandGroup(name, _)
            | OpCreateParamType::SuccessorGroupNamed(name)
            | OpCreateParamType::SuccessorGroupKeyed(name, _)
            | OpCreateParamType::Symbol(Symbol { name, .. }) => vec![name.clone()],
            OpCreateParamType::Successor(name) => {
                vec![name.clone(), format_ident!("{}_args", name)]
            }
        }
    }

    /// Returns the names of all required (i.e. non-defaulted) bindings implied by this parameter.
    pub fn minimal_bindings(&self) -> Vec<Ident> {
        if self.default {
            return vec![];
        }
        self.all_bindings()
    }

    /// Returns the types assigned to the bindings returned by [Self::bindings]
    pub fn all_binding_types(&self, allow_attr_conversions: bool) -> Vec<syn::Type> {
        match &self.param_ty {
            OpCreateParamType::Attr(OpAttribute { name, .. }) if allow_attr_conversions => {
                vec![make_type(format!("T{}Value", name.to_string().to_pascal_case()))]
            }
            OpCreateParamType::Attr(OpAttribute { value_ty, .. }) => {
                vec![value_ty.clone()]
            }
            OpCreateParamType::CustomField(_, ty) => {
                vec![ty.clone()]
            }
            OpCreateParamType::Operand(_) => vec![make_type("::midenc_hir::ValueRef")],
            OpCreateParamType::OperandGroup(group_name, _)
            | OpCreateParamType::SuccessorGroupNamed(group_name)
            | OpCreateParamType::SuccessorGroupKeyed(group_name, _) => {
                vec![make_type(format!("T{}", group_name.to_string().to_pascal_case()))]
            }
            OpCreateParamType::Successor(name) => vec![
                make_type("::midenc_hir::BlockRef"),
                make_type(format!("T{}Args", name.to_string().to_pascal_case())),
            ],
            OpCreateParamType::Symbol(Symbol { name, ty }) => match ty {
                SymbolType::Any | SymbolType::Callable | SymbolType::Trait(_) => {
                    vec![make_type(format!("T{}", name.to_string().to_pascal_case()))]
                }
                SymbolType::Concrete(ty) => {
                    vec![parse_quote! { ::midenc_hir::UnsafeIntrusiveEntityRef<#ty> }]
                }
            },
        }
    }

    /// Returns the types assigned to the bindings returned by [Self::required_bindings]
    pub fn minimal_binding_types(&self, allow_attr_conversions: bool) -> Vec<syn::Type> {
        if self.default {
            return vec![];
        }
        self.all_binding_types(allow_attr_conversions)
    }

    pub fn extend_minimal_where_clause(
        &self,
        where_clause: &mut syn::WhereClause,
        allow_attr_conversions: bool,
    ) {
        if self.default {
            return;
        }
        self.extend_full_where_clause(where_clause, allow_attr_conversions);
    }

    pub fn extend_full_where_clause(
        &self,
        where_clause: &mut syn::WhereClause,
        allow_attr_conversions: bool,
    ) {
        #[allow(clippy::single_match)]
        match &self.param_ty {
            OpCreateParamType::Attr(OpAttribute { name, value_ty, .. })
                if allow_attr_conversions =>
            {
                let param_ty = format_ident!(
                    "T{}Value",
                    name.to_string().to_pascal_case(),
                    span = name.span()
                );
                where_clause.predicates.push(parse_quote_spanned! { name.span() =>
                    #value_ty: From<#param_ty>
                });
            }
            _ => (),
        }
    }

    /// Returns the generic type parameters bound for use by the types in [Self::binding_typess]
    pub fn all_generic_types(&self, allow_attr_conversions: bool) -> Vec<syn::GenericParam> {
        match &self.param_ty {
            OpCreateParamType::Attr(OpAttribute { name, .. }) if allow_attr_conversions => {
                vec![syn::GenericParam::Type(syn::TypeParam {
                    attrs: vec![],
                    ident: format_ident!(
                        "T{}Value",
                        &name.to_string().to_pascal_case(),
                        span = name.span()
                    ),
                    colon_token: Some(syn::token::Colon(name.span())),
                    bounds: syn::punctuated::Punctuated::new(),
                    eq_token: None,
                    r#default: None,
                })]
            }
            OpCreateParamType::OperandGroup(name, _) => {
                let value_iter_bound: syn::TypeParamBound =
                    syn::parse_str("IntoIterator<Item = ::midenc_hir::ValueRef>").unwrap();
                vec![syn::GenericParam::Type(syn::TypeParam {
                    attrs: vec![],
                    ident: format_ident!(
                        "T{}",
                        &name.to_string().to_pascal_case(),
                        span = name.span()
                    ),
                    colon_token: Some(syn::token::Colon(name.span())),
                    bounds: syn::punctuated::Punctuated::from_iter([value_iter_bound]),
                    eq_token: None,
                    r#default: None,
                })]
            }
            OpCreateParamType::Successor(name) => {
                let value_iter_bound: syn::TypeParamBound =
                    syn::parse_str("IntoIterator<Item = ::midenc_hir::ValueRef>").unwrap();
                vec![syn::GenericParam::Type(syn::TypeParam {
                    attrs: vec![],
                    ident: format_ident!(
                        "T{}Args",
                        &name.to_string().to_pascal_case(),
                        span = name.span()
                    ),
                    colon_token: Some(syn::token::Colon(name.span())),
                    bounds: syn::punctuated::Punctuated::from_iter([value_iter_bound]),
                    eq_token: None,
                    r#default: None,
                })]
            }
            OpCreateParamType::SuccessorGroupNamed(name) => {
                let value_iter_bound: syn::TypeParamBound = syn::parse_str(
                    "IntoIterator<Item = (::midenc_hir::BlockRef, \
                     ::alloc::vec::Vec<::midenc_hir::ValueRef>)>",
                )
                .unwrap();
                vec![syn::GenericParam::Type(syn::TypeParam {
                    attrs: vec![],
                    ident: format_ident!(
                        "T{}",
                        &name.to_string().to_pascal_case(),
                        span = name.span()
                    ),
                    colon_token: Some(syn::token::Colon(name.span())),
                    bounds: syn::punctuated::Punctuated::from_iter([value_iter_bound]),
                    eq_token: None,
                    r#default: None,
                })]
            }
            OpCreateParamType::SuccessorGroupKeyed(name, ty) => {
                let item_name = name.to_string().to_pascal_case();
                let iterator_ty = format_ident!("T{item_name}", span = name.span());
                vec![syn::parse_quote! {
                    #iterator_ty: IntoIterator<Item = #ty>
                }]
            }
            OpCreateParamType::Symbol(Symbol { name, ty }) => match ty {
                SymbolType::Any => {
                    let as_symbol_ref_bound =
                        syn::parse_str::<syn::TypeParamBound>("::midenc_hir::AsSymbolRef").unwrap();
                    vec![syn::GenericParam::Type(syn::TypeParam {
                        attrs: vec![],
                        ident: format_ident!("T{}", name.to_string().to_pascal_case()),
                        colon_token: Some(syn::token::Colon(name.span())),
                        bounds: syn::punctuated::Punctuated::from_iter([as_symbol_ref_bound]),
                        eq_token: None,
                        r#default: None,
                    })]
                }
                SymbolType::Callable => {
                    let as_callable_symbol_ref_bound =
                        syn::parse_str::<syn::TypeParamBound>("::midenc_hir::AsCallableSymbolRef")
                            .unwrap();
                    vec![syn::GenericParam::Type(syn::TypeParam {
                        attrs: vec![],
                        ident: format_ident!("T{}", name.to_string().to_pascal_case()),
                        colon_token: Some(syn::token::Colon(name.span())),
                        bounds: syn::punctuated::Punctuated::from_iter([
                            as_callable_symbol_ref_bound,
                        ]),
                        eq_token: None,
                        r#default: None,
                    })]
                }
                SymbolType::Concrete(_) => vec![],
                SymbolType::Trait(bounds) => {
                    let as_symbol_ref_bound = syn::parse_str("::midenc_hir::AsSymbolRef").unwrap();
                    vec![syn::GenericParam::Type(syn::TypeParam {
                        attrs: vec![],
                        ident: format_ident!("T{}", name.to_string().to_pascal_case()),
                        colon_token: Some(syn::token::Colon(name.span())),
                        bounds: syn::punctuated::Punctuated::from_iter(
                            [as_symbol_ref_bound].into_iter().chain(bounds.iter().cloned()),
                        ),
                        eq_token: None,
                        r#default: None,
                    })]
                }
            },
            _ => vec![],
        }
    }

    /// Returns the generic type parameters bound for use by the types in [Self::required_binding_typess]
    pub fn minimal_generic_types(&self, allow_attr_conversions: bool) -> Vec<syn::GenericParam> {
        if self.default {
            return vec![];
        }
        self.all_generic_types(allow_attr_conversions)
    }
}

/// A symbol value
#[derive(Debug, Clone)]
pub struct Symbol {
    pub name: Ident,
    pub ty: SymbolType,
}

/// Represents the type of a symbol
#[derive(Debug, Clone)]
pub enum SymbolType {
    /// Any `Symbol` implementation can be used
    Any,
    /// Any `Symbol + CallableOpInterface` implementation can be used
    Callable,
    /// Only the specific concrete type can be used, it must implement `Op` and `Symbol` traits
    Concrete(syn::Type),
    /// Any implementation of the provided trait can be used.
    ///
    /// The given trait type _must_ have `Symbol` as a supertrait.
    Trait(syn::punctuated::Punctuated<syn::TypeParamBound, Token![+]>),
}

struct SymbolTraitBound {
    _eq_token: Token![=],
    bounds: syn::punctuated::Punctuated<syn::TypeParamBound, Token![+]>,
}
impl syn::parse::Parse for SymbolTraitBound {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let lookahead = input.lookahead1();
        if !lookahead.peek(Token![=]) {
            return Err(lookahead.error());
        }

        let _eq_token = input.parse::<Token![=]>()?;
        let bounds = syn::punctuated::Punctuated::parse_separated_nonempty(input)?;

        Ok(Self { _eq_token, bounds })
    }
}
impl From<SymbolTraitBound> for SymbolType {
    #[inline]
    fn from(value: SymbolTraitBound) -> Self {
        SymbolType::Trait(value.bounds)
    }
}

pub struct DocString {
    span: proc_macro2::Span,
    doc: String,
}
impl DocString {
    pub fn new(span: proc_macro2::Span, doc: String) -> Self {
        Self { span, doc }
    }
}
impl quote::ToTokens for DocString {
    fn to_tokens(&self, tokens: &mut proc_macro2::TokenStream) {
        let attr = syn::Attribute {
            pound_token: syn::token::Pound(self.span),
            style: syn::AttrStyle::Outer,
            bracket_token: syn::token::Bracket(self.span),
            meta: syn::Meta::NameValue(syn::MetaNameValue {
                path: attr_path("doc"),
                eq_token: syn::token::Eq(self.span),
                value: syn::Expr::Lit(syn::ExprLit {
                    attrs: vec![],
                    lit: syn::Lit::Str(syn::LitStr::new(&self.doc, self.span)),
                }),
            }),
        };

        attr.to_tokens(tokens);
    }
}

#[derive(Copy, Clone)]
enum PathStyle {
    Default,
    Absolute,
}

fn make_type(s: impl AsRef<str>) -> syn::Type {
    let s = s.as_ref();
    let path = type_path(s);
    syn::Type::Path(syn::TypePath { qself: None, path })
}

fn type_path(s: impl AsRef<str>) -> syn::Path {
    let s = s.as_ref();
    let (s, style) = if let Some(s) = s.strip_prefix("::") {
        (s, PathStyle::Absolute)
    } else {
        (s, PathStyle::Default)
    };
    let parts = s.split("::");
    make_path(parts, style)
}

fn attr_path(s: impl AsRef<str>) -> syn::Path {
    make_path([s.as_ref()], PathStyle::Default)
}

fn make_path<'a>(parts: impl IntoIterator<Item = &'a str>, style: PathStyle) -> syn::Path {
    use proc_macro2::Span;

    syn::Path {
        leading_colon: match style {
            PathStyle::Default => None,
            PathStyle::Absolute => Some(syn::token::PathSep(Span::call_site())),
        },
        segments: syn::punctuated::Punctuated::from_iter(parts.into_iter().map(|part| {
            syn::PathSegment {
                ident: format_ident!("{}", part),
                arguments: syn::PathArguments::None,
            }
        })),
    }
}

#[allow(unused)]
#[derive(Debug, Clone)]
pub struct PropertyInfo {
    pub name: Ident,
    pub ty: syn::Type,
    pub symbol: Option<SymbolType>,
    pub is_optional: bool,
    pub hide: bool,
}

#[allow(unused)]
#[derive(Debug, Clone)]
pub struct UserdataFieldInfo {
    pub name: Ident,
    pub ty: syn::Type,
}

#[derive(Debug, Clone)]
pub struct OperandGroupInfo {
    #[allow(unused)]
    pub field_name: Ident,
    /// The name of the operand group, if applicable.
    ///
    /// This is only set when the group is derived from a `#[operands]` or `#[successor]` field
    pub name: Option<Ident>,
    /// The type constraint of the operand group, if applicable.
    ///
    /// This is only set when the group is derived from a `#[operands]` field
    #[allow(unused)]
    pub ty: Option<syn::Type>,
    /// The index of this group
    pub index: usize,
    /// This group has an exact size
    pub size: Option<NonZeroU8>,
    /// This group must contain at least `min_size` operands
    ///
    /// If `size` is provided, `min_size` is always the same value
    pub min_size: u8,
    /// If this operand group requires surrounding `(` `)` delimiters
    pub requires_delimiter: bool,
    /// Whether this group holds successor operands
    pub successor_operands: bool,
    /// If this group represents one or more #[operand] fields, then `size` will be set, and this
    /// vector will contain `size` operands
    pub operands: Vec<OperandInfo>,
}

#[derive(Debug, Clone)]
pub struct OperandInfo {
    /// The name of the operand field
    #[allow(unused)]
    pub name: Ident,
    /// The index of this operand relative to its containing group
    pub index: usize,
    /// The type of the field, representing the type constraint associated with this result
    #[allow(unused)]
    pub ty: syn::Type,
}

#[derive(Debug, Clone)]
pub struct SuccessorGroupInfo {
    pub field_name: Ident,
    /// The name of the successor group, if applicable.
    ///
    /// This is only set when the group is derived from a `#[successors]` field
    pub name: Option<Ident>,
    /// The starting successor index of this group
    pub index: usize,
    /// The starting operand group index for successors in this group
    pub base_operand_group: usize,
    /// If this successor group requires surrounding `[` `]` delimiters
    pub requires_delimiter: bool,
    /// The successors in this group
    pub successors: Vec<SuccessorInfo>,
    /// Whether this group was decorated with `#[successors(keyed)]`, and if so, the field type
    pub keyed: Option<syn::Type>,
}

#[derive(Debug, Clone)]
pub struct SuccessorInfo {
    /// The field decorated with #[successor]
    #[allow(unused)]
    pub field: Ident,
    /// The concrete type of the field specified in the Rust source code
    #[allow(unused)]
    pub field_ty: syn::Type,
    /// The index of this successor relative to its group
    #[allow(unused)]
    pub index: usize,
}

#[allow(unused)]
#[derive(Debug, Clone)]
pub struct ResultsInfo {
    /// The name of the result group, if applicable.
    ///
    /// This is only set when the group is derived from a `#[results]` field
    pub field: Option<Ident>,
    /// This group has an exact size
    pub size: Option<NonZeroU8>,
    /// This group must contain at least `min_size` operands
    ///
    /// If `size` is provided, `min_size` is always the same value
    pub min_size: u8,
    /// If this result group requires surrounding `(` `)` delimiters
    pub requires_delimiter: bool,
    /// If this group represents one or more #[result] fields, then `size` will be set, and this
    /// vector will contain `size` results
    pub results: Vec<ResultInfo>,
}

#[derive(Debug, Clone)]
pub struct ResultInfo {
    /// The name of the operand field
    #[allow(unused)]
    pub name: Ident,
    /// The type of the field, representing the type constraint associated with this result
    #[allow(unused)]
    pub ty: syn::Type,
}

#[derive(Default, Debug, Clone)]
pub enum ParamShape {
    #[default]
    None,
    /// A statically known shape, given by the types of all the operands of the given operand group
    #[allow(unused)]
    Static(usize),
    /// A shape consisting of a fixed portion and a trailing variable-length group
    #[allow(unused)]
    TrailingVarArgs {
        /// The operand group of the fixed component
        fixed: usize,
        /// The operand group of the varargs component
        varargs: usize,
    },
    /// A shape that is determined entirely dynamically, corresponding to the given operand group
    #[allow(unused)]
    Dynamic(usize),
}

#[derive(Default, Debug, Clone)]
#[allow(clippy::large_enum_variant)]
pub enum ResultShape {
    #[default]
    None,
    /// One or more statically known results
    Static(Vec<ResultInfo>),
    /// A shape that is determined entirely dynamically
    #[allow(unused)]
    Dynamic(ResultInfo),
}

#[derive(Default, Debug, Clone)]
pub struct OpSignature {
    /// The inputs to this operation
    pub params: ParamShape,
    /// The results of this operation, if applicable
    pub results: ResultShape,
    /// Whether or not the operation implements InferTypeOpInterface OR all type constraints
    /// are concrete, so we are able to infer the type statically
    pub can_infer: bool,
}

#[derive(Debug, Clone)]
pub struct RegionInfo {
    pub name: Ident,
    pub options: RegionOptions,
}

#[derive(Default, Debug)]
pub struct OperationFormat {
    pub signature: OpSignature,
    pub properties: Vec<PropertyInfo>,
    #[allow(unused)]
    pub fields: Vec<UserdataFieldInfo>,
    pub operand_groups: Vec<OperandGroupInfo>,
    pub successor_groups: Vec<SuccessorGroupInfo>,
    pub regions: Vec<RegionInfo>,
    pub isolated_from_above: bool,
}

impl OperationFormat {
    pub fn from_struct(
        _ident: &Ident,
        fields: &[&OperationField],
        traits: &darling::util::PathList,
        implements: &darling::util::PathList,
    ) -> darling::Result<Self> {
        let mut format = Self::default();

        if traits.iter().any(|p| p.segments.last().unwrap().ident == "IsolatedFromAbove") {
            format.isolated_from_above = true;
        }
        if implements
            .iter()
            .any(|p| p.segments.last().unwrap().ident == "InferTypeOpInterface")
        {
            format.signature.can_infer = true;
        }

        for f in fields.iter() {
            let field_name = f.ident.clone().unwrap();
            if let Some(kind) = f.attrs.attr.as_deref().cloned() {
                let hide = matches!(kind, Some(AttrKind::Hidden));
                let is_optional = f.attrs.default.is_present();
                format.properties.push(PropertyInfo {
                    name: f.ident.clone().unwrap(),
                    ty: f.ty.clone(),
                    symbol: None,
                    is_optional,
                    hide,
                });
            } else if let Some(symbol) = f.attrs.symbol.as_deref().cloned() {
                let is_optional = f.attrs.default.is_present();
                format.properties.push(PropertyInfo {
                    name: f.ident.clone().unwrap(),
                    ty: f.ty.clone(),
                    symbol,
                    is_optional,
                    hide: false,
                });
            } else if f.attrs.operand.is_present() {
                let mut param_operand_group_index = 0;
                if let Some(prev_group) = format.operand_groups.last()
                    && prev_group.name.is_some()
                {
                    assert!(prev_group.successor_operands);
                    let index = prev_group.index + prev_group.min_size as usize;
                    param_operand_group_index = index;
                    let requires_delimiter =
                        !prev_group.requires_delimiter && prev_group.size.is_none();
                    let name = field_name.clone();
                    format.operand_groups.push(OperandGroupInfo {
                        field_name,
                        name: None,
                        ty: None,
                        index,
                        size: None,
                        min_size: 1,
                        requires_delimiter,
                        successor_operands: false,
                        operands: vec![OperandInfo {
                            name,
                            index,
                            ty: f.ty.clone(),
                        }],
                    });
                } else if let Some(group) = format.operand_groups.last_mut() {
                    param_operand_group_index = group.index;
                    let index = group.operands.last().unwrap().index + 1;
                    {
                        let size = group.size.as_mut().unwrap();
                        *size = size.checked_add(1).unwrap();
                    }
                    group.min_size += 1;
                    group.operands.push(OperandInfo {
                        name: f.ident.clone().unwrap(),
                        index,
                        ty: f.ty.clone(),
                    });
                } else {
                    let name = field_name.clone();
                    format.operand_groups.push(OperandGroupInfo {
                        field_name,
                        name: None,
                        ty: None,
                        index: 0,
                        size: NonZeroU8::new(1),
                        min_size: 1,
                        requires_delimiter: false,
                        successor_operands: false,
                        operands: vec![OperandInfo {
                            name,
                            index: 0,
                            ty: f.ty.clone(),
                        }],
                    });
                }
                match &mut format.signature.params {
                    params @ ParamShape::None => {
                        *params = ParamShape::Static(param_operand_group_index);
                    }
                    ParamShape::Static(group) => {
                        assert_eq!(*group, param_operand_group_index);
                    }
                    ParamShape::Dynamic(_) | ParamShape::TrailingVarArgs { .. } => {
                        panic!("cannot have #[operand] fields after an #[operands] field")
                    }
                }
            } else if f.attrs.operands.is_present() {
                let name = Some(field_name.clone());
                let mut param_operand_group_index = 0;
                if let Some(prev_group) = format.operand_groups.last() {
                    assert!(prev_group.successor_operands || !prev_group.operands.is_empty());
                    let index = prev_group.index + prev_group.min_size as usize;
                    param_operand_group_index = index;
                    let requires_delimiter =
                        !prev_group.requires_delimiter && prev_group.size.is_none();
                    format.operand_groups.push(OperandGroupInfo {
                        field_name,
                        name,
                        ty: Some(f.ty.clone()),
                        index,
                        size: None,
                        min_size: 0,
                        requires_delimiter,
                        successor_operands: false,
                        operands: vec![],
                    });
                } else {
                    format.operand_groups.push(OperandGroupInfo {
                        field_name,
                        name,
                        ty: Some(f.ty.clone()),
                        index: 0,
                        size: None,
                        min_size: 0,
                        requires_delimiter: false,
                        successor_operands: false,
                        operands: vec![],
                    });
                }
                match &mut format.signature.params {
                    params @ ParamShape::None => {
                        *params = ParamShape::Dynamic(param_operand_group_index);
                    }
                    ParamShape::Static(group) => {
                        let group = *group;
                        format.signature.params = ParamShape::TrailingVarArgs {
                            fixed: group,
                            varargs: param_operand_group_index,
                        };
                    }
                    ParamShape::Dynamic(_) | ParamShape::TrailingVarArgs { .. } => {
                        panic!("cannot have multiple #[operands] fields in a struct")
                    }
                }
            } else if f.attrs.successor.is_present() {
                let operand_group_index = format.operand_groups.len();
                let starting_operand_index = format
                    .operand_groups
                    .last()
                    .map(|group| group.index + group.min_size as usize)
                    .unwrap_or(0);
                let name = Some(field_name.clone());
                format.operand_groups.push(OperandGroupInfo {
                    field_name: field_name.clone(),
                    name,
                    ty: None,
                    index: starting_operand_index,
                    size: None,
                    min_size: 0,
                    requires_delimiter: true,
                    successor_operands: true,
                    operands: vec![],
                });
                if let Some(prev_group) = format.successor_groups.last()
                    && prev_group.name.is_some()
                {
                    let index = prev_group.index + prev_group.successors.len();
                    let requires_delimiter =
                        !prev_group.requires_delimiter && prev_group.successors.is_empty();
                    let field = field_name.clone();
                    format.successor_groups.push(SuccessorGroupInfo {
                        field_name,
                        name: None,
                        index,
                        requires_delimiter,
                        base_operand_group: operand_group_index,
                        keyed: None,
                        successors: vec![SuccessorInfo {
                            field,
                            field_ty: f.ty.clone(),
                            index: 0,
                        }],
                    });
                } else if let Some(group) = format.successor_groups.last_mut() {
                    let index = group.index + group.successors.len();
                    group.successors.push(SuccessorInfo {
                        field: field_name,
                        field_ty: f.ty.clone(),
                        index,
                    });
                } else {
                    let field = field_name.clone();
                    format.successor_groups.push(SuccessorGroupInfo {
                        field_name,
                        name: None,
                        index: 0,
                        base_operand_group: operand_group_index,
                        requires_delimiter: false,
                        keyed: None,
                        successors: vec![SuccessorInfo {
                            field,
                            field_ty: f.ty.clone(),
                            index: 0,
                        }],
                    });
                }
            } else if let Some(ty) = f.attrs.successors.as_deref() {
                let keyed = if matches!(ty, SuccessorsType::Keyed) {
                    Some(f.ty.clone())
                } else {
                    None
                };
                let name = Some(field_name.clone());
                let operand_group_index = format.operand_groups.len();
                let starting_operand_index = format
                    .operand_groups
                    .last()
                    .map(|group| group.index + group.min_size as usize)
                    .unwrap_or(0);
                let index = format
                    .successor_groups
                    .last()
                    .map(|prev_group| prev_group.index + prev_group.successors.len())
                    .unwrap_or(0);
                format.operand_groups.push(OperandGroupInfo {
                    field_name: field_name.clone(),
                    name: None,
                    ty: None,
                    index: starting_operand_index,
                    size: None,
                    min_size: 0,
                    requires_delimiter: true,
                    successor_operands: true,
                    operands: vec![],
                });
                format.successor_groups.push(SuccessorGroupInfo {
                    field_name,
                    name,
                    index,
                    base_operand_group: operand_group_index,
                    requires_delimiter: false,
                    keyed,
                    successors: vec![],
                });
            } else if let Some(region) = f.attrs.region.as_ref() {
                format.regions.push(RegionInfo {
                    name: field_name,
                    options: region.clone().into_inner().unwrap_or_default(),
                });
            } else if f.attrs.result.is_present() {
                match &mut format.signature.results {
                    result @ ResultShape::None => {
                        *result = ResultShape::Static(vec![ResultInfo {
                            name: field_name,
                            ty: f.ty.clone(),
                        }]);
                    }
                    ResultShape::Static(results) => {
                        results.push(ResultInfo {
                            name: field_name,
                            ty: f.ty.clone(),
                        });
                    }
                    ResultShape::Dynamic(_) => panic!("#[result] and #[results] cannot be mixed"),
                }
            } else if f.attrs.results.is_present() {
                match &mut format.signature.results {
                    result @ ResultShape::None => {
                        *result = ResultShape::Dynamic(ResultInfo {
                            name: field_name,
                            ty: f.ty.clone(),
                        });
                    }
                    ResultShape::Static(_) => panic!("#[result] and #[results] cannot be mixed"),
                    ResultShape::Dynamic(_) => {
                        panic!("#[results] cannot appear twice in the same struct")
                    }
                }
            }
        }

        Ok(format)
    }
}
