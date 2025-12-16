use midenc_session::LibraryPath;

use crate::{
    Ident, Op, Operation, RegionKind, RegionKindInterface, Symbol, SymbolManager, SymbolManagerMut,
    SymbolMap, SymbolName, SymbolRef, SymbolTable, SymbolUseList, UnsafeIntrusiveEntityRef, Usable,
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
/// uses the Canonical ABI of the Wasm Component Model. However, it is possible for a [Component]
/// to export multiple interfaces which are implemented within the same shared-everything boundary.
/// Even when that is the case, calls to any [Interface] export from within that boundary, will
/// still be treated as crossing a shared-nothing boundary. In this way, components can both define
/// and re-export interfaces from other components, without callers needing to know where the
/// actual definition is provided from.
///
/// Interfaces correspond to component _instances_ exported from a component _definition_ in the
/// Wasm Component Model. This means that they are almost identical concepts, however we distinguish
/// between [Component] and [Interface] in the IR to better model the relationships between these
/// concepts, as well as to draw a connection to interfaces in WIT (WebAssembly Interface Types).
///
/// ## Contents
///
/// Interfaces may only contain [Function] items, and may only _export_ functions with the
/// `CanonLift` calling convention. It is expected that these functions will rely on implementation
/// details defined in a sibling [Module], though that is not strictly required.
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
pub struct Interface {
    #[attr]
    name: Ident,
    #[region]
    body: RegionRef,
    #[default]
    symbols: SymbolMap,
    #[default]
    uses: SymbolUseList,
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
        let id = self.name_mut();
        id.name = name;
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

impl Interface {
    /// Get the Miden Assembly [LibraryPath] that uniquely identifies this interface.
    pub fn library_path(&self) -> Option<LibraryPath> {
        let parent = self.as_operation().parent_op()?;
        let parent = parent.borrow();
        let component = parent
            .downcast_ref::<builtin::Component>()
            .expect("invalid parent for interface operation: expected component");
        let component_id = component.id();
        let path = component_id.to_library_path();
        Some(path.append_unchecked(self.name().as_str()))
    }
}
