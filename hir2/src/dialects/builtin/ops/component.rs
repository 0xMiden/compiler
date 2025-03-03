mod interface;

pub use self::interface::{
    ComponentExport, ComponentId, ComponentInterface, ModuleExport, ModuleInterface,
};
use crate::{
    derive::operation,
    dialects::builtin::BuiltinDialect,
    traits::{
        GraphRegionNoTerminator, HasOnlyGraphRegion, IsolatedFromAbove, NoRegionArguments,
        NoTerminator, SingleBlock, SingleRegion,
    },
    version::Version,
    Ident, OpPrinter, Operation, RegionKind, RegionKindInterface, Symbol, SymbolManager,
    SymbolManagerMut, SymbolMap, SymbolName, SymbolRef, SymbolTable, SymbolUseList,
    UnsafeIntrusiveEntityRef, Usable, Visibility,
};

pub type ComponentRef = UnsafeIntrusiveEntityRef<Component>;

/// A [Component] is a modular abstraction operation, i.e. it is designed to model shared-nothing
/// boundaries between groups of shared-everything modules in a system.
///
/// Components can contain the following entities:
///
/// * [Interface], used to export groups of related functionality from the component. Interfaces
///   always have `Public` visibility.
/// * [Function] used to export standalone component-level functions, e.g. a program entrypoint,
///   or component initializer. These functions always have `Public` visibility, and must be
///   representable using the Canonical ABI.
/// * [Module], used to implement the functionality exported backing an [Interface] or a component-
///   level [Function]. Modules may not have `Public` visibility. All modules within a [Component]
///   are within the same shared-everything boundary, so conflicting data segment declarations are
///   not allowed. Additionally, global variables within the same shared-everything boundary
///   are allocated in the same linear memory address space.
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
/// * A [Component] corresponds to a Miden Assembly namespace, and a Miden package
/// * Component-level functions are emitted to a MASM module corresponding to the root of the
///   namespace, i.e. as if defined in `mod.masm` at the root of a MASM source project.
/// * Each [Interface] of a component is emitted to a MASM module of the same name
/// * Each [Module] of a component is emitted to a MASM module of the same name
/// * The [Segment] declarations of all modules in the component are gathered together, checked for
///   overlap, hashed, and then added to the set of advice map entries to be initialized when the
///   resulting package is loaded. The initialization code generated to load the data segments into
///   the linear memory of the component, is placed in a top-level component function called `init`.
/// * The [GlobalVariable] declarations of all modules in the component are gathered together,
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
    namespace: Ident,
    #[attr]
    name: Ident,
    #[attr]
    version: Version,
    #[attr]
    #[default]
    visibility: Visibility,
    #[region]
    body: RegionRef,
    #[default]
    symbols: SymbolMap,
    #[default]
    uses: SymbolUseList,
}

impl OpPrinter for Component {
    fn print(
        &self,
        flags: &crate::OpPrintingFlags,
        _context: &crate::Context,
    ) -> crate::formatter::Document {
        use crate::formatter::*;

        let header = display(self.op.name()) + const_text(" ") + display(self.id());
        let body = crate::print::render_regions(&self.op, flags);
        header + body
    }
}

impl midenc_session::Emit for Component {
    fn name(&self) -> Option<midenc_hir_symbol::Symbol> {
        Some(self.name().as_symbol())
    }

    fn output_type(&self, _mode: midenc_session::OutputMode) -> midenc_session::OutputType {
        midenc_session::OutputType::Hir
    }

    fn write_to<W: std::io::Write>(
        &self,
        mut writer: W,
        _mode: midenc_session::OutputMode,
        _session: &midenc_session::Session,
    ) -> std::io::Result<()> {
        let flags = crate::OpPrintingFlags::default();
        let document = <Component as OpPrinter>::print(self, &flags, self.op.context());
        writer.write_fmt(format_args!("{}", document))
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

    fn name(&self) -> SymbolName {
        let id = ComponentId {
            namespace: self.namespace().as_symbol(),
            name: Component::name(self).as_symbol(),
            version: self.version().clone(),
        };
        SymbolName::intern(id)
    }

    fn set_name(&mut self, name: SymbolName) {
        let ComponentId {
            name,
            namespace,
            version,
        } = name.as_str().parse::<ComponentId>().expect("invalid component identifier");
        self.name_mut().name = name;
        self.namespace_mut().name = namespace;
        *self.version_mut() = version;
    }

    fn visibility(&self) -> Visibility {
        *Component::visibility(self)
    }

    fn set_visibility(&mut self, visibility: Visibility) {
        *self.visibility_mut() = visibility;
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
    fn get(&self, name: SymbolName) -> Option<SymbolRef> {
        self.symbols.get(name)
    }
}

impl Component {
    pub fn id(&self) -> ComponentId {
        let namespace = self.namespace().as_symbol();
        let name = self.name().as_symbol();
        let version = self.version().clone();
        ComponentId {
            namespace,
            name,
            version,
        }
    }
}
