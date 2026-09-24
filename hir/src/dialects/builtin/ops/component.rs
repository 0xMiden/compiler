mod interface;

pub use self::interface::{ComponentExport, ComponentInterface, ModuleExport, ModuleInterface};
use crate::{
    Ident, IdentAttr, Op, OpParser, OpPrinter, Operation, RegionKind, RegionKindInterface, Symbol,
    SymbolManager, SymbolManagerMut, SymbolMap, SymbolNameComponent, SymbolPath, SymbolRef,
    SymbolTable, SymbolUseList, UnsafeIntrusiveEntityRef, Usable, Visibility,
    derive::operation,
    dialects::builtin::{BuiltinDialect, attributes::VisibilityAttr},
    interner,
    print::AsmPrinter,
    traits::{
        GraphRegionNoTerminator, HasOnlyGraphRegion, IsolatedFromAbove, NoRegionArguments,
        NoTerminator, SingleBlock, SingleRegion,
    },
};

pub type ComponentRef = UnsafeIntrusiveEntityRef<Component>;

/// A [Component] is a modular abstraction operation, i.e. it is designed to model shared-nothing
/// boundaries between groups of shared-everything modules in a system.
///
/// Components can contain the following entities:
///
/// * [super::Interface], used to export groups of related functionality from the component.
///   Interfaces always have `Public` visibility.
/// * [super::Function] used to export standalone component-level functions, e.g. a program
///   entrypoint, or component initializer. These functions always have `Public` visibility, and
///   must be representable using the Canonical ABI.
/// * [super::Module], used to implement the functionality exported backing an [super::Interface] or
///   a component-level [super::Function]. Modules may not have `Public` visibility. All modules
///   within a [Component] are within the same shared-everything boundary, so conflicting data
///   segment declarations are not allowed. Additionally, global variables within the same
///   shared-everything boundary are allocated in the same linear memory address space.
///
/// Externally-defined functions are represented as declarations, and must be referenced using their
/// fully-qualified name in order to resolve them.
///
/// ## Linking
///
/// NOTE: Components always have `Public` visibility.
///
/// Components are linked into Miden Assembly according to the following rules:
///
/// * A [Component] corresponds to a Miden Assembly namespace, and a Miden package. The name of the
///   component IS that namespace path, e.g. `miden::counter_contract::counter_contract`: its
///   `::`-separated segments are the leading components of the path of every symbol inside it
///   (see [Component::namespace_path]).
/// * Component-level functions are emitted to a MASM module corresponding to the root of the
///   namespace, i.e. as if defined in `mod.masm` at the root of a MASM source project.
/// * Each [super::Interface] of a component is emitted to a MASM module of the same name
/// * Each [super::Module] of a component is emitted to a MASM module of the same name
/// * The [super::Segment] declarations of all modules in the component are gathered together,
///   checked for overlap, hashed, and then added to the set of advice map entries to be initialized
///   when the resulting package is loaded. The initialization code generated to load the data
///   segments into the linear memory of the component, is placed in a top-level component function
///   called `init`.
/// * The [super::GlobalVariable] declarations of all modules in the component are gathered together,
///   de-duplicated, initializer data hashed and added to the set of advice map entries of the
///   package, and allocated specific offsets in the address space of the component. Loads/stores
///   of these variables will be lowered to use these allocated offsets. The initialization code
///   for each global will be emitted in the top-level component function called `init`.
/// * The set of externally-defined components that have at least one reference, will be added as
///   dependencies of the output package.
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
pub struct Component {
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

impl OpPrinter for Component {
    fn print(&self, printer: &mut AsmPrinter<'_>) {
        printer.print_space();
        printer.print_keyword(self.get_visibility().as_str());
        printer.print_space();
        let path = SymbolPath::from_iter(
            SymbolPath::segments_of(Symbol::name(self))
                .into_iter()
                .map(SymbolNameComponent::Component),
        );
        printer.print_symbol_path(&path);
        printer.print_space();
        printer.print_region(&self.body());
    }
}

impl OpParser for Component {
    fn parse(
        state: &mut crate::OperationState,
        parser: &mut dyn crate::OpAsmParser<'_>,
    ) -> crate::ParseResult {
        use crate::parse::Token;

        let context = parser.context_rc();
        let visibility = parser
            .parse_keyword_from(&[
                Token::BareIdent("public"),
                Token::BareIdent("private"),
                Token::BareIdent("internal"),
            ])?
            .into_inner()
            .parse::<Visibility>()
            .expect("one or more of these visibilities are no longer valid");
        state
            .add_attribute("visibility", context.create_attribute::<VisibilityAttr, _>(visibility));

        // The name is the whole (possibly multi-segment) path, joined with `::`
        let (span, path) = parser.parse_symbol_path()?.into_parts();
        let name = path.to_symbol_name();
        state.add_attribute(
            "name",
            context.create_attribute::<IdentAttr, _>(Ident::new(name, span)),
        );

        let region = parser.context().create_region();
        parser.parse_region(region, &[], true)?;
        state.add_region(region);

        Ok(())
    }
}

impl midenc_session::Emit for Component {
    /// The last segment of the component name, which is file-name friendly.
    fn name(&self) -> Option<midenc_hir_symbol::Symbol> {
        SymbolPath::segments_of(self.get_name().as_symbol()).last().copied()
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
        use crate::OpPrinter;
        let flags = crate::OpPrintingFlags::default();
        let mut printer = AsmPrinter::new(self.as_operation().context_rc(), &flags);
        <Self as OpPrinter>::print(self, &mut printer);
        writer.write_fmt(format_args!("{}", printer.finish()))
    }
}

impl RegionKindInterface for Component {
    #[inline(always)]
    fn kind(&self) -> RegionKind {
        RegionKind::Graph
    }
}

impl Usable for Component {
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

impl Symbol for Component {
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
        self.name_mut().name = name;
    }

    fn visibility(&self) -> Visibility {
        *self.get_visibility()
    }

    fn set_visibility(&mut self, visibility: Visibility) {
        *self.get_visibility_mut() = visibility;
    }
}

impl SymbolTable for Component {
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
    fn get(&self, name: interner::Symbol) -> Option<SymbolRef> {
        self.symbols.get(name)
    }
}

impl Component {
    /// Name of the optional operation attribute (a `BoolAttr`) marking a component the compiler
    /// invented to wrap a bare core module, rather than one an author wrote.
    ///
    /// This is a marker rather than a name comparison because the name is not the compiler's to
    /// reserve: `root_ns:root@1.0.0`, the name of the wrapper, is a name an author may write, and
    /// a component carrying it is theirs, with the module visibility they declared.
    pub const SYNTHETIC_WRAPPER_ATTR: &'static str = "synthetic_wrapper";

    /// Returns the absolute path this component is rooted at, with one component per `::`-separated
    /// segment of its name; every symbol inside the component has it as a prefix.
    ///
    /// Note that `Symbol::path()` of the component itself ends in a single `Leaf` holding its
    /// whole name instead.
    pub fn namespace_path(&self) -> SymbolPath {
        SymbolPath::namespace_from_name(Symbol::name(self))
    }

    /// Mark this component as the compiler's wrapper around a bare core module.
    ///
    /// The marker is a plain attribute, but setting it by hand is three lines of attribute
    /// plumbing that say nothing about what is being asserted, and it is asserted from the
    /// frontend that invents the wrapper and from every test standing in for that frontend. This
    /// is the counterpart of [`Component::is_synthetic_wrapper`], and the two belong together:
    /// whatever the marker's representation is, both ends of it should change at once.
    pub fn mark_synthetic_wrapper(&mut self) {
        let marker = self
            .as_operation()
            .context_rc()
            .create_attribute::<crate::dialects::builtin::attributes::BoolAttr, _>(true);
        self.as_operation_mut().set_attribute(Self::SYNTHETIC_WRAPPER_ATTR, marker);
    }

    /// Returns true if this component is the compiler's wrapper around a bare core module.
    pub fn is_synthetic_wrapper(&self) -> bool {
        self.as_operation()
            .get_typed_attribute::<crate::dialects::builtin::attributes::BoolAttr>(
                Self::SYNTHETIC_WRAPPER_ATTR,
            )
            .is_some_and(|attr| **attr.borrow())
    }

    #[inline(always)]
    pub fn as_component_ref(&self) -> ComponentRef {
        unsafe { ComponentRef::from_raw(self) }
    }
}
