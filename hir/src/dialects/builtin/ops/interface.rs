use midenc_session::LibraryPath;

use crate::{
    Context, IdentAttr, Op, OpParser, OpPrinter, Operation, RegionKind, RegionKindInterface,
    Report, Symbol, SymbolManager, SymbolManagerMut, SymbolMap, SymbolName, SymbolPath, SymbolRef,
    SymbolTable, SymbolTableKind, SymbolUseList, UnsafeIntrusiveEntityRef, Usable, Verify,
    Visibility,
    derive::operation,
    dialects::builtin::{self, BuiltinDialect},
    traits::{
        GraphRegionNoTerminator, HasOnlyGraphRegion, IsolatedFromAbove, NoRegionArguments,
        NoTerminator, SingleBlock, SingleRegion,
    },
};

pub type InterfaceRef = UnsafeIntrusiveEntityRef<Interface>;

/// An [Interface] is a modular abstraction operation, i.e. it is designed to model groups of
/// related functionality meant to be produced/consumed together.
///
/// An [Interface] itself represents a shared-nothing boundary, i.e. the functionality it exports
/// uses the Canonical ABI of the Wasm Component Model. However, it is possible for a
/// [super::Component] to export multiple interfaces which are implemented within the same
/// shared-everything boundary. Even when that is the case, calls to any [Interface] export from
/// within that boundary, will still be treated as crossing a shared-nothing boundary. In this way,
/// components can both define and re-export interfaces from other components, without callers
/// needing to know where the actual definition is provided from.
///
/// Interfaces correspond to component _instances_ exported from a component _definition_ in the
/// Wasm Component Model. This means that they are almost identical concepts, however we distinguish
/// between [super::Component] and [Interface] in the IR to better model the relationships between
/// these concepts, as well as to draw a connection to interfaces in WIT (WebAssembly Interface Types).
///
/// ## Contents
///
/// Interfaces may only contain [builtin::Function] items, and may only _export_ functions with the
/// `CanonLift` calling convention. It is expected that these functions will rely on implementation
/// details defined in a sibling [builtin::Module], though that is not strictly required.
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
    implements(RegionKindInterface, SymbolTable, Symbol, OpPrinter)
)]
pub struct Interface {
    #[attr]
    name: IdentAttr,
    #[region]
    body: RegionRef,
    #[default]
    symbols: SymbolMap,
    #[default]
    uses: SymbolUseList,
}

impl OpPrinter for Interface {
    fn print(&self, printer: &mut crate::print::AsmPrinter<'_>) {
        printer.print_space();
        // Printed as its raw segment, which the parser re-spells
        printer.print_symbol_name(SymbolPath::raw_segment(self.get_name().as_symbol()));
        printer.print_space();
        printer.print_region(&self.body());
    }
}

impl OpParser for Interface {
    fn parse(
        state: &mut crate::OperationState,
        parser: &mut dyn crate::OpAsmParser<'_>,
    ) -> crate::ParseResult {
        // The name is the Miden Assembly spelling of the parsed raw segment
        let name = parser.parse_symbol_name()?;
        let name = crate::Ident::new(
            Interface::canonical_name(name.name).map_err(|err| {
                crate::parse::ParserError::invalid_symbol_table_name(name.span, err)
            })?,
            name.span,
        );
        state.add_attribute("name", parser.context_rc().create_attribute::<IdentAttr, _>(name));

        let region = parser.context().create_region();
        parser.parse_region(region, &[], true)?;
        state.add_region(region);

        Ok(())
    }
}

impl RegionKindInterface for Interface {
    #[inline(always)]
    fn kind(&self) -> RegionKind {
        RegionKind::Graph
    }
}

impl Usable for Interface {
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

impl Symbol for Interface {
    #[inline(always)]
    fn as_symbol_operation(&self) -> &Operation {
        &self.op
    }

    #[inline(always)]
    fn as_symbol_operation_mut(&mut self) -> &mut Operation {
        &mut self.op
    }

    fn name(&self) -> SymbolName {
        Interface::name(self).as_symbol()
    }

    fn set_name(&mut self, name: SymbolName) {
        Interface::set_name(self, name)
    }

    fn visibility(&self) -> Visibility {
        Visibility::Public
    }

    fn set_visibility(&mut self, visibility: Visibility) {
        assert_eq!(
            visibility,
            Visibility::Public,
            "cannot give interfaces a visibility other than public"
        );
    }
}

impl SymbolTable for Interface {
    #[inline(always)]
    fn as_symbol_table_operation(&self) -> &Operation {
        &self.op
    }

    #[inline(always)]
    fn as_symbol_table_operation_mut(&mut self) -> &mut Operation {
        &mut self.op
    }

    fn symbol_manager(&self) -> SymbolManager<'_> {
        SymbolManager::new(&self.op, crate::Symbols::Borrowed(&self.symbols))
    }

    fn symbol_manager_mut(&mut self) -> SymbolManagerMut<'_> {
        SymbolManagerMut::new(&mut self.op, crate::SymbolsMut::Borrowed(&mut self.symbols))
    }

    #[inline]
    fn get(&self, name: SymbolName) -> Option<SymbolRef> {
        self.symbols.get(name)
    }
}

/// Interfaces are built through the builtin builders and the parser, which spell and validate
/// their names; a name set afterwards is not, so it is checked here as well.
impl Verify<dyn SymbolTable> for Interface {
    fn verify(&self, _context: &Context) -> Result<(), Report> {
        SymbolPath::validate_table_name(SymbolTableKind::Interface, Symbol::name(self))
    }
}

impl Interface {
    /// Returns the name an interface given the raw segment `name` gets: its Miden Assembly
    /// spelling (see [SymbolPath::segments_of]), e.g. `"my-api"` for `my-api` and `api` for `api`.
    ///
    /// `name` is always read as one raw segment, never as a spelled name: `"my-api"` is not the
    /// segment `my-api`. Returns an error when Miden Assembly cannot spell the segment (see
    /// [SymbolPath::is_spellable]) or it contains `::`.
    pub fn canonical_name(name: SymbolName) -> Result<SymbolName, Report> {
        SymbolPath::table_name(SymbolTableKind::Interface, name)
    }

    /// Get the Miden Assembly [LibraryPath] that uniquely identifies this interface.
    pub fn library_path(&self) -> Option<LibraryPath> {
        let parent = self.as_operation().parent_op()?;
        let parent = parent.borrow();
        let component = parent
            .downcast_ref::<builtin::Component>()
            .expect("invalid parent for interface operation: expected component");
        let mut path = component.namespace_path().to_library_path();
        let name = self.name().as_str();
        let suffix = LibraryPath::new(&name).expect("invalid interface module name");
        path.push(suffix.as_path());
        Some(path)
    }
}
