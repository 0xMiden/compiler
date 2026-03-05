use crate::{
    AsSymbolRef, Context, IdentAttr, OpParser, OpPrinter, Operation, PointerType, Report, Spanned,
    Symbol, SymbolName, SymbolRef, SymbolUseList, Type, UnsafeIntrusiveEntityRef, Usable, Value,
    Visibility,
    derive::{EffectOpInterface, operation},
    dialects::builtin::{
        BuiltinDialect,
        attributes::{I32Attr, TypeAttr, VisibilityAttr},
    },
    effects::{AlwaysSpeculatable, ConditionallySpeculatable, MemoryEffectOpInterface, Pure},
    parse::ParserExt,
    print::AsmPrinter,
    traits::{
        InferTypeOpInterface, IsolatedFromAbove, NoRegionArguments, PointerOf, SingleBlock,
        SingleRegion, UInt8,
    },
};

pub type GlobalVariableRef = UnsafeIntrusiveEntityRef<GlobalVariable>;

/// A [GlobalVariable] represents a named, typed, location in memory.
///
/// Global variables may also specify an initializer, but if not provided, the underlying bytes
/// will be zeroed, which may or may not be a valid instance of the type. It is up to frontends
/// to ensure that an initializer is specified if necessary.
///
/// Global variables, like functions, may also be assigned a visibility. This is only used when
/// resolving symbol uses, and does not impose any access restrictions once lowered to Miden
/// Assembly.
#[operation(
    dialect = BuiltinDialect,
    traits(
        SingleRegion,
        SingleBlock,
        NoRegionArguments,
        IsolatedFromAbove,
    ),
    implements(Symbol, OpPrinter)
)]
pub struct GlobalVariable {
    #[attr]
    name: IdentAttr,
    #[attr]
    visibility: VisibilityAttr,
    #[attr]
    ty: TypeAttr,
    #[region]
    initializer: RegionRef,
    #[default]
    uses: SymbolUseList,
}

impl GlobalVariable {
    #[inline(always)]
    pub fn as_global_var_ref(&self) -> GlobalVariableRef {
        unsafe { GlobalVariableRef::from_raw(self) }
    }
}

impl Usable for GlobalVariable {
    type Use = crate::SymbolUse;

    #[inline(always)]
    fn uses(&self) -> &SymbolUseList {
        &self.uses
    }

    #[inline(always)]
    fn uses_mut(&mut self) -> &mut SymbolUseList {
        &mut self.uses
    }
}

impl Symbol for GlobalVariable {
    #[inline(always)]
    fn as_symbol_operation(&self) -> &Operation {
        &self.op
    }

    #[inline(always)]
    fn as_symbol_operation_mut(&mut self) -> &mut Operation {
        &mut self.op
    }

    fn name(&self) -> SymbolName {
        GlobalVariable::name(self).as_symbol()
    }

    fn set_name(&mut self, name: SymbolName) {
        GlobalVariable::set_name(self, name)
    }

    fn visibility(&self) -> Visibility {
        *self.get_visibility()
    }

    fn set_visibility(&mut self, visibility: Visibility) {
        GlobalVariable::set_visibility(self, visibility);
    }

    /// Returns true if this operation is a declaration, rather than a definition, of a symbol
    ///
    /// The default implementation assumes that all operations are definitions
    #[inline]
    fn is_declaration(&self) -> bool {
        self.initializer().is_empty()
    }
}

impl AsSymbolRef for GlobalVariable {
    fn as_symbol_ref(&self) -> SymbolRef {
        unsafe { SymbolRef::from_raw(self as &dyn Symbol) }
    }
}

impl OpPrinter for GlobalVariable {
    fn print(&self, printer: &mut AsmPrinter<'_>) {
        use crate::formatter::*;

        printer.print_space();
        printer.print_keyword(self.get_visibility().as_str());
        printer.print_space();
        printer.print_symbol_name(self.get_name().as_symbol());
        *printer += const_text(" : ");
        printer.print_type(&self.get_ty());
        if self.is_declaration() {
            return;
        }
        printer.print_space();
        printer.print_region(&self.initializer());
    }
}

impl OpParser for GlobalVariable {
    fn parse(
        state: &mut crate::OperationState,
        parser: &mut dyn crate::OpAsmParser<'_>,
    ) -> crate::ParseResult {
        use crate::parse::Token;

        let visibility = parser
            .parse_keyword_from(&[
                Token::BareIdent("public"),
                Token::BareIdent("private"),
                Token::BareIdent("internal"),
            ])?
            .into_inner()
            .parse::<Visibility>()
            .unwrap();
        state.add_attribute(
            "visibility",
            parser.context_rc().create_attribute::<VisibilityAttr, _>(visibility),
        );

        let name = parser.parse_symbol_name()?;
        state.add_attribute("name", parser.context_rc().create_attribute::<IdentAttr, _>(name));

        let ty = parser.parse_colon_type()?;
        state.add_attribute(
            "name",
            parser.context_rc().create_attribute::<TypeAttr, _>(ty.into_inner()),
        );

        let initializer =
            parser.parse_optional_region(&[], /*enable_name_shadowing=*/ false)?;
        // We always add the initializer region, even if empty
        state
            .regions
            .push(initializer.unwrap_or_else(|| parser.context().create_region()));

        Ok(())
    }
}

/// A [GlobalSymbol] reifies the address of a [GlobalVariable] as a value.
///
/// An optional signed offset value may also be provided, which will be applied by the operation
/// internally.
///
/// The result type is always a pointer, whose pointee type is derived from the referenced symbol.
#[derive(EffectOpInterface)]
#[operation(
    dialect = BuiltinDialect,
    traits(Pure, AlwaysSpeculatable),
    implements(InferTypeOpInterface, OpPrinter, ConditionallySpeculatable, MemoryEffectOpInterface)
)]
pub struct GlobalSymbol {
    /// The name of the global variable that is referenced
    #[symbol]
    symbol: GlobalVariable,
    /// A constant offset, in bytes, from the address of the symbol
    #[attr]
    #[default]
    offset: I32Attr,
    #[result]
    addr: PointerOf<UInt8>,
}

impl OpPrinter for GlobalSymbol {
    fn print(&self, printer: &mut AsmPrinter<'_>) {
        use crate::formatter::*;

        printer.print_space();
        printer.print_symbol_path(self.get_symbol().path());
        let offset = *self.get_offset();
        match offset {
            0 => (),
            n if n > 0 => {
                *printer += const_text("+") + display(n);
            }
            n => *printer += display(n),
        };

        *printer += const_text(" : ");
        printer.print_type(self.addr().ty());
    }
}

impl OpParser for GlobalSymbol {
    fn parse(
        state: &mut crate::OperationState,
        parser: &mut dyn crate::OpAsmParser<'_>,
    ) -> crate::ParseResult {
        use crate::parse::Token;

        let symbol = parser.parse_symbol_ref()?.into_inner();
        state.add_attribute("symbol", symbol);

        let offset = if parser.token_stream_mut().next_if_eq(Token::Plus)? {
            parser.parse_decimal_integer::<i32>()?.into_inner()
        } else {
            parser
                .parse_optional_decimal_integer::<i32>()?
                .map(|spanned| spanned.into_inner())
                .unwrap_or(0)
        };
        let offset = parser.context_rc().create_attribute::<I32Attr, _>(offset);
        state.add_attribute("offset", offset);

        state.results.push(Type::Ptr(PointerType::new(Type::U8).into()));

        Ok(())
    }
}

impl ConditionallySpeculatable for GlobalSymbol {
    fn speculatability(&self) -> crate::effects::Speculatability {
        crate::effects::Speculatability::Speculatable
    }
}

impl InferTypeOpInterface for GlobalSymbol {
    fn infer_return_types(&mut self, _context: &Context) -> Result<(), Report> {
        self.addr_mut().set_type(Type::from(PointerType::new(Type::U8)));
        Ok(())
    }
}
