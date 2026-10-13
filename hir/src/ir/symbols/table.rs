use core::fmt;

use smallvec::SmallVec;

use super::{Symbol, SymbolName, SymbolNameComponent, SymbolPath, SymbolRef, generate_symbol_name};
use crate::{
    FxHashMap, FxHashSet, Op, Operation, OperationName, OperationRef, ProgramPoint, Report,
    UnsafeIntrusiveEntityRef, interner,
    traits::{GraphRegionNoTerminator, NoTerminator, Terminator},
};

/// A type alias for [SymbolTable] implementations referenced via [UnsafeIntrusiveEntityRef]
pub type SymbolTableRef = UnsafeIntrusiveEntityRef<dyn SymbolTable>;

/// A [SymbolTable] is an IR entity which contains other IR entities, called _symbols_, each of
/// which has a name, aka symbol, that uniquely identifies it amongst all other entities in the
/// same [SymbolTable].
///
/// The symbols in a [SymbolTable] do not need to all refer to the same entity type, however the
/// concrete value type of the symbol itself, e.g. `String`, must be the same. This is enforced
/// in the way that the [SymbolTable] and [Symbol] traits interact. A [SymbolTable] has an
/// associated `Key` type, and a [Symbol] has an associated `Id` type - only types whose `Id`
/// type matches the `Key` type of the [SymbolTable], can be stored in that table.
pub trait SymbolTable {
    /// Get a reference to the underlying [Operation]
    fn as_symbol_table_operation(&self) -> &Operation;

    /// Get a mutable reference to the underlying [Operation]
    fn as_symbol_table_operation_mut(&mut self) -> &mut Operation;

    /// Get a [SymbolManager] for this symbol table.
    fn symbol_manager(&self) -> SymbolManager<'_>;

    /// Get a [SymbolManagerMut] for this symbol table.
    fn symbol_manager_mut(&mut self) -> SymbolManagerMut<'_>;

    /// Get the entry for `name` in this table
    ///
    /// This is a raw lookup: if the entry is an alias (e.g. [`FunctionAlias`]), the alias itself is
    /// returned, not its target. See [SymbolTable::resolve_canonical].
    ///
    /// [`FunctionAlias`]: crate::dialects::builtin::FunctionAlias
    fn get(&self, name: SymbolName) -> Option<SymbolRef> {
        self.symbol_manager().lookup(name)
    }

    /// Resolve the entry for `path` in this table, or via the root symbol table
    ///
    /// This resolves the *named* symbol only: if it is an alias (e.g. [`FunctionAlias`]), the alias
    /// itself is returned, not its target. See [SymbolTable::resolve_canonical].
    ///
    /// [`FunctionAlias`]: crate::dialects::builtin::FunctionAlias
    fn resolve(&self, path: &SymbolPath) -> Option<SymbolRef> {
        let found = self.symbol_manager().lookup_symbol_ref(path)?;
        found.as_trait_ref::<dyn Symbol>()
    }

    /// Like [SymbolTable::resolve], but follows symbol aliases (e.g. [`FunctionAlias`]) to the
    /// canonical symbol.
    ///
    /// Each alias hop is resolved in the symbol table of the alias being followed. Returns
    /// `None` if
    ///
    /// * the path or any hop does not resolve
    /// * the alias chain is cyclic
    /// * the alias chain exceeds the maximum alias depth
    ///
    /// [`FunctionAlias`]: crate::dialects::builtin::FunctionAlias
    fn resolve_canonical(&self, path: &SymbolPath) -> Option<SymbolRef> {
        let found = self.resolve(path)?;
        found.resolve_canonical().ok()
    }

    /// Resolve a callable name to both the named symbol and its validated canonical target.
    ///
    /// Non-callable symbols are rejected and resolution failures retain their cause for
    /// diagnostics.
    fn resolve_callable(
        &self,
        path: &SymbolPath,
    ) -> Result<crate::ResolvedSymbolCallee, crate::SymbolResolutionError> {
        self.resolve(path)
            .ok_or_else(|| crate::SymbolResolutionError::UnknownSymbol { path: path.clone() })?
            .resolve_callable()
    }

    /// Insert `entry` in the symbol table, but only if no other symbol has the same name or the
    /// same path (see [SymbolMap::insert_new]).
    ///
    /// If provided, the symbol will be inserted at the given insertion point in the body of the
    /// symbol table operation.
    ///
    /// This function will panic if the symbol is attached to another symbol table.
    ///
    /// Returns `true` if successful, `false` if a symbol with the same name or path is already
    /// defined
    fn insert_new(&mut self, entry: SymbolRef, ip: ProgramPoint) -> bool {
        self.symbol_manager_mut().insert_new(entry, ip)
    }

    /// Like [SymbolTable::insert_new], except the symbol is renamed to avoid collisions.
    ///
    /// Returns the name of the symbol after insertion.
    fn insert(&mut self, entry: SymbolRef, ip: ProgramPoint) -> SymbolName {
        self.symbol_manager_mut().insert(entry, ip)
    }

    /// Remove the symbol `name`, and return the entry if one was present.
    ///
    /// This unlinks any current users of the removed symbol, as the removed name can no longer be
    /// resolved within this table.
    fn remove(&mut self, name: SymbolName) -> Option<SymbolRef> {
        let mut manager = self.symbol_manager_mut();

        let mut symbol = manager.lookup(name)?;
        manager.remove(symbol);
        symbol.borrow_mut().uses_mut().clear();
        Some(symbol)
    }

    /// Renames the symbol named `from`, as `to`, as well as all uses of that symbol.
    ///
    /// Returns `Err` if unable to update all uses.
    ///
    /// # Panics
    ///
    /// This function will panic if no operation named `from` exists in this symbol table.
    fn rename(&mut self, from: SymbolName, to: SymbolName) -> Result<(), Report> {
        let mut manager = self.symbol_manager_mut();

        let symbol = manager.lookup(from).unwrap_or_else(|| panic!("undefined symbol '{from}'"));
        manager.rename_symbol(symbol, to)
    }
}

impl dyn SymbolTable {
    /// Get an [OperationRef] for the operation underlying this symbol table
    ///
    /// NOTE: This relies on the assumption that all ops are allocated via the arena, and that all
    /// [SymbolTable] implementations are ops.
    pub fn as_operation_ref(&self) -> OperationRef {
        self.as_symbol_table_operation().as_operation_ref()
    }

    /// Look up a symbol with the given name and concrete type, returning `None` if no such symbol
    /// exists
    pub fn find<T: Op + Symbol>(&self, name: SymbolName) -> Option<UnsafeIntrusiveEntityRef<T>> {
        let symbol = self.get(name)?;
        symbol
            .borrow()
            .as_symbol_operation()
            .as_operation_ref()
            .try_downcast_op::<T>()
            .ok()
    }

    /// Returns an error naming the symbol that keeps a symbol named `name`, an op of `kind`, from
    /// being defined in this table: a symbol with that name, or a symbol of the other kind
    /// (symbol table or not) with the path the new symbol would have (see
    /// [SymbolMap::insert_new]).
    pub fn can_define(&self, name: SymbolName, kind: &OperationName) -> Result<(), SymbolConflict> {
        let is_table = kind.implements::<dyn SymbolTable>();
        match self.symbol_manager().symbols().conflict(name, is_table) {
            None => Ok(()),
            Some(existing) => Err(SymbolConflict::new(kind.name(), name, &existing)),
        }
    }
}

/// A symbol that cannot be defined in a symbol table, because another symbol there has its name,
/// or the same path (see [SymbolTable::insert_new]).
#[derive(Debug, Clone)]
pub struct SymbolConflict {
    /// The opcode of the symbol that cannot be defined, e.g. `module`
    pub kind: interner::Symbol,
    /// The name of the symbol that cannot be defined
    pub name: SymbolName,
    /// The opcode of the symbol already defined
    pub existing_kind: interner::Symbol,
    /// The name of the symbol already defined
    pub existing_name: SymbolName,
    /// The path of the symbol already defined, which the new symbol would have too
    pub path: SymbolPath,
}

impl SymbolConflict {
    /// The conflict of a symbol named `name`, an op with the opcode `kind`, with `existing`.
    ///
    /// # Panics
    ///
    /// This function computes the path of `existing`, so it panics when an ancestor of `existing`
    /// is mutably borrowed (see [Symbol::path]).
    pub fn new(kind: interner::Symbol, name: SymbolName, existing: &SymbolRef) -> Self {
        let existing = existing.borrow();
        Self {
            kind,
            name,
            existing_kind: existing.as_symbol_operation().name().name(),
            existing_name: existing.name(),
            path: existing.path(),
        }
    }
}

impl fmt::Display for SymbolConflict {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.name == self.existing_name {
            return write!(f, "redefinition of symbol {}", self.name);
        }
        write!(
            f,
            "`{} {}` and `{} {}` have the same path `{}`",
            self.kind, self.name, self.existing_kind, self.existing_name, self.path
        )
    }
}

impl core::error::Error for SymbolConflict {}

/// A [SymbolMap] is a low-level datastructure used in implementing a [SymbolTable] operation.
///
/// It is primarily responsible for maintaining a mapping between symbol names, and the symbol
/// operations registered to those names, within the body of the containing [SymbolTable] op.
///
/// In most circumstances, you will want to interact with this via [SymbolManager] or
/// [SymbolManagerMut], as the operations provided here are mostly low-level plumbing, and thus
/// incomplete without functionality provided by higher-level abstractions.
#[derive(Debug)]
pub struct SymbolMap {
    /// A low-level mapping of symbols to operations found in this table
    symbols: FxHashMap<SymbolName, SymbolRef>,
    /// The names in `symbols` registered to symbol-table ops, which are named by the Miden
    /// Assembly spelling of their raw segments, while any other symbol is named by its raw name
    tables: FxHashSet<SymbolName>,
    /// Used to unique symbol names when conflicts are detected
    uniquing_count: usize,
    /// The largest number of segments of the name of any symbol table registered in this map, each
    /// name read as the Miden Assembly spelling of its segments (see [SymbolPath::segments_of]).
    ///
    /// This is only an upper bound (it never decreases on removal) that keeps the common case of
    /// single-segment names at one lookup per path component in [SymbolMap::lookup_prefix].
    max_name_segments: usize,
}
/// An empty map, whose `max_name_segments` starts at 1: every name has at least one segment, so
/// [SymbolMap::lookup_prefix] tries no multi-segment names until one is registered.
impl Default for SymbolMap {
    fn default() -> Self {
        Self {
            symbols: Default::default(),
            tables: Default::default(),
            uniquing_count: 0,
            max_name_segments: 1,
        }
    }
}
impl SymbolMap {
    /// Build a [SymbolMap] on the fly from the given operation.
    ///
    /// It is assumed that the given operation is a [SymbolTable] op, but this is not checked, and
    /// does not affect the correctness - however, it has limited utility for non-symbol table ops.
    pub fn build(op: &Operation) -> Self {
        Self::try_build(op).expect("expected region to contain uniquely named symbol operations")
    }

    /// Like [`SymbolMap::build`], but reports the conflict instead of panicking when two symbols
    /// have the same name or the same path (see [SymbolMap::insert_new]).
    ///
    /// Callers handling parsed text must use this: a duplicate there is something the author
    /// wrote, not a broken invariant. An operation with no region, or whose region has no blocks,
    /// defines no symbols and yields an empty map.
    ///
    /// # Panics
    ///
    /// Reporting a conflict computes the path of a symbol in `op` (see [SymbolConflict::new]), so
    /// `op` must not be mutably borrowed.
    pub fn try_build(op: &Operation) -> Result<Self, SymbolConflict> {
        let mut map = Self::default();

        let Some(region) = op.regions().front().get() else {
            return Ok(map);
        };
        if !region.is_empty() {
            for op in region.entry().body() {
                if let Some(symbol_ref) = op.as_symbol_ref() {
                    let name = symbol_ref.borrow().name();
                    let is_table = is_symbol_table(&symbol_ref);
                    if let Some(existing) = map.conflict(name, is_table) {
                        return Err(SymbolConflict::new(op.name().name(), name, &existing));
                    }
                    map.register(name, symbol_ref, is_table);
                }
            }
        }

        Ok(map)
    }

    /// Registers `symbol` as `name`, a symbol table when `is_table`, see [SymbolMap::tables] and
    /// [SymbolMap::max_name_segments].
    fn register(&mut self, name: SymbolName, symbol: SymbolRef, is_table: bool) {
        self.symbols.insert(name, symbol);
        if !is_table {
            // Only a symbol table can match a run of path components
            return;
        }
        self.tables.insert(name);
        // Bounds the segments `SymbolPath::segments_of` would produce, without interning them; a
        // quoted segment holding `::` makes this an overcount, which only costs a lookup
        let segments = name.as_str().matches("::").count() + 1;
        self.max_name_segments = self.max_name_segments.max(segments);
    }

    /// Returns the name under which a symbol of the other kind than a symbol named `name` (a
    /// symbol table when `is_table`) would have the same path.
    ///
    /// A symbol table named by one segment that needs quoting (`"my-mod"`) and a plain symbol
    /// named by that raw segment (`my-mod`) both have the path `[Leaf(my-mod)]` (see
    /// [Symbol::path]), although their names differ.
    fn path_alias(name: SymbolName, is_table: bool) -> Option<SymbolName> {
        if is_table {
            match SymbolPath::segments_of(name).as_slice() {
                [segment] if *segment != name => Some(*segment),
                _ => None,
            }
        } else {
            SymbolPath::spell_segment(name).filter(|spelled| *spelled != name)
        }
    }

    /// Returns the symbol that keeps a symbol named `name`, a symbol table when `is_table`, from
    /// being registered in this map: the symbol with that name, or the symbol of the other kind
    /// with the same path (see [SymbolMap::path_alias]).
    fn conflict(&self, name: SymbolName, is_table: bool) -> Option<SymbolRef> {
        if let Some(existing) = self.symbols.get(&name) {
            return Some(*existing);
        }
        // Without symbol tables, no symbol has the path of a plain symbol under another name;
        // skipping the alias spares spelling (and interning) the name
        if !is_table && self.tables.is_empty() {
            return None;
        }
        let alias = Self::path_alias(name, is_table)?;
        // A symbol of the same kind named `alias` has another path: a plain symbol named
        // `"my-mod"` has the leaf `"my-mod"`, a symbol table named `my-mod` the leaf `my-mod`
        self.symbols
            .get(&alias)
            .filter(|_| self.tables.contains(&alias) != is_table)
            .copied()
    }

    /// Returns true when a symbol named `name`, a symbol table when `is_table`, can be registered
    /// in this map (see [SymbolMap::conflict]).
    fn is_free(&self, name: SymbolName, is_table: bool) -> bool {
        self.conflict(name, is_table).is_none()
    }

    /// Get the symbol named `name`, or `None` if undefined.
    pub fn get(&self, name: impl Into<SymbolName>) -> Option<SymbolRef> {
        let name = name.into();
        self.symbols.get(&name).cloned()
    }

    /// Get the symbol named `name` as an [OperationRef], or `None` if undefined.
    pub fn get_op(&self, name: impl Into<SymbolName>) -> Option<OperationRef> {
        let name = name.into();
        self.symbols.get(&name).map(|symbol| symbol.borrow().as_operation_ref())
    }

    /// Get the symbol referenced by `attr` as an [OperationRef], or `None` if undefined.
    ///
    /// This function will search for the symbol path according to whether the path is absolute or
    /// relative:
    ///
    /// * Absolute paths will be resolved by traversing up the operation tree to the root operation,
    ///   which will be expected to be an anonymous SymbolTable, and then resolve path components
    ///   from there.
    /// * Relative paths will be resolved from the current SymbolTable
    ///
    /// In the special case where a absolute path is given, but the root operation is also a Symbol,
    /// it is presumed that what we have found is not the absolute root which represents the global
    /// namespace, but rather a symbol defined in the global namespace. This means that only
    /// children of that symbol are possibly resolvable (as we have no way to reach other symbols
    /// defined in the global namespace). In short, we only attempt to resolve absolute paths whose
    /// leading components are the raw segments of the root symbol's name, the Miden Assembly
    /// spelling of those segments (see [SymbolPath::segments_of]), the last one possibly as the
    /// leaf. If they are, then the symbol is resolved normally from there, otherwise `None` is
    /// returned.
    ///
    /// Path components are matched against symbol names as described in
    /// [SymbolMap::lookup_prefix].
    pub fn resolve(&self, symbol_table: &Operation, attr: &SymbolPath) -> Option<OperationRef> {
        let components = attr.components().collect::<SmallVec<[SymbolNameComponent; 4]>>();

        // Resolve absolute paths via the root symbol table
        if attr.is_absolute() {
            let mut components = &components[1..];

            // Locate the root operation
            let root = if let Some(mut root) = symbol_table.parent_op() {
                while let Some(ancestor) = root.borrow().parent_op() {
                    root = ancestor;
                }
                root
            } else {
                symbol_table.as_operation_ref()
            };

            let root_op = root.borrow();

            // If the root is also a Symbol, then we aren't actually in the root namespace, but
            // in one of the symbols within the root namespace. As a result, we can only resolve
            // absolute symbol paths which are children of `root`, as we cannot reach any other
            // symbols in the root namespace.
            if let Some(root_symbol) = root_op.as_trait::<dyn Symbol>() {
                // The raw segments of the root symbol's name are matched, the last one possibly
                // as the leaf (as `Symbol::path` gives it)
                let segments = SymbolPath::segments_of(root_symbol.name());
                for (i, segment) in segments.iter().enumerate() {
                    match components.get(i)? {
                        SymbolNameComponent::Component(name) if name == segment => {}
                        // The path ends at the root symbol, spelled segment by segment
                        SymbolNameComponent::Leaf(name)
                            if name == segment
                                && i + 1 == segments.len()
                                && i + 1 == components.len() =>
                        {
                            return Some(root);
                        }
                        _ => return None,
                    }
                }
                components = &components[segments.len()..];
            }

            // Resolve the symbol from `root`
            let root_symbol_table = root_op.as_trait::<dyn SymbolTable>()?;
            let symbol_manager = root_symbol_table.symbol_manager();
            symbol_manager.symbols().resolve_components(components)
        } else {
            self.resolve_components(&components)
        }
    }

    pub fn resolve_all_in(
        &self,
        _symbol_table: &Operation,
        _attr: &SymbolPath,
        _nested_references: &mut SmallVec<[OperationRef; 4]>,
    ) -> Option<OperationRef> {
        todo!()
    }

    /// Resolves the relative path `components` starting from this map, descending one nested
    /// symbol table per [SymbolMap::lookup_prefix] match.
    fn resolve_components(&self, components: &[SymbolNameComponent]) -> Option<OperationRef> {
        let (mut found, mut rest) = self.lookup_prefix(components)?;
        while !rest.is_empty() {
            let next = {
                let op = found.borrow();
                let symbol_table = op.as_trait::<dyn SymbolTable>()?;
                symbol_table.symbol_manager().symbols().lookup_prefix(rest)?
            };
            (found, rest) = next;
        }
        Some(found)
    }

    /// Looks up the symbol named by the leading components of `components` in this map, and
    /// returns it together with the components that remain to be resolved inside it.
    ///
    /// A leading `Leaf` names the symbol whose path relative to this map it is (see
    /// [SymbolMap::get_leaf]).
    ///
    /// Otherwise, the lookup matches the **longest run of leading `Component`s whose Miden
    /// Assembly spelling is the registered name of a symbol table**, since a symbol-table op may
    /// be named by a multi-segment path (see [SymbolPath::segments_of]). For example, a component
    /// named `miden::a::b` coexists with a module tree rooted at `miden` in the same table; the
    /// path `miden::a::b::m::f` resolves through the component, because the longest registered
    /// name wins. The run may end in the `Leaf` holding the raw last segment of the symbol table
    /// the path names. A component Miden Assembly cannot spell names no symbol table, so it ends
    /// the run.
    fn lookup_prefix<'a>(
        &self,
        components: &'a [SymbolNameComponent],
    ) -> Option<(OperationRef, &'a [SymbolNameComponent])> {
        match components.first()? {
            SymbolNameComponent::Leaf(name) => Some((self.get_leaf(*name)?, &components[1..])),
            SymbolNameComponent::Component(_) => {
                // Only raw segments are spelled: a quoted raw segment (`"my-mod"`) would
                // otherwise pass through as the spelling of the segment it quotes
                let spellable =
                    |c: &SymbolNameComponent| SymbolPath::is_spellable(c.as_symbol_name().as_str());
                let run = components
                    .iter()
                    .take_while(|c| matches!(c, SymbolNameComponent::Component(_)) && spellable(c))
                    .count();
                if run == 0 {
                    return None;
                }
                // A symbol table's own path ends in the leaf holding its last segment
                let run = run
                    + usize::from(components.get(run).is_some_and(|c| c.is_leaf() && spellable(c)));
                // Longest first, down to the name of the first segment alone. Only a symbol table
                // can have the remaining components resolved inside it; any other symbol whose
                // opaque name happens to spell the joined path must not capture the lookup.
                (1..=run.min(self.max_name_segments)).rev().find_map(|len| {
                    let name = SymbolPath::join_components(&components[..len]);
                    if !self.tables.contains(&name) {
                        return None;
                    }
                    Some((self.get_op(name)?, &components[len..]))
                })
            }
            SymbolNameComponent::Root => {
                unreachable!("root component should have already been consumed")
            }
        }
    }

    /// Returns the symbol whose path relative to this map is the lone leaf `name` (see
    /// [Symbol::path]): a plain symbol named `name`, or a symbol table whose one raw segment is
    /// `name`.
    ///
    /// Leaf names are opaque, even when they contain `::`, so a plain symbol is looked up as is.
    /// A symbol table is named by the Miden Assembly spelling of its raw segment instead, which
    /// is `name` itself when it is a bare identifier; a leaf Miden Assembly cannot spell names no
    /// symbol table. Each lookup only accepts its kind of symbol: a plain symbol whose opaque
    /// name happens to be a spelling (`"my-mod"`, `a::b`) must not capture the path of a symbol
    /// table, nor the other way around.
    pub(crate) fn get_leaf(&self, name: SymbolName) -> Option<OperationRef> {
        if !self.tables.contains(&name)
            && let Some(op) = self.get_op(name)
        {
            return Some(op);
        }
        let spelled = SymbolPath::spell_segment(name)?;
        if !self.tables.contains(&spelled) {
            return None;
        }
        self.get_op(spelled)
    }

    /// Returns true if a symbol named `name` is in the map
    #[inline]
    pub fn contains_key(&self, name: impl Into<SymbolName>) -> bool {
        self.symbols.contains_key(&name.into())
    }

    /// Remove the entry for `name` from this map, if present.
    #[inline]
    pub fn remove(&mut self, name: SymbolName) -> Option<SymbolRef> {
        self.tables.remove(&name);
        self.symbols.remove(&name)
    }

    /// Inserts `symbol` in the map, as `name`, so long as no symbol has `name` or the path a
    /// symbol named `name` would have (a symbol table named `"my-mod"` and a function named
    /// `my-mod` share the path `my-mod`, see [Symbol::path]).
    pub fn insert_new(&mut self, name: SymbolName, symbol: SymbolRef) -> bool {
        let is_table = is_symbol_table(&symbol);
        let inserted = self.is_free(name, is_table);
        if inserted {
            self.register(name, symbol, is_table);
        }
        inserted
    }

    /// Inserts `symbol` in the map, with `name` if [SymbolMap::insert_new] would accept that
    /// name. Otherwise, a unique variation of `name` is generated, and `symbol` is inserted in
    /// the map with that name instead.
    ///
    /// If `name` is modified to make it unique, `symbol` is updated with the new name on insertion.
    ///
    /// Returns the name `symbol` has after insertion.
    ///
    /// NOTE: If `symbol` is already in the map with `name`, this is a no-op.
    ///
    /// Unsupported: a uniqued name is not checked against the nesting rules of a world (see
    /// [SymbolMap::make_unique]).
    pub fn insert(&mut self, name: SymbolName, mut symbol: SymbolRef) -> SymbolName {
        // If this exact symbol was already in the table, do nothing
        if self.symbols.get(&name) == Some(&symbol) {
            assert_eq!(
                symbol.borrow().name(),
                name,
                "name does not match what was registered with the symbol table"
            );
            return name;
        }

        let is_table = is_symbol_table(&symbol);
        let name = if self.is_free(name, is_table) {
            name
        } else {
            let mut counter = self.uniquing_count;
            let uniqued = generate_unique_name(is_table, name, &mut counter, |name| {
                self.is_free(name, is_table)
            });
            self.uniquing_count = counter;
            uniqued
        };
        // The uses of a uniqued symbol are not rewritten: a symbol is inserted before it is used,
        // and `SymbolManagerMut::rename_symbol` rewrites the uses of a symbol renamed afterwards
        symbol.borrow_mut().set_name(name);
        self.register(name, symbol, is_table);
        name
    }

    /// Ensures that the given symbol name is unique within this symbol map, as well as all of the
    /// provided symbol managers.
    ///
    /// Returns the unique name, but this function does not modify the map or rename the symbol
    /// itself, that is expected to be done from [SymbolManagerMut].
    ///
    /// Unsupported: like [SymbolMap::insert], uniquing only consults the symbol maps, not the
    /// nesting rules of a world (see `World::reject_component_shadowing`), so a component `a::b`
    /// may be uniqued to `a::b_1` beside a component `a::b_1::x`.
    pub fn make_unique(&mut self, op: &SymbolRef, tables: &[SymbolManager<'_>]) -> SymbolName {
        // Determine new name that is unique in all symbol tables.
        let name = { op.borrow().name() };
        let is_table = is_symbol_table(op);

        let mut counter = self.uniquing_count;
        let uniqued = generate_unique_name(is_table, name, &mut counter, |name| {
            self.is_free(name, is_table) && tables.iter().all(|t| t.symbols.is_free(name, is_table))
        });
        self.uniquing_count = counter;
        uniqued
    }

    /// Get an iterator of [SymbolRef] corresponding to the [Symbol] operations in this map
    pub fn symbols(&self) -> impl Iterator<Item = SymbolRef> + '_ {
        self.symbols.values().cloned()
    }
}

/// Returns true when `symbol` is a symbol-table op.
fn is_symbol_table(symbol: &SymbolRef) -> bool {
    symbol.borrow().as_symbol_operation().implements::<dyn SymbolTable>()
}

/// Returns a variation of `name`, the name of a symbol (a symbol table when `is_table`), that
/// `is_unique` accepts (see [generate_symbol_name]).
///
/// The name of a symbol-table op is the Miden Assembly spelling of its raw segments (see
/// [SymbolPath::segments_of]), so the suffix goes on its raw last segment and the result is
/// spelled again: `"masm-dep"` becomes `"masm-dep_1"`, not `"masm-dep"_1`.
fn generate_unique_name<F>(
    is_table: bool,
    name: SymbolName,
    counter: &mut usize,
    is_unique: F,
) -> SymbolName
where
    F: Fn(SymbolName) -> bool,
{
    if !is_table {
        return generate_symbol_name(name, counter, is_unique);
    }
    let mut segments = SymbolPath::segments_of(name);
    let last = segments.len() - 1;
    let spell = |segment: SymbolName| {
        let mut segments = segments.clone();
        segments[last] = segment;
        SymbolPath::spell(&segments)
    };
    let segment =
        generate_symbol_name(segments[last], counter, |candidate| is_unique(spell(candidate)));
    segments[last] = segment;
    SymbolPath::spell(&segments)
}

/// This type is used to abstract over ownership of an immutable [SymbolMap].
pub enum Symbols<'a> {
    /// The symbol map is owned by this struct, typically because the operation to which it
    /// ostensibly belongs did not have one for us, so we were forced to compute the symbol
    /// mapping for that operation on the fly.
    Owned(SymbolMap),
    /// The symbol map is being borrowed (typically from the [SymbolTable] operation)
    Borrowed(&'a SymbolMap),
}
impl From<SymbolMap> for Symbols<'_> {
    fn from(value: SymbolMap) -> Self {
        Self::Owned(value)
    }
}
impl core::ops::Deref for Symbols<'_> {
    type Target = SymbolMap;

    fn deref(&self) -> &Self::Target {
        match self {
            Self::Owned(symbols) => symbols,
            Self::Borrowed(symbols) => symbols,
        }
    }
}

/// This type is used to abstract over ownership of an immutable [SymbolMap].
pub enum SymbolsMut<'a> {
    /// The symbol map is owned by this struct, typically because the operation to which it
    /// ostensibly belongs did not have one for us, so we were forced to compute the symbol
    /// mapping for that operation on the fly.
    Owned(SymbolMap),
    /// The symbol map is being borrowed (typically from the [SymbolTable] operation)
    Borrowed(&'a mut SymbolMap),
}
impl From<SymbolMap> for SymbolsMut<'_> {
    fn from(value: SymbolMap) -> Self {
        Self::Owned(value)
    }
}
impl core::ops::Deref for SymbolsMut<'_> {
    type Target = SymbolMap;

    fn deref(&self) -> &Self::Target {
        match self {
            Self::Owned(symbols) => symbols,
            Self::Borrowed(symbols) => symbols,
        }
    }
}
impl core::ops::DerefMut for SymbolsMut<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        match self {
            Self::Owned(symbols) => symbols,
            Self::Borrowed(symbols) => symbols,
        }
    }
}

/// This type provides high-level read-only symbol table operations for [SymbolTable] impls.
///
/// It is designed to be able to handle both dynamically-computed symbol table mappings, or use
/// cached mappings provided by the [SymbolTable] op itself.
///
/// See [SymbolManagerMut] for read/write use cases.
pub struct SymbolManager<'a> {
    /// The [SymbolTable] operation we're managing
    symbol_table: &'a Operation,
    /// The symbols registered under `symbol_table`.
    ///
    /// This information can either be computed dynamically, or cached by the operation itself.
    symbols: Symbols<'a>,
}

impl<'a> SymbolManager<'a> {
    /// Create a new [SymbolManager] from the given operation and symbol mappings
    pub fn new(symbol_table: &'a Operation, symbols: Symbols<'a>) -> Self {
        Self {
            symbol_table,
            symbols,
        }
    }

    /// Returns true if this symbol table corresponds to the root namespace
    pub fn is_root(&self) -> bool {
        self.symbol_table.parent().is_none()
    }

    /// Returns a reference to the underlying symbol table [Operation]
    pub fn symbol_table(&self) -> &Operation {
        self.symbol_table
    }

    pub fn symbols(&self) -> &SymbolMap {
        &self.symbols
    }

    /// Get the symbol named `name`, or `None` if undefined.
    pub fn lookup(&self, name: impl Into<SymbolName>) -> Option<SymbolRef> {
        self.symbols.get(name)
    }

    /// Get the symbol named `name` as an [OperationRef], or `None` if undefined.
    pub fn lookup_op(&self, name: impl Into<SymbolName>) -> Option<OperationRef> {
        self.symbols.get_op(name)
    }

    /// Get the symbol referenced by `attr` as an [OperationRef], or `None` if undefined.
    ///
    /// See [SymbolMap::resolve] for more details about symbol resolution.
    pub fn lookup_symbol_ref(&self, attr: &SymbolPath) -> Option<OperationRef> {
        self.symbols.resolve(self.symbol_table, attr)
    }
}

impl<'a> From<&'a Operation> for SymbolManager<'a> {
    fn from(symbol_table: &'a Operation) -> Self {
        Self {
            symbol_table,
            symbols: SymbolMap::build(symbol_table).into(),
        }
    }
}

/// This type provides high-level read and write symbol table operations for [SymbolTable] impls.
///
/// It is designed to be able to handle both dynamically-computed symbol table mappings, or use
/// cached mappings provided by the [SymbolTable] op itself.
pub struct SymbolManagerMut<'a> {
    /// The [SymbolTable] operation we're managing
    symbol_table: &'a mut Operation,
    /// The symbols registered under `symbol_table`.
    ///
    /// This information can either be computed dynamically, or cached by the operation itself.
    symbols: SymbolsMut<'a>,
}
impl<'a> SymbolManagerMut<'a> {
    /// Create a new [SymbolManager] from the given operation and symbol mappings
    pub fn new(symbol_table: &'a mut Operation, symbols: SymbolsMut<'a>) -> Self {
        Self {
            symbol_table,
            symbols,
        }
    }

    pub(crate) fn symbols_mut<'b: 'a, 'this: 'b>(&'this mut self) -> &'this mut SymbolsMut<'b> {
        &mut self.symbols
    }

    /// Returns an immutable reference to the underlying symbol table [Operation]
    ///
    /// NOTE: This requires a mutable reference to `self`, because the underlying [Operation]
    /// reference is a mutable one.
    pub fn symbol_table(&mut self) -> &Operation {
        self.symbol_table
    }

    /// Returns a mutable reference to the underlying symbol table [Operation]
    pub fn symbol_table_mut(&mut self) -> &mut Operation {
        self.symbol_table
    }

    /// Get the symbol named `name`, or `None` if undefined.
    pub fn lookup(&self, name: impl Into<SymbolName>) -> Option<SymbolRef> {
        self.symbols.get(name)
    }

    /// Get the symbol named `name` as an [OperationRef], or `None` if undefined.
    pub fn lookup_op(&self, name: impl Into<SymbolName>) -> Option<OperationRef> {
        self.symbols.get_op(name)
    }

    /// Get the symbol referenced by `attr` as an [OperationRef], or `None` if undefined.
    ///
    /// See [SymbolMap::resolve] for more details about symbol resolution.
    pub fn lookup_symbol_ref(&self, attr: &SymbolPath) -> Option<OperationRef> {
        self.symbols.resolve(self.symbol_table, attr)
    }

    /// Remove the given [Symbol] op from the table
    ///
    /// NOTE: This does not remove users of `op`'s symbol, that is left up to callers
    pub fn remove(&mut self, op: SymbolRef) {
        let name = {
            let symbol = op.borrow();
            let symbol_op = symbol.as_operation_ref();
            assert_eq!(
                symbol_op.borrow().parent_op(),
                Some(self.symbol_table.as_operation_ref()),
                "expected `op` to be a child of this symbol table"
            );
            symbol.name()
        };

        self.symbols.remove(name);
    }

    /// Inserts a new symbol into the table, as long as no symbol has its name or its path (see
    /// [SymbolMap::insert_new]).
    ///
    /// Returns `false` if a symbol with the same name or path is already in the table.
    ///
    /// # Panics
    ///
    /// This function will panic if `symbol` is already attached to another operation.
    pub fn insert_new(&mut self, symbol: SymbolRef, ip: ProgramPoint) -> bool {
        let name = symbol.borrow().name();
        let is_table = is_symbol_table(&symbol);
        if !self.symbols.is_free(name, is_table) {
            return false;
        }

        self.place(symbol, ip);
        self.symbols.register(name, symbol, is_table);

        true
    }

    /// Insert a new symbol into the table, renaming it as necessary to avoid name collisions.
    ///
    /// If `ip` is provided, the operation will be inserted at the specified program point.
    /// Otherwise, the new symbol is inserted at the end of the body of the symbol table op.
    ///
    /// Returns the name of the symbol after insertion, which may not be the same as its original
    /// name.
    ///
    /// # Panics
    ///
    /// This function will panic if `symbol` is already attached to another operation.
    pub fn insert(&mut self, symbol: SymbolRef, ip: ProgramPoint) -> SymbolName {
        let name = self.place(symbol, ip);
        // Add the symbol to the symbol map
        self.symbols.insert(name, symbol)
    }

    /// Makes `symbol` a child of the symbol table op, at `ip` if set, unless it already is one,
    /// and returns its name. The symbol map is left to the caller.
    ///
    /// # Panics
    ///
    /// This function will panic if `symbol` is already attached to another operation.
    fn place(&mut self, symbol: SymbolRef, mut ip: ProgramPoint) -> SymbolName {
        // The symbol cannot be the child of another op, and must be the child of the symbol table
        // after insertion.
        let (name, symbol_op) = {
            let sym = symbol.borrow();
            let symbol_op = sym.as_operation_ref();
            assert!(
                symbol_op
                    .borrow()
                    .parent_op()
                    .is_none_or(|p| p == self.symbol_table.as_operation_ref()),
                "symbol is already inserted in another op"
            );
            (sym.name(), symbol_op)
        };

        if symbol_op.borrow().parent().is_none() {
            let requires_terminator = !self.symbol_table.implements::<dyn NoTerminator>()
                && !self.symbol_table.implements::<dyn GraphRegionNoTerminator>();
            let mut body = self.symbol_table.region_mut(0);
            let mut block = body.entry_mut();

            // If no terminator is required in the symbol table body, simply insert it at the
            if requires_terminator {
                let block_terminator = block
                    .terminator()
                    .expect("symbol table op requires a terminator, but one was not found");

                if ip.is_unset() {
                    let ops = block.body_mut();
                    unsafe {
                        let mut cursor = ops.cursor_mut_from_ptr(block_terminator);
                        cursor.insert_before(symbol_op);
                    }
                } else {
                    let ip_block = ip.block();
                    assert_eq!(
                        ip_block,
                        Some(block.as_block_ref()),
                        "invalid insertion point: not located in this symbol table"
                    );
                    if ip.is_at_block_end() {
                        // If the insertion point would place the symbol after the region terminator
                        // it must be itself a valid region terminator, or the insertion point is
                        // not valid
                        assert!(
                            symbol_op.borrow().implements::<dyn Terminator>(),
                            "cannot insert symbol after the region terminator"
                        );
                    }
                    ip.cursor_mut().unwrap().insert_after(symbol_op);
                }
            } else if ip.is_unset() {
                block.body_mut().push_back(symbol_op);
            } else {
                let ip_block = ip.block();
                assert_eq!(
                    ip_block,
                    Some(block.as_block_ref()),
                    "invalid insertion point: not located in this symbol table"
                );
                ip.cursor_mut().unwrap().insert_after(symbol_op);
            }
        }

        name
    }

    /// Renames the given operation, and updates the symbol table and all uses of the old name.
    ///
    /// `to` is the name as stored, see [Symbol::set_name].
    ///
    /// Returns `Err` if not all uses could be updated.
    pub fn rename_symbol(&mut self, mut op: SymbolRef, to: SymbolName) -> Result<(), Report> {
        let name = {
            let symbol = op.borrow();
            let name = symbol.name();
            let symbol_op = symbol.as_symbol_operation();
            assert!(
                symbol_op
                    .parent_op()
                    .is_some_and(|parent| parent == self.symbol_table.as_operation_ref()),
                "expected operation to be a child of this symbol table"
            );
            assert!(
                self.lookup(name).as_ref().is_some_and(|o| o == &op),
                "current name does not resolve to `op`"
            );
            assert!(
                self.symbols.is_free(to, is_symbol_table(&op)),
                "new symbol name given by `to` is already in use"
            );
            name
        };

        // Rename the name stored in all users of `op`
        self.replace_all_symbol_uses(op, to)?;

        // Remove op with old name, change name, add with new name.
        //
        // The order is important here due to how `remove` and `insert` rely on the op name.
        self.remove(op);
        {
            op.borrow_mut().set_name(to);
        }
        self.insert(op, ProgramPoint::default());

        assert!(
            self.lookup(to).is_some_and(|o| o == op),
            "new name does not resolve to renamed op"
        );
        assert!(!self.symbols.contains_key(name), "old name still exists");

        Ok(())
    }

    /// Replaces the symbol name stored in all uses of the symbol `op`.
    ///
    /// `to` is the name as stored, see [Symbol::set_name]. The name of a symbol-table op ends a
    /// path as its raw segments (see [Symbol::path]), so those segments of each use are replaced
    /// by the raw segments of `to`.
    ///
    /// NOTE: This is not the same as replacing uses of one symbol with another, this used while
    /// renaming the symbol name of `op`, while preserving its uses.
    ///
    /// Unsupported: only the direct uses of `op` are rewritten. A use of a symbol nested in a
    /// renamed symbol table (`::miden::a::b::m::f` after renaming the component `miden::a::b`)
    /// keeps the old path, and no longer resolves.
    pub fn replace_all_symbol_uses(&mut self, op: SymbolRef, to: SymbolName) -> Result<(), Report> {
        let symbol = op.borrow();
        // For a symbol table, the number of trailing components naming it in a path, and the
        // components replacing them
        let table_segments =
            symbol.as_symbol_operation().implements::<dyn SymbolTable>().then(|| {
                let mut new = SymbolPath::segments_of(to)
                    .into_iter()
                    .map(SymbolNameComponent::Component)
                    .collect::<SmallVec<[_; 3]>>();
                if let Some(last) = new.last_mut() {
                    *last = SymbolNameComponent::Leaf(last.as_symbol_name());
                }
                (SymbolPath::segments_of(symbol.name()).len(), new)
            });

        // Visit all users of `symbol`, and rewrite the name used with `to`
        let mut users = symbol.uses().front();
        while let Some(user) = users.as_pointer() {
            users.move_next();

            let mut attr = user.borrow().attr;
            let mut attr = attr.borrow_mut();
            let path = attr.path_mut();
            match &table_segments {
                None => path.set_name(to),
                Some((old_len, new)) => {
                    let path = &mut path.path;
                    // Keep what precedes the components naming `op`, and at least the root of an
                    // absolute path, so the rewritten use stays absolute
                    let keep =
                        path.len().saturating_sub(*old_len).max(usize::from(path[0].is_root()));
                    path.truncate(keep);
                    path.extend(new.iter().copied());
                }
            }
        }

        Ok(())
    }

    /// Renames the given operation to a name that is unique within this and all of the provided
    /// symbol tables, updating the symbol table and all uses of the old name.
    ///
    /// Returns the new name, or `Err` if renaming fails.
    pub fn make_unique(
        &mut self,
        op: SymbolRef,
        tables: &[SymbolManager<'_>],
    ) -> Result<SymbolName, Report> {
        // Determine new name that is unique in all symbol tables.
        let uniqued = self.symbols.make_unique(&op, tables);

        // Rename the symbol to the new name
        self.rename_symbol(op, uniqued)?;

        Ok(uniqued)
    }
}

impl<'a> From<&'a mut Operation> for SymbolManagerMut<'a> {
    fn from(symbol_table: &'a mut Operation) -> Self {
        let symbols = SymbolMap::build(&*symbol_table).into();
        Self {
            symbol_table,
            symbols,
        }
    }
}
