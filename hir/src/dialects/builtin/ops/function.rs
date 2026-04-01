use alloc::format;
use core::str::FromStr;

use crate::{
    AsValueRange, BlockRef, CallConv, CallableOpInterface, CallableSymbol, EntityRef, IdentAttr,
    Immediate, ImmediateAttr, Op, OpParser, OpPrinter, Operation, RegionKind, RegionKindInterface,
    RegionRef, SmallVec, Symbol, SymbolUse, SymbolUseList, Type, UnsafeIntrusiveEntityRef, Usable,
    Visibility,
    derive::operation,
    dialects::builtin::{
        BuiltinDialect,
        attributes::{LocalVariable, Signature, SignatureAttr, TypeArrayAttr, VisibilityAttr},
    },
    interner,
    parse::ParserExt,
    print::AsmPrinter,
    traits::{AnyType, IsolatedFromAbove, ReturnLike, SingleRegion, Terminator},
};

trait UsableSymbol = Usable<Use = SymbolUse>;

pub type FunctionRef = UnsafeIntrusiveEntityRef<Function>;

#[operation(
    dialect = BuiltinDialect,
    traits(SingleRegion, IsolatedFromAbove),
    implements(
        UsableSymbol,
        Symbol,
        CallableOpInterface,
        CallableSymbol,
        RegionKindInterface,
        OpPrinter
    )
)]
pub struct Function {
    #[attr]
    name: IdentAttr,
    #[attr]
    linkage: VisibilityAttr,
    #[attr]
    signature: SignatureAttr,
    #[region]
    body: RegionRef,
    /// The set of local variables allocated within this function
    #[attr]
    #[default]
    locals: TypeArrayAttr,
    /// The uses of this function as a symbol
    #[default]
    uses: SymbolUseList,
}

impl OpParser for Function {
    fn parse(
        state: &mut crate::OperationState,
        parser: &mut dyn crate::OpAsmParser<'_>,
    ) -> crate::ParseResult {
        use alloc::string::ToString;

        use crate::parse::{Delimiter, ParserError, Token};

        let visibility = match parser.parse_optional_keyword()? {
            Some(tok) => match tok.inner() {
                Token::BareIdent("public") => Visibility::Public,
                Token::BareIdent("private") => Visibility::Private,
                Token::BareIdent("internal") => Visibility::Internal,
                invalid => {
                    return Err(ParserError::UnexpectedToken {
                        span: tok.span(),
                        token: invalid.to_string(),
                        expected: Some("visibility keyword".to_string()),
                    });
                }
            },
            None => Visibility::Private,
        };

        let extern_keyword = parser.parse_optional_keyword()?;
        match extern_keyword.map(|tok| tok.into_parts()) {
            None => (),
            Some((_, Token::BareIdent("extern"))) => (),
            Some((span, invalid)) => {
                return Err(ParserError::UnexpectedToken {
                    span,
                    token: invalid.to_string(),
                    expected: Some("extern keyword".to_string()),
                });
            }
        }

        parser.parse_lparen()?;
        let cc_string = parser.parse_string()?;
        let cc = CallConv::from_str(cc_string.as_str()).map_err(|_| {
            let (span, cc_string) = cc_string.into_parts();
            ParserError::UnexpectedToken {
                span,
                token: cc_string.into_string(),
                expected: Some("calling convention string".to_string()),
            }
        })?;
        parser.parse_rparen()?;

        let name = parser.parse_symbol_name()?;

        let mut args = SmallVec::new_const();
        parser.parse_argument_list(Delimiter::Paren, true, true, &mut args)?;

        let mut results = SmallVec::new_const();
        parser.parse_optional_arrow_type_list(&mut results)?;

        let sig = Signature::with_convention(
            &parser.context_rc(),
            cc,
            args.iter().map(|arg| arg.ty.clone()),
            results,
        );

        let name = parser.context_rc().create_attribute::<IdentAttr, _>(name);
        state.add_attribute("name", name);

        let visibility = parser.context_rc().create_attribute::<VisibilityAttr, _>(visibility);
        state.add_attribute("linkage", visibility);

        let ty = parser.context_rc().create_attribute::<SignatureAttr, _>(sig);
        state.add_attribute("signature", ty);

        let locals = parser.context_rc().create_attribute::<TypeArrayAttr, _>([]);
        state.add_attribute("locals", locals);

        if let Some(body) = parser.parse_optional_region(&args, false)? {
            state.add_region(body);
        }
        Ok(())
    }
}

impl OpPrinter for Function {
    fn print(&self, printer: &mut AsmPrinter<'_>) {
        use alloc::borrow::Cow;
        let sig = self.get_signature();

        printer.print_space();
        printer.print_keyword(self.get_linkage().as_str());
        printer.print_space();
        printer.print_keyword("extern");
        printer.print_lparen();
        printer.print_string(sig.calling_convention().as_str());
        printer.print_rparen();
        printer.print_space();

        printer.print_symbol_name(self.get_name().as_symbol());
        if self.is_declaration() {
            printer.print_function_type_parts(
                sig.params().iter().map(|p| &p.ty),
                sig.results().iter().map(|p| &p.ty),
            );
        } else {
            let body = self.body();
            let entry = body.entry();
            printer.print_value_id_and_type_list(entry.argument_values());
            if !sig.results().is_empty() {
                printer.print_space();
                printer.print_arrow_type_list(
                    /*elide_single_type_parens=*/ true,
                    sig.results().iter().map(|p| Cow::Borrowed(&p.ty)),
                );
            }
            printer.print_space();
            printer.print_region(&body);
        }
    }
}

/// Builders
impl Function {
    /// Conver this function from a declaration (no body) to a definition (has a body) by creating
    /// the entry block based on the function signature.
    ///
    /// NOTE: The resulting function is _invalid_ until the block has a terminator inserted into it.
    ///
    /// This function will panic if an entry block has already been created
    pub fn create_entry_block(&mut self) -> BlockRef {
        assert!(self.body().is_empty(), "entry block already exists");
        let signature = self.get_signature();
        let block = self
            .as_operation()
            .context_rc()
            .create_block_with_params(signature.params().iter().map(|p| p.ty.clone()));
        drop(signature);
        let mut body = self.body_mut();
        body.push_back(block);
        block
    }
}

/// Accessors
impl Function {
    #[inline]
    pub fn entry_block(&self) -> BlockRef {
        self.body()
            .body()
            .front()
            .as_pointer()
            .expect("cannot get entry block for declaration")
    }

    pub fn last_block(&self) -> BlockRef {
        self.body()
            .body()
            .back()
            .as_pointer()
            .expect("cannot access blocks of a function declaration")
    }

    pub fn num_locals(&self) -> usize {
        self.locals().len()
    }

    pub fn get_local(&self, id: &LocalVariable) -> EntityRef<'_, Type> {
        assert_eq!(
            self.as_operation_ref(),
            id.function().as_operation_ref(),
            "attempted to use local variable reference from different function"
        );
        EntityRef::map(self.get_locals(), |locals| &locals[id.as_usize()])
    }

    pub fn alloc_local(&mut self, ty: Type) -> LocalVariable {
        let mut locals = self.get_locals_mut();
        let id = locals.len();
        locals.push(ty);
        drop(locals);
        LocalVariable::new(self.as_function_ref(), id)
    }

    pub fn iter_locals(&self) -> impl ExactSizeIterator<Item = LocalVariable> {
        let fun = self.as_function_ref();
        (0..self.locals().len()).map(move |i| LocalVariable::new(fun, i))
    }

    #[inline(always)]
    pub fn as_function_ref(&self) -> FunctionRef {
        unsafe { FunctionRef::from_raw(self) }
    }

    #[inline(always)]
    pub const fn signature_ref(&self) -> UnsafeIntrusiveEntityRef<SignatureAttr> {
        self.signature
    }
}

impl RegionKindInterface for Function {
    #[inline(always)]
    fn kind(&self) -> RegionKind {
        RegionKind::SSA
    }
}

impl Usable for Function {
    type Use = SymbolUse;

    #[inline(always)]
    fn uses(&self) -> &SymbolUseList {
        &self.uses
    }

    #[inline(always)]
    fn uses_mut(&mut self) -> &mut SymbolUseList {
        &mut self.uses
    }
}

impl Symbol for Function {
    #[inline(always)]
    fn as_symbol_operation(&self) -> &Operation {
        &self.op
    }

    #[inline(always)]
    fn as_symbol_operation_mut(&mut self) -> &mut Operation {
        &mut self.op
    }

    fn name(&self) -> interner::Symbol {
        self.get_name().as_symbol()
    }

    fn set_name(&mut self, name: interner::Symbol) {
        self.get_name_mut().name = name;
    }

    fn visibility(&self) -> Visibility {
        *self.get_linkage()
    }

    fn set_visibility(&mut self, visibility: Visibility) {
        *self.get_linkage_mut() = visibility;
    }

    /// Returns true if this operation is a declaration, rather than a definition, of a symbol
    ///
    /// The default implementation assumes that all operations are definitions
    #[inline]
    fn is_declaration(&self) -> bool {
        self.body().is_empty()
    }
}

impl CallableOpInterface for Function {
    fn get_callable_region(&self) -> Option<RegionRef> {
        if self.is_declaration() {
            None
        } else {
            self.op.regions().front().as_pointer()
        }
    }

    fn signature(&self) -> Signature {
        self.get_signature().clone()
    }
}

/// Returns from the enclosing function with the provided operands as its results.
#[operation(
    dialect = BuiltinDialect,
    traits(Terminator, ReturnLike),
    implements(OpPrinter)
)]
pub struct Ret {
    #[operands]
    values: AnyType,
}

impl OpPrinter for Ret {
    fn print(&self, printer: &mut AsmPrinter<'_>) {
        use alloc::borrow::Cow;

        let returning = self.values();
        if returning.is_empty() {
            return;
        }

        printer.print_space();
        printer.print_value_uses(returning.as_value_range());
        printer.print_space();
        printer.print_colon_type_list(returning.iter().map(|r| Cow::Owned(r.borrow().ty())));
    }
}

impl OpParser for Ret {
    fn parse(
        state: &mut crate::OperationState,
        parser: &mut dyn crate::OpAsmParser<'_>,
    ) -> crate::ParseResult {
        use crate::{
            diagnostics::SourceSpan,
            parse::{Delimiter, Token},
        };

        if !parser.token_stream_mut().is_next(|tok| matches!(tok, Token::PercentIdent(_))) {
            return Ok(());
        }

        let start = parser.token_stream().current_position();
        let mut args = SmallVec::default();
        parser.parse_operand_list(
            &mut args,
            Delimiter::None,
            /*allow_result_number=*/ true,
            None,
        )?;

        let mut types = SmallVec::default();
        parser.parse_colon_type_list(&mut types)?;

        let end = parser.token_stream().current_span();
        let span = SourceSpan::new(end.source_id(), start..end.end());

        let mut operands = SmallVec::default();
        parser.resolve_operands(span, &args, &types, &mut operands)?;

        state.operands.push(operands);

        Ok(())
    }
}

/// Returns from the enclosing function with the provided immediate value as its result.
#[operation(
    dialect = BuiltinDialect,
    traits(Terminator, ReturnLike),
    implements(OpPrinter)
)]
pub struct RetImm {
    #[attr(hidden)]
    value: ImmediateAttr,
}

impl OpPrinter for RetImm {
    fn print(&self, printer: &mut AsmPrinter<'_>) {
        printer.print_space();
        printer.print_attribute_value(&*self.value());
    }
}

impl OpParser for RetImm {
    fn parse(
        state: &mut crate::OperationState,
        parser: &mut dyn crate::OpAsmParser<'_>,
    ) -> crate::ParseResult {
        use crate::{diagnostics::SourceSpan, parse::ParserError};

        let start = parser.token_stream().current_position();
        let imm = parser.parse_integer::<Immediate>()?;
        let ty = parser.parse_colon_type()?;
        let end = parser.token_stream().current_span();
        let span = SourceSpan::new(end.source_id(), start..end.end());

        if ty.is_numeric() {
            let attr = parser
                .context_rc()
                .create_attribute_with_type::<ImmediateAttr, _>(imm.into_inner(), ty.into_inner());
            state.add_attribute("value", attr);
        } else {
            return Err(ParserError::InvalidOperationType {
                span,
                ty_span: ty.span(),
                reason: format!("expected numeric type, got {ty}"),
            });
        }

        Ok(())
    }
}
