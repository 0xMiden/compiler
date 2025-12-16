use super::{
    Symbol, SymbolName, SymbolNameComponent, SymbolPath, SymbolPathAttr, SymbolRef,
    generate_symbol_name,
};
use crate::{
    FxHashMap, Op, Operation, OperationRef, ProgramPoint, Report, UnsafeIntrusiveEntityRef,
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
    fn get(&self, name: SymbolName) -> Option<SymbolRef> {
        self.symbol_manager().lookup(name)
    }

    /// Resolve the entry for `path` in this table, or via the root symbol table
    fn resolve(&self, path: &SymbolPath) -> Option<SymbolRef> {
        let found = self.symbol_manager().lookup_symbol_ref(path)?;
        let op = found.borrow();
        let sym = op.as_symbol().expect("symbol table resolved to a non-symbol op!");
        Some(unsafe { SymbolRef::from_raw(sym) })
    }

    /// Insert `entry` in the symbol table, but only if no other symbol with the same name exists.
    ///
    /// If provided, the symbol will be inserted at the given insertion point in the body of the
    /// symbol table operation.
    ///
    /// This function will panic if the symbol is attached to another symbol table.
    ///
    /// Returns `true` if successful, `false` if the symbol is already defined
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
    fn remove(&mut self, name: SymbolName) -> Option<SymbolRef> {
        let mut manager = self.symbol_manager_mut();

        if let Some(symbol) = manager.lookup(name) {
            manager.remove(symbol);
            Some(symbol)
        } else {
            None
        }
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
        let op = self.get(name)?;
        let op = op.borrow();
        let op = op.as_symbol_operation().downcast_ref::<T>()?;
        Some(unsafe { UnsafeIntrusiveEntityRef::from_raw(op) })
    }
}

/// A [SymbolMap] is a low-level datastructure used in implementing a [SymbolTable] operation.
///
/// It is primarily responsible for maintaining a mapping between symbol names, and the symbol
/// operations registered to those names, within the body of the containing [SymbolTable] op.
///
/// In most circumstances, you will want to interact with this via [SymbolManager] or
/// [SymbolManagerMut], as the operations provided here are mostly low-level plumbing, and thus
/// incomplete without functionality provided by higher-level abstractions.
#[derive(Default, Debug)]
pub struct SymbolMap {
    /// A low-level mapping of symbols to operations found in this table
    symbols: FxHashMap<SymbolName, SymbolRef>,
    /// Used to unique symbol names when conflicts are detected
    uniquing_count: usize,
}
impl SymbolMap {
    /// Build a [SymbolMap] on the fly from the given operation.
    ///
    /// It is assumed that the given operation is a [SymbolTable] op, but this is not checked, and
    /// does not affect the correctness - however, it has limited utility for non-symbol table ops.
    pub fn build(op: &Operation) -> Self {
        let mut symbols = FxHashMap::default();

        let region = op.regions().front().get().unwrap();
        for op in region.entry().body() {
            if let Some(symbol) = op.as_trait::<dyn Symbol>() {
                let name = symbol.name();
                let symbol_ref = unsafe { SymbolRef::from_raw(symbol) };
                symbols
                    .try_insert(name, symbol_ref)
                    .expect("expected region to contain uniquely named symbol operations");
            }
        }

        Self {
            symbols,
            uniquing_count: 0,
        }
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
    /// defined in the global namespace). In short, we only attempt to resolve absolute paths where
    /// the first component matches the root symbol. If it matches, then the symbol is resolved
    /// normally from there, otherwise `None` is returned.
    pub fn resolve(&self, symbol_table: &Operation, attr: &SymbolPath) -> Option<OperationRef> {
        let mut components = attr.components();

        // Resolve absolute paths via the root symbol table
        if attr.is_absolute() {
            let _ = components.next();

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
                match components.next()? {
                    SymbolNameComponent::Leaf(name) => {
                        return if name == root_symbol.name() {
                            Some(root)
                        } else {
                            None
                        };
                    }
                    SymbolNameComponent::Component(name) => {
                        if name != root_symbol.name() {
                            return None;
                        }
                    }
                    SymbolNameComponent::Root => unreachable!(),
                }
            }

            // Resolve the symbol from `root`
            let root_symbol_table = root_op.as_trait::<dyn SymbolTable>()?;
            let symbol_manager = root_symbol_table.symbol_manager();
            symbol_manager.symbols().resolve_components(components)
        } else {
            self.resolve_components(components)
        }
    }

    fn resolve_components(
        &self,
        mut components: impl ExactSizeIterator<Item = SymbolNameComponent>,
    ) -> Option<OperationRef> {
        match components.next()? {
            super::SymbolNameComponent::Component(name) => {
                let mut found = self.get_op(name);
                loop {
                    let op_ref = found.take()?;
                    let op = op_ref.borrow();
                    let symbol_table = op.as_trait::<dyn SymbolTable>()?;
                    let manager = symbol_table.symbol_manager();
                    match components.next() {
                        None => return Some(op_ref),
                        Some(super::SymbolNameComponent::Component(name)) => {
                            found = manager.lookup_op(name);
                        }
                        Some(super::SymbolNameComponent::Leaf(name)) => {
                            assert_eq!(components.next(), None);
                            break manager.lookup_op(name);
                        }
                        Some(super::SymbolNameComponent::Root) => unreachable!(),
                    }
                }
            }
            super::SymbolNameComponent::Leaf(name) => self.get_op(name),
            super::SymbolNameComponent::Root => {
                unreachable!("root component should have already been consumed")
            }
        }
    }

    /// Returns true if a symbol named `name` is in the map
    #[inline]
    pub fn contains_key<K>(&self, name: &K) -> bool
    where
        K: ?Sized + core::hash::Hash + hashbrown::Equivalent<SymbolName>,
    {
        self.symbols.contains_key(name)
    }

    /// Remove the entry for `name` from this map, if present.
    #[inline]
    pub fn remove(&mut self, name: SymbolName) -> Option<SymbolRef> {
        self.symbols.remove(&name)
    }

    /// Inserts `symbol` in the map, as `name`, so long as `name` is not already in the map.
    #[inline]
    pub fn insert_new(&mut self, name: SymbolName, symbol: SymbolRef) -> bool {
        self.symbols.try_insert(name, symbol).is_ok()
    }

    /// Inserts `symbol` in the map, with `name` if that name is not already registered in the map.
    /// Otherwise, a unique variation of `name` is generated, and `symbol` is inserted in the map
    /// with that name instead.
    ///
    /// If `name` is modified to make it unique, `symbol` is updated with the new name on insertion.
    ///
    /// Returns the name `symbol` has after insertion.
    ///
    /// NOTE: If `symbol` is already in the map with `name`, this is a no-op.
    pub fn insert(&mut self, name: SymbolName, mut symbol: SymbolRef) -> SymbolName {
        // Add the symbol to the symbol map
        // let sym = symbol.borrow();
        match self.symbols.try_insert(name, symbol) {
            Ok(_) => {
                symbol.borrow_mut().set_name(name);
                name
            }
            Err(err) => {
                // If this exact symbol was already in the table, do nothing
                if err.entry.get() == &symbol {
                    assert_eq!(
                        symbol.borrow().name(),
                        name,
                        "name does not match what was registered with the symbol table"
                    );
                    return name;
                }

                // Otherwise, we need to make the symbol name unique
                let uniqued = generate_symbol_name(name, &mut self.uniquing_count, |name| {
                    !self.symbols.contains_key(name)
                });
                // drop(sym);
                symbol.borrow_mut().set_name(uniqued);
                // TODO: visit uses? symbol should be unused AFAICT
                self.symbols.insert(uniqued, symbol);
                uniqued
            }
        }
    }

    /// Ensures that the given symbol name is unique within this symbol map, as well as all of the
    /// provided symbol managers.
    ///
    /// Returns the unique name, but this function does not modify the map or rename the symbol
    /// itself, that is expected to be done from [SymbolManagerMut].
    pub fn make_unique(&mut self, op: &SymbolRef, tables: &[SymbolManager<'_>]) -> SymbolName {
        // Determine new name that is unique in all symbol tables.
        let name = { op.borrow().name() };

        generate_symbol_name(name, &mut self.uniquing_count, |name| {
            if self.symbols.contains_key(name) {
                return false;
            }
            !tables.iter().any(|t| t.symbols.contains_key(name))
        })
    }

    /// Get an iterator of [SymbolRef] corresponding to the [Symbol] operations in this map
    pub fn symbols(&self) -> impl Iterator<Item = SymbolRef> + '_ {
        self.symbols.values().cloned()
    }
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

    /// Inserts a new symbol into the table, as long as the symbol name is unique.
    ///
    /// Returns `false` if an existing symbol with the same name is already in the table.
    ///
    /// # Panics
    ///
    /// This function will panic if `symbol` is already attached to another operation.
    pub fn insert_new(&mut self, symbol: SymbolRef, ip: ProgramPoint) -> bool {
        let name = symbol.borrow().name();
        if self.symbols.contains_key(&name) {
            return false;
        }

        assert_eq!(self.insert(symbol, ip), name, "expected insertion to preserve original name");

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
    pub fn insert(&mut self, symbol: SymbolRef, mut ip: ProgramPoint) -> SymbolName {
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

        // Add the symbol to the symbol map
        self.symbols.insert(name, symbol)
    }

    /// Renames the given operation, and updates the symbol table and all uses of the old name.
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
                !self.symbols.contains_key(&to),
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
        assert!(!self.symbols.contains_key(&name), "old name still exists");

        Ok(())
    }

    /// Replaces the symbol name stored in all uses of the symbol `op`.
    ///
    /// NOTE: This is not the same as replacing uses of one symbol with another, this used while
    /// renaming the symbol name of `op`, while preserving its uses.
    pub fn replace_all_symbol_uses(
        &mut self,
        mut op: SymbolRef,
        to: SymbolName,
    ) -> Result<(), Report> {
        // Visit all users of `symbol`, and rewrite the name used with `to`
        let mut symbol = op.borrow_mut();
        let mut users = symbol.uses_mut().front_mut();
        while let Some(mut user) = users.as_pointer() {
            users.move_next();

            let mut user = user.borrow_mut();
            let mut user_op = user.owner.borrow_mut();
            let symbol_name_attr = user_op
                .get_typed_attribute_mut::<SymbolPathAttr>(user.attr)
                .expect("invalid symbol use");
            symbol_name_attr.path.set_name(to);
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
