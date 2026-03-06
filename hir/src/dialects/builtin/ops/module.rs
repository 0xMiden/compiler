use crate::{
    OpParser, OpPrinter, Operation, RegionKind, RegionKindInterface, Symbol, SymbolManager,
    SymbolManagerMut, SymbolMap, SymbolName, SymbolRef, SymbolTable, SymbolUseList,
    UnsafeIntrusiveEntityRef, Usable, Visibility,
    derive::operation,
    dialects::builtin::{
        BuiltinDialect,
        attributes::{IdentAttr, VisibilityAttr},
    },
    print::AsmPrinter,
    traits::{
        GraphRegionNoTerminator, HasOnlyGraphRegion, IsolatedFromAbove, NoRegionArguments,
        NoTerminator, SingleBlock, SingleRegion,
    },
};

pub type ModuleRef = UnsafeIntrusiveEntityRef<Module>;

/// A [Module] is a namespaced container for [super::Function] definitions, and represents the most
/// atomic translation unit that supports compilation to Miden Assembly.
///
/// [Module] cannot be nested, use [super::Component] for such use cases.
///
/// Modules can contain one of the following entities:
///
/// * [super::Segment], describing how a specific region of memory should be initialized (i.e. what
///   content it should be assumed to contain on program start). Segment definitions must not
///   conflict within a shared-everything boundary. For example, multiple segments within the same
///   module, or segments defined in sibling modules of the same [super::Component].
/// * [super::Function], either a declaration of an externally-defined function, or a definition.
///   Declarations are required in order to reference functions which are not in the compilation
///   graph, but are expected to be provided at runtime. The difference between the two depends on
///   whether or not the [super::Function] operation has a region (no region == declaration).
/// * [super::GlobalVariable], either a declaration of an externally-defined global, or a
///   definition, same as [super::Function].
///
/// Multiple modules can be grouped together into a [super::Component]. Doing so allows
/// interprocedural analysis to reason across call boundaries for functions defined in different
/// modules, in particular, dead code analysis.
///
/// Modules may also have a specified [crate::dialects::builtin::attributes::Visibility]:
///
/// * `Visibility::Public` indicates that all functions exported from the module with `Public`
///   visibility form the public interface of the module, and thus are not permitted to be dead-
///   code eliminated, or otherwise rewritten by optimizations in a way that changes the public
///   interface.
/// * `Visibility::Internal` indicates that all functions exported from the module with `Public`
///   or `Internal` visibility are only visibile by modules in the current compilation graph, and
///   are thus eligible for dead-code elimination or other invasive rewrites so long as all
///   callsites are known statically. If the address of any of those functions is captured, they
///   must not be modified.
/// * `Visibility::Private` indicates that the module and its exports are only visible to other
///   modules in the same [super::Component], and otherwise adheres to the same rules as `Internal`.
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
pub struct Module {
    #[attr]
    name: IdentAttr,
    #[attr]
    #[default]
    visibility: VisibilityAttr,
    #[region]
    body: RegionRef,
    #[default]
    symbols: SymbolMap,
    #[default]
    uses: SymbolUseList,
}

impl Module {
    #[inline(always)]
    pub fn as_module_ref(&self) -> ModuleRef {
        unsafe { ModuleRef::from_raw(self) }
    }
}

impl OpPrinter for Module {
    fn print(&self, printer: &mut AsmPrinter<'_>) {
        printer.print_space();
        printer.print_keyword(self.get_visibility().as_str());
        printer.print_space();
        printer.print_symbol_name(self.get_name().as_symbol());
        printer.print_space();
        printer.print_region(&self.body());
    }
}

impl OpParser for Module {
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
            .expect("one or more of these visibilities are no longer valid");
        state.add_attribute(
            "visibility",
            parser.context_rc().create_attribute::<VisibilityAttr, _>(visibility),
        );

        let name = parser.parse_symbol_name()?;
        state.add_attribute("name", parser.context_rc().create_attribute::<IdentAttr, _>(name));

        let region = parser.context().create_region();
        parser.parse_region(region, &[], true)?;
        state.add_region(region);

        Ok(())
    }
}

impl midenc_session::Emit for Module {
    fn name(&self) -> Option<midenc_hir_symbol::Symbol> {
        Some(self.name().as_symbol())
    }

    fn output_type(&self, _mode: midenc_session::OutputMode) -> midenc_session::OutputType {
        midenc_session::OutputType::Hir
    }

    fn write_to<W: midenc_session::Writer>(
        &self,
        mut writer: W,
        _mode: midenc_session::OutputMode,
        _session: &midenc_session::Session,
    ) -> anyhow::Result<()> {
        use crate::Op;
        let flags = crate::OpPrintingFlags::default();
        let mut printer = AsmPrinter::new(self.as_operation().context_rc(), &flags);
        <Self as OpPrinter>::print(self, &mut printer);
        let document = printer.finish();
        writer.write_fmt(format_args!("{document}"))
    }
}

impl RegionKindInterface for Module {
    #[inline(always)]
    fn kind(&self) -> RegionKind {
        RegionKind::Graph
    }
}

impl Usable for Module {
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

impl Symbol for Module {
    #[inline(always)]
    fn as_symbol_operation(&self) -> &Operation {
        &self.op
    }

    #[inline(always)]
    fn as_symbol_operation_mut(&mut self) -> &mut Operation {
        &mut self.op
    }

    fn name(&self) -> SymbolName {
        self.get_name().as_symbol()
    }

    fn set_name(&mut self, name: SymbolName) {
        Module::set_name(self, name)
    }

    fn visibility(&self) -> Visibility {
        *Module::get_visibility(self)
    }

    fn set_visibility(&mut self, visibility: Visibility) {
        Module::set_visibility(self, visibility)
    }
}

impl SymbolTable for Module {
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
