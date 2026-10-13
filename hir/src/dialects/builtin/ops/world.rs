use alloc::{format, vec::Vec};

use smallvec::{SmallVec, smallvec};

use crate::{
    Context, OpPrinter, Operation, OperationName, OperationRef, RegionKind, RegionKindInterface,
    Report, Symbol, SymbolManager, SymbolManagerMut, SymbolMap, SymbolName, SymbolPath, SymbolRef,
    SymbolTable, SymbolUseList, UnsafeIntrusiveEntityRef, Usable, Verify,
    derive::{OpParser, OpPrinter, operation},
    dialects::builtin::{BuiltinDialect, Component, Module},
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
    /// Returns an error when the world declares a tree of modules or interfaces with a symbol at
    /// the path the component name `component` spells (a module, or a function or global named by
    /// the raw last segment), which the component would shadow, or another component whose name nests with
    /// `component` (one is a segment-prefix of the other) or names the same raw path: resolution
    /// prefers the longest registered name, so any of them would make paths ambiguous.
    ///
    /// The name is the Miden Assembly spelling of the component's namespace path (see
    /// [SymbolPath::to_symbol_name]).
    pub fn reject_component_shadowing(&self, component: SymbolName) -> Result<(), Report> {
        self.reject_nested_components(component)?;
        let segments = SymbolPath::segments_of(component);
        let Some((last, tables)) = segments.split_last() else {
            return Ok(());
        };
        // Walk the symbol tables (modules, interfaces) named by the leading segments; a component
        // is not walked into, since components do not nest
        let mut table = self.op.as_operation_ref();
        for segment in tables {
            let Some(next) = SymbolPath::spell_segment(*segment)
                .and_then(|name| table.borrow().as_trait::<dyn SymbolTable>()?.get(name))
                .map(|symbol| symbol.borrow().as_operation_ref())
                .filter(|op| {
                    let op = op.borrow();
                    op.implements::<dyn SymbolTable>() && !op.is::<Component>()
                })
            else {
                return Ok(());
            };
            table = next;
        }
        let shadowed = {
            let table = table.borrow();
            let table = table.as_trait::<dyn SymbolTable>().expect("a world or a symbol table");
            table.symbol_manager().symbols().get_leaf(*last)
        };
        // At the world level, the component found by its own name is not shadowed
        let Some(shadowed) = shadowed.filter(|op| !op.borrow().is::<Component>()) else {
            return Ok(());
        };
        let shadowed = shadowed.borrow();
        let path = shadowed.as_symbol().expect("only symbols are registered").path();
        Err(same_path_error(component, &shadowed.name(), &path))
    }

    /// Returns an error when a symbol named `name`, an op of `kind` about to be defined in the
    /// symbol table `table`, would be at or below the path of a world-level component: `table` is
    /// the world or a module of a world-level module tree, and the component is named by the path
    /// of the symbol or of one of its enclosing modules (see [World::component_shadowing]).
    ///
    /// Symbols defined anywhere else (in a component, say) are not checked, nor is anything
    /// spelled when the world holds no component.
    pub(crate) fn reject_symbol_shadowed_by_component(
        table: OperationRef,
        name: SymbolName,
        kind: &OperationName,
    ) -> Result<(), Report> {
        let is_table = kind.implements::<dyn SymbolTable>();
        // The raw segments of the path of the new symbol, leaf first
        let mut segments: SmallVec<[SymbolName; 4]> = smallvec![if is_table {
            SymbolPath::raw_segment(name)
        } else {
            name
        }];
        let mut current = table;
        loop {
            let parent = {
                let op = current.borrow();
                if op.is::<World>() {
                    break;
                }
                let Some(module) = op.downcast_ref::<Module>() else {
                    return Ok(());
                };
                segments.push(SymbolPath::raw_segment(Symbol::name(module)));
                op.parent_op()
            };
            let Some(parent) = parent else {
                return Ok(());
            };
            current = parent;
        }
        let world = current.borrow();
        let world = world.downcast_ref::<World>().expect("the loop ends at a world");
        let body = world.body();
        if body.is_empty() || !body.entry().body().iter().any(|op| op.is::<Component>()) {
            return Ok(());
        }
        drop(body);
        // No component has a segment Miden Assembly cannot spell
        if !segments.iter().all(|segment| SymbolPath::is_spellable(segment.as_str())) {
            return Ok(());
        }
        segments.reverse();
        let path = SymbolPath::spell(&segments);
        let Some(component) = world.component_shadowing(path) else {
            return Ok(());
        };
        if component == path {
            return Err(same_path_error(component, kind, &SymbolPath::from_symbol_name(path)));
        }
        // The component is a strict prefix of the path, so it shadows the enclosing module tree
        let module_path = SymbolPath::spell(&segments[..segments.len() - usize::from(!is_table)]);
        Err(shadowing_error(component, module_path, component))
    }

    /// Returns the name of the world-level component named by the module path `path`, given as
    /// its Miden Assembly spelling (see [SymbolPath::to_symbol_name]), or by a segment-prefix of
    /// it, i.e. the component that would shadow a module tree reaching `path`.
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
    /// segment-prefix of `component`, or extends it, or whose raw segments joined with `::` read
    /// the same as those of `component` (`"a::b"` and `a::b`).
    fn reject_nested_components(&self, component: SymbolName) -> Result<(), Report> {
        let body = self.body();
        if body.is_empty() {
            return Ok(());
        }
        let joined = |name: SymbolName| {
            SymbolPath::segments_of(name)
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join("::")
        };
        for op in body.entry().body() {
            let Some(existing) = op.downcast_ref::<Component>().map(Symbol::name) else {
                continue;
            };
            // Only a quoted segment can join to the text of different segments
            if existing != component
                && (existing.as_str().contains('"') || component.as_str().contains('"'))
                && joined(existing) == joined(component)
            {
                return Err(Report::msg(format!(
                    "component `{component}` and component `{existing}` both join to the segments \
                     `{}`; component namespaces must be distinct",
                    joined(component)
                )));
            }
            let (shorter, longer) = if existing.as_str().len() < component.as_str().len() {
                (existing, component)
            } else {
                (component, existing)
            };
            if SymbolPath::nests_in(longer, shorter) {
                return Err(Report::msg(format!(
                    "component `{component}` and component `{existing}` nest (`{shorter}` is a \
                     segment-prefix of `{longer}`); component namespaces must not nest"
                )));
            }
        }
        Ok(())
    }
}

/// The builders reject a component that shadows a symbol, or a symbol a component shadows, when
/// the second one of the two is defined through `WorldBuilder::define_component`,
/// `WorldBuilder::declare_module_tree` or the module, interface, function and global builders.
/// Parsed input, and ops built or renamed otherwise, are not checked, so the rule is checked here
/// as well.
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

/// The error for a component named `component` at the path `path` of a symbol of `kind`.
fn same_path_error(component: SymbolName, kind: &OperationName, path: &SymbolPath) -> Report {
    Report::msg(format!(
        "component `{component}` and `{kind}` `{path}` have the same path; a component must not \
         share its path with another symbol"
    ))
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
