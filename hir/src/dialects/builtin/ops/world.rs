use alloc::format;

use crate::{
    Context, OpPrinter, Operation, RegionKind, RegionKindInterface, Report, Symbol, SymbolManager,
    SymbolManagerMut, SymbolMap, SymbolName, SymbolPath, SymbolRef, SymbolTable, SymbolUseList,
    UnsafeIntrusiveEntityRef, Usable, Verify,
    derive::{OpParser, OpPrinter, operation},
    dialects::builtin::{BuiltinDialect, Component, Module, ModuleRef},
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

impl World {
    /// Returns an error when the world declares a module tree reaching the path the component name
    /// `component` spells, which the component would shadow, or another component whose name
    /// nests with `component` (one is a segment-prefix of the other): resolution prefers the
    /// longest registered name, so either would make paths ambiguous.
    ///
    /// The name is the component's namespace path, with its segments joined by `::`.
    pub fn reject_component_shadowing(&self, component: SymbolName) -> Result<(), Report> {
        self.reject_nested_components(component)?;
        let mut segments = SymbolPath::segments_of(component).into_iter();
        let Some(mut module) =
            segments.next().and_then(|first| self.get(first)).and_then(as_module)
        else {
            return Ok(());
        };
        for segment in segments {
            let Some(next) = module.borrow().get(segment).and_then(as_module) else {
                return Ok(());
            };
            module = next;
        }
        Err(shadowing_error(component, component, component))
    }

    /// Returns the name of the world-level component named by the `::`-joined module path `path`
    /// or by a segment-prefix of it, i.e. the component that would shadow a module tree reaching
    /// `path`.
    pub(crate) fn component_shadowing(&self, path: SymbolName) -> Option<SymbolName> {
        let body = self.body();
        if body.is_empty() {
            return None;
        }
        for op in body.entry().body() {
            let Some(name) = op.downcast_ref::<Component>().map(Symbol::name) else {
                continue;
            };
            if name == path || SymbolPath::nests_in(path, name) {
                return Some(name);
            }
        }
        None
    }

    /// Returns an error when a world-level component other than `component` has a name that is a
    /// segment-prefix of `component`, or extends it.
    fn reject_nested_components(&self, component: SymbolName) -> Result<(), Report> {
        let body = self.body();
        if body.is_empty() {
            return Ok(());
        }
        for op in body.entry().body() {
            let Some(existing) = op.downcast_ref::<Component>().map(Symbol::name) else {
                continue;
            };
            let (shorter, longer) = if existing.as_str().len() < component.as_str().len() {
                (existing, component)
            } else {
                (component, existing)
            };
            if SymbolPath::nests_in(longer, shorter) {
                return Err(Report::msg(format!(
                    "component `{component}` and component `{existing}` nest (`{shorter}` is a \
                     prefix of `{longer}`); component namespaces must not nest"
                )));
            }
        }
        Ok(())
    }
}

/// A world is built through `WorldBuilder`, which enforces the shadowing rule as it goes; parsed
/// input is not, so the rule is checked here as well.
impl Verify<dyn SymbolTable> for World {
    fn verify(&self, _context: &Context) -> Result<(), Report> {
        let body = self.body();
        if body.is_empty() {
            return Ok(());
        }
        for op in body.entry().body() {
            if let Some(component) = op.downcast_ref::<Component>() {
                self.reject_component_shadowing(Symbol::name(component))?;
            }
        }
        Ok(())
    }
}

/// The module `symbol` refers to, if it is one.
fn as_module(symbol: SymbolRef) -> Option<ModuleRef> {
    symbol
        .borrow()
        .as_symbol_operation()
        .downcast_ref::<Module>()
        .map(|m| m.as_module_ref())
}

/// The error for a component named `component` that shadows the module path `module_path`, the
/// two sharing the prefix `prefix`.
pub(crate) fn shadowing_error(
    component: SymbolName,
    module_path: SymbolName,
    prefix: SymbolName,
) -> Report {
    Report::msg(format!(
        "component `{component}` and module path `{module_path}` share the prefix `{prefix}`; a \
         component name must not shadow a module tree"
    ))
}
