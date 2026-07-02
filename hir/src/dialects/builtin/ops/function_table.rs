use crate::{
    AsSymbolRef, NamedAttribute, OpParser, OpPrinter, Operation, RegionKind, RegionKindInterface,
    Symbol, SymbolName, SymbolRef, SymbolUseList, UnsafeIntrusiveEntityRef, Usable, Visibility,
    derive::operation,
    dialects::builtin::{
        BuiltinDialect,
        attributes::{IdentAttr, U32Attr, VisibilityAttr},
    },
    parse::ParserExt,
    print::AsmPrinter,
    traits::{
        GraphRegionNoTerminator, HasOnlyGraphRegion, IsolatedFromAbove, NoRegionArguments,
        NoTerminator, SingleBlock, SingleRegion,
    },
};

pub type FunctionTableRef = UnsafeIntrusiveEntityRef<FunctionTable>;

/// A [FunctionTable] declares a function-reference table in the shared memory of a
/// [super::Component]; the Wasm frontend lowers `funcref` tables to it.
///
/// The table occupies one word (4 field elements, 16 bytes) of linear memory per slot, holding
/// the MAST root digest of the referenced function; an all-zero word denotes a null entry. The
/// base address of the table is assigned by the linker (word-aligned), and initialized slots are
/// filled at program startup by the component `init` procedure.
///
/// The `entries` region holds one [FunctionTableEntry] per initialized slot, in application
/// order (later entries overwrite earlier ones at the same index).
#[operation(
    dialect = BuiltinDialect,
    traits(
        SingleRegion,
        SingleBlock,
        NoRegionArguments,
        NoTerminator,
        HasOnlyGraphRegion,
        GraphRegionNoTerminator,
        IsolatedFromAbove,
    ),
    implements(RegionKindInterface, Symbol, OpPrinter)
)]
pub struct FunctionTable {
    #[attr]
    name: IdentAttr,
    #[attr]
    visibility: VisibilityAttr,
    /// The number of slots in the table
    #[attr]
    num_slots: U32Attr,
    #[region]
    entries: RegionRef,
    #[default]
    uses: SymbolUseList,
}

impl FunctionTable {
    #[inline(always)]
    pub fn as_function_table_ref(&self) -> FunctionTableRef {
        unsafe { FunctionTableRef::from_raw(self) }
    }
}

impl RegionKindInterface for FunctionTable {
    #[inline(always)]
    fn kind(&self) -> RegionKind {
        RegionKind::Graph
    }
}

impl Usable for FunctionTable {
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

impl Symbol for FunctionTable {
    #[inline(always)]
    fn as_symbol_operation(&self) -> &Operation {
        &self.op
    }

    #[inline(always)]
    fn as_symbol_operation_mut(&mut self) -> &mut Operation {
        &mut self.op
    }

    fn name(&self) -> SymbolName {
        FunctionTable::name(self).as_symbol()
    }

    fn set_name(&mut self, name: SymbolName) {
        FunctionTable::set_name(self, name)
    }

    fn visibility(&self) -> Visibility {
        *self.get_visibility()
    }

    fn set_visibility(&mut self, visibility: Visibility) {
        FunctionTable::set_visibility(self, visibility);
    }
}

impl AsSymbolRef for FunctionTable {
    fn as_symbol_ref(&self) -> SymbolRef {
        self.as_symbol_operation()
            .as_symbol_ref()
            .expect("function tables must provide a symbol operation")
    }
}

impl OpPrinter for FunctionTable {
    fn print(&self, printer: &mut AsmPrinter<'_>) {
        use crate::formatter::*;

        printer.print_space();
        printer.print_keyword(self.get_visibility().as_str());
        printer.print_space();
        printer.print_symbol_name(self.get_name().as_symbol());
        *printer += const_text(" : ");
        *printer += display(*self.get_num_slots());
        printer.print_space();
        printer.print_region(&self.entries());
    }
}

impl OpParser for FunctionTable {
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

        parser.parse_colon()?;
        let num_slots = parser.parse_decimal_integer::<u32>()?.into_inner();
        state.add_attribute(
            "num_slots",
            parser.context_rc().create_attribute::<U32Attr, _>(num_slots),
        );

        let entries = parser.parse_optional_region(&[], /*enable_name_shadowing=*/ false)?;
        // We always add the entries region, even if empty
        state.regions.push(entries.unwrap_or_else(|| parser.context().create_region()));

        Ok(())
    }
}

/// A [FunctionTableEntry] describes a single initialized slot of a [FunctionTable]: at program
/// startup, slot `index` is filled with the MAST root digest of `callee`.
///
/// This operation type is only permitted in the `entries` region of a [FunctionTable].
#[operation(
    dialect = BuiltinDialect,
    implements(OpPrinter)
)]
pub struct FunctionTableEntry {
    /// The table slot to initialize
    #[attr]
    index: U32Attr,
    /// The function whose MAST root fills the slot
    #[symbol(callable)]
    callee: SymbolPath,
}

impl OpPrinter for FunctionTableEntry {
    fn print(&self, printer: &mut AsmPrinter<'_>) {
        use crate::formatter::*;

        printer.print_space();
        *printer += display(*self.get_index());
        printer.print_space();
        let callee = self.callee();
        printer.print_symbol_path(callee.path());
    }
}

impl OpParser for FunctionTableEntry {
    fn parse(
        state: &mut crate::OperationState,
        parser: &mut dyn crate::OpAsmParser<'_>,
    ) -> crate::ParseResult {
        let index = parser.parse_decimal_integer::<u32>()?.into_inner();
        state.add_attribute("index", parser.context_rc().create_attribute::<U32Attr, _>(index));

        let callee = parser.parse_symbol_ref()?;
        state.attrs.push(NamedAttribute::new("callee", callee.into_inner()));

        Ok(())
    }
}
