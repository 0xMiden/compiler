use crate::{
    OpPrinter, Operation, RegionKind, RegionKindInterface, SymbolManager, SymbolManagerMut,
    SymbolMap, SymbolName, SymbolRef, SymbolTable, SymbolUseList, UnsafeIntrusiveEntityRef, Usable,
    derive::{OpParser, OpPrinter, operation},
    dialects::builtin::BuiltinDialect,
    traits::{
        GraphRegionNoTerminator, HasOnlyGraphRegion, IsolatedFromAbove, NoRegionArguments,
        NoTerminator, SingleBlock, SingleRegion,
    },
};

pub type WorldRef = UnsafeIntrusiveEntityRef<World>;

/// [World] represents the global namespace which all symbols are resolved relative to.
///
/// A world consists of a single region in which `Symbol`-like operations are declared/defined. It
/// is most analagous to worlds in the WebAssembly Interface Types spec.
///
/// Currently, worlds are presumed to contain one of the following:
///
/// * [super::Component]s
/// * [super::Interface]s
/// * [super::Module]s
///
/// The codegen backend currently does not support lowering from [World] directly when the world
/// contains [super::Component]s - each component must be lowered independently, as we currently
/// expect components to map 1:1 with packages.
///
/// NOTE: Worlds always have `Public` visibility.
#[derive(OpPrinter, OpParser)]
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
    implements(RegionKindInterface, SymbolTable, OpPrinter)
)]
pub struct World {
    #[region]
    body: RegionRef,
    #[default]
    symbols: SymbolMap,
    #[default]
    uses: SymbolUseList,
}

impl RegionKindInterface for World {
    #[inline(always)]
    fn kind(&self) -> RegionKind {
        RegionKind::Graph
    }
}

impl Usable for World {
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

impl SymbolTable for World {
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
