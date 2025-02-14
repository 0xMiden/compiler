use crate::{
    derive::operation,
    dialects::builtin::BuiltinDialect,
    traits::{
        GraphRegionNoTerminator, HasOnlyGraphRegion, IsolatedFromAbove, NoRegionArguments,
        NoTerminator, SingleBlock, SingleRegion,
    },
    Ident, Operation, RegionKind, RegionKindInterface, Symbol, SymbolManager, SymbolManagerMut,
    SymbolMap, SymbolName, SymbolRef, SymbolTable, SymbolUseList, UnsafeIntrusiveEntityRef, Usable,
    Visibility,
};

pub type ModuleRef = UnsafeIntrusiveEntityRef<Module>;

/// A [Module] is a namespaced container for [Function] definitions, and represents the most atomic
/// translation unit that supports compilation to Miden Assembly.
///
/// [Module] cannot be nested, use [Component] for such use cases.
///
/// Modules can contain one of the following entities:
///
/// * [Segment], describing how a specific region of memory should be initialized (i.e. what content
///   it should be assumed to contain on program start). Segment definitions must not conflict
///   within a shared-everything boundary. For example, multiple segments within the same module,
///   or segments defined in sibling modules of the same [Component].
/// * [Function], either a declaration of an externally-defined function, or a definition.
///   Declarations are required in order to reference functions which are not in the compilation
///   graph, but are expected to be provided at runtime. The difference between the two depends on
///   whether or not the [Function] operation has a region (no region == declaration).
/// * [GlobalVariable], either a declaration of an externally-defined global, or a definition, same
///   as [Function].
///
/// Multiple modules can be grouped together into a [Component]. Doing so allows interprocedural
/// analysis to reason across call boundaries for functions defined in different modules, in
/// particular, dead code analysis.
///
/// Modules may also have a specified [Visibility]:
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
///   modules in the same [Component], and otherwise adheres to the same rules as `Internal`.
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
    implements(RegionKindInterface, SymbolTable, Symbol)
)]
pub struct Module {
    #[attr]
    name: Ident,
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

impl Module {
    #[inline(always)]
    pub fn as_module_ref(&self) -> ModuleRef {
        unsafe { ModuleRef::from_raw(self) }
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
        Module::name(self).as_symbol()
    }

    fn set_name(&mut self, name: SymbolName) {
        let id = self.name_mut();
        id.name = name;
    }

    fn visibility(&self) -> Visibility {
        *Module::visibility(self)
    }

    fn set_visibility(&mut self, visibility: Visibility) {
        *self.visibility_mut() = visibility;
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
