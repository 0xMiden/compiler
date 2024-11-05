mod interface;

pub use self::interface::{
    ComponentExport, ComponentId, ComponentInterface, ModuleExport, ModuleInterface,
};
use crate::{
    derive::operation,
    dialects::hir::HirDialect,
    traits::{
        GraphRegionNoTerminator, HasOnlyGraphRegion, IsolatedFromAbove, NoRegionArguments,
        NoTerminator, SingleBlock, SingleRegion,
    },
    version::Version,
    Ident, Operation, RegionKind, RegionKindInterface, Symbol, SymbolManager, SymbolManagerMut,
    SymbolMap, SymbolName, SymbolRef, SymbolTable, SymbolUseList, Usable, Visibility,
};

/// A [Component] is a modular abstraction operation, i.e. it is designed to model shared-nothing
/// boundaries between groups of shared-everything modules in a system.
///
/// Components can contain the following entities:
///
/// * [Segment], describing how a specific region of memory, shared across all modules in the
///   component, should be initialized (i.e. what content it should be assumed to contain on
///   program start). This must be defined at the component level, as it affects all modules in
///   the component (though not those of nested components), and so we require data segments to
///   be defined at the root of the shared context.
/// * [Module], either as an abstract interface description (more below), or as the implementation
///   of such a description. All modules within a [Component] share the same resources (i.e. memory)
///   and can participate in interprocedural optimization.
/// * [Component], either as an abstract interface description (see below), or as the implementation
///   of a component visible only within the current component. Components nested within a
///   [Component] do _not_ share the same resources, instead, this is how many components are linked
///   together into larger programs.
///
/// ## Component Interfaces
///
/// As mentioned above, within a [Component], both modules and components can be expressed in a form
/// that represents an abstract _interface_. This is used for two purposes:
///
/// 1. To import an externally-defined component for use by the program. This is used much like how
///    [Function] can either be a declaration or a definition. Such components look like a normal
///    component, except there are no function definitions contained within. To facilitate efficient
///    compilation, such components are flagged with an `external` attribute. Any component with
///    this flag set is assumed to contain only declarations of publically-visible symbols.
/// 2. To indicate the specific elements of an externally-defined component that are needed by the
///    program. Any component definition which contains those elements can satisfy the interface.
///
/// ## Linking
///
/// Components may also have a specified [Visibility]:
///
/// * `Visibility::Public` indicates that all modules (and components) with `Public` visibility form
///   the public interface of the component (and correspondingly, `Public` functions of any `Public`
///   modules are also part of that interface). In addition to constituting the _component
///   interface_, these public entities are further restricted from being candidates for dead-code
///   elimination, or other aggressive optimizations.
/// * `Visibility::Internal` indicates that the component is only visible to entities in the current
///   compilation graph, so all uses can be assumed to be visible to the compiler. The component
///   interface is otherwise described the same as `Public`.
/// * `Visibility::Private` indicates that the component is only visible within its parent component,
///   if it has one. As with the other two visibilities, the interface of the component module is
///   described by its publically-visible contents.
#[operation(
    dialect = HirDialect,
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
        Component::name(self).as_symbol()
    }

    fn set_name(&mut self, name: SymbolName) {
        let id = self.name_mut();
        id.name = name;
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
