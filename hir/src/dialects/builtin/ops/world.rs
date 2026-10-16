use alloc::format;

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
    /// Returns true when the world holds a component that is not declaration-only (see
    /// [Component::is_declaration_only]), i.e. a component codegen lowers.
    ///
    /// A world holding declaration-only components alone, such as import stubs beside top-level
    /// modules, is lowered through its modules, as a world without components is.
    pub fn has_defining_component(&self) -> bool {
        let body = self.body();
        body.entry_block_ref().is_some_and(|entry| {
            entry.borrow().body().iter().any(|op| {
                op.downcast_ref::<Component>()
                    .is_some_and(|component| !component.is_declaration_only())
            })
        })
    }

    /// Returns an error when the world declares a tree of modules or interfaces with a symbol at
    /// the path the component name `component` spells (a module, or a function or global named by
    /// the raw last segment), which the component would shadow, or another component whose name
    /// nests with `component` (one is a segment-prefix of the other) while the shorter of the two
    /// is not declaration-only, or declares a symbol named by the last segment of a longer name
    /// one segment longer than its own: resolution prefers the longest registered name, so any of these would make
    /// paths ambiguous.
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
        // named by them nests with `component`, which `reject_nested_components` checks
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
    /// segment-prefix of `component`, or extends it, unless the shorter of the two is
    /// declaration-only (see [Component::is_declaration_only]) and, when the longer name is one
    /// segment longer, declares no symbol named by that last segment (`z` for `x::y` and
    /// `x::y::z`), whose path the longer component would capture. A deeper nesting captures
    /// nothing: `x::y::z` beside `x::y::z::w` still resolves to the leaf `z` of `x::y`.
    ///
    /// Raw segments are compared, not their text: `"a::b"` (one segment) and `a::b` (two) are
    /// different paths, in Miden Assembly as here.
    ///
    /// A component named `component` that the world does not hold yet counts as empty, i.e. as
    /// declaration-only.
    fn reject_nested_components(&self, component: SymbolName) -> Result<(), Report> {
        let body = self.body();
        if body.is_empty() {
            return Ok(());
        }
        for op in body.entry().body() {
            let Some(existing_op) = op.downcast_ref::<Component>() else {
                continue;
            };
            let existing = Symbol::name(existing_op);
            let (shorter, longer) = if existing.as_str().len() < component.as_str().len() {
                (existing, component)
            } else {
                (component, existing)
            };
            if !SymbolPath::nests_in(longer, shorter) {
                continue;
            }
            // Resolution picks the longest registered name, so a path under `longer` can only be
            // misrouted when `shorter` has a module tree of its own reaching that path, or a leaf
            // the longer name captures, since a path's trailing leaf joins the longest match
            let shorter_op = if shorter == existing {
                op.as_operation_ref()
            } else {
                let Some(symbol) = self.get(component) else {
                    continue;
                };
                symbol.borrow().as_symbol_operation().as_operation_ref()
            };
            let shorter_op = shorter_op.borrow();
            let Some(shorter_op) = shorter_op.downcast_ref::<Component>() else {
                continue;
            };
            if !shorter_op.is_declaration_only() {
                return Err(nested_components_error(component, existing));
            }
            if let Some(leaf) = captured_segment(shorter, longer)
                && shorter_op.symbol_manager().symbols().get_leaf(leaf).is_some()
            {
                return Err(captured_leaf_error(shorter, leaf, longer));
            }
        }
        Ok(())
    }

    /// Returns an error when a world-level component's name extends `component`, i.e. when
    /// `component` must stay declaration-only because a longer component nests in it.
    ///
    /// With `leaf`, the name of a declaration about to be added to `component`, only a component
    /// whose name is `component` followed by the one segment `leaf` (and would capture it) is an
    /// error.
    pub(crate) fn reject_definition_in_nesting_component(
        &self,
        component: SymbolName,
        leaf: Option<SymbolName>,
    ) -> Result<(), Report> {
        let body = self.body();
        if body.is_empty() {
            return Ok(());
        }
        for op in body.entry().body() {
            let Some(existing) = op.downcast_ref::<Component>().map(Symbol::name) else {
                continue;
            };
            if !SymbolPath::nests_in(existing, component) {
                continue;
            }
            match leaf {
                None => return Err(nested_components_error(component, existing)),
                Some(leaf) if captured_segment(component, existing) == Some(leaf) => {
                    return Err(captured_leaf_error(component, leaf, existing));
                }
                Some(_) => (),
            }
        }
        Ok(())
    }
}

/// Returns the last raw segment of `longer` when `longer` is exactly one segment longer than
/// `shorter`, which nests in it: the leaf of `shorter` whose path `longer` names.
fn captured_segment(shorter: SymbolName, longer: SymbolName) -> Option<SymbolName> {
    let longer = SymbolPath::segments_of(longer);
    (longer.len() == SymbolPath::segments_of(shorter).len() + 1).then(|| longer[longer.len() - 1])
}

/// The error for the declaration-only component `shorter` holding the symbol `leaf`, which the
/// component `longer`, whose name extends `shorter` by `leaf`, would capture.
fn captured_leaf_error(shorter: SymbolName, leaf: SymbolName, longer: SymbolName) -> Report {
    let mut segments = SymbolPath::segments_of(shorter);
    segments.push(leaf);
    let path = SymbolPath::spell(&segments);
    Report::msg(format!(
        "component `{shorter}` declares `{leaf}`, which component `{longer}` would capture: a \
         path `{path}` cannot name both"
    ))
}

/// The error for the components `component` and `existing`, whose names nest, where the shorter
/// of the two is not declaration-only.
fn nested_components_error(component: SymbolName, existing: SymbolName) -> Report {
    let (shorter, longer) = if existing.as_str().len() < component.as_str().len() {
        (existing, component)
    } else {
        (component, existing)
    };
    Report::msg(format!(
        "component `{component}` and component `{existing}` nest (`{shorter}` is a segment-prefix \
         of `{longer}`); a component namespace may only have another nested in it while it holds \
         declarations only"
    ))
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
