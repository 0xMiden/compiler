use super::{generate_symbol_name, Symbol, SymbolName, SymbolNameAttr, SymbolRef};
use crate::{
    traits::Terminator, FxHashMap, InsertionPoint, IteratorExt, Op, Operation, OperationRef,
    Report, UnsafeIntrusiveEntityRef,
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

    /// Insert `entry` in the symbol table, but only if no other symbol with the same name exists.
    ///
    /// If provided, the symbol will be inserted at the given insertion point in the body of the
    /// symbol table operation.
    ///
    /// This function will panic if the symbol is attached to another symbol table.
    ///
    /// Returns `true` if successful, `false` if the symbol is already defined
    fn insert_new(&mut self, entry: SymbolRef, ip: Option<InsertionPoint>) -> bool {
        self.symbol_manager_mut().insert_new(entry, ip)
    }

    /// Like [SymbolTable::insert_new], except the symbol is renamed to avoid collisions.
    ///
    /// Returns the name of the symbol after insertion.
    fn insert(&mut self, entry: SymbolRef, ip: Option<InsertionPoint>) -> SymbolName {
        self.symbol_manager_mut().insert(entry, ip)
    }

    /// Remove the symbol `name`, and return the entry if one was present.
    fn remove(&mut self, name: SymbolName) -> Option<SymbolRef> {
        let mut manager = self.symbol_manager_mut();

        if let Some(symbol) = manager.lookup(name) {
            manager.remove(symbol.clone());
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
#[derive(Default)]
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
        let sym = symbol.borrow();
        match self.symbols.try_insert(name, symbol.clone()) {
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
                drop(sym);
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
impl<'a> From<SymbolMap> for Symbols<'a> {
    fn from(value: SymbolMap) -> Self {
        Self::Owned(value)
    }
}
impl<'a> core::ops::Deref for Symbols<'a> {
    type Target = SymbolMap;

    fn deref(&self) -> &Self::Target {
        match self {
            Self::Owned(ref symbols) => symbols,
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
impl<'a> From<SymbolMap> for SymbolsMut<'a> {
    fn from(value: SymbolMap) -> Self {
        Self::Owned(value)
    }
}
impl<'a> core::ops::Deref for SymbolsMut<'a> {
    type Target = SymbolMap;

    fn deref(&self) -> &Self::Target {
        match self {
            Self::Owned(ref symbols) => symbols,
            Self::Borrowed(symbols) => symbols,
        }
    }
}
impl<'a> core::ops::DerefMut for SymbolsMut<'a> {
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
    /// The name associated with this symbol table
    ///
    /// All symbols defined within this table, are qualified with this name
    #[allow(unused)]
    name: SymbolName,
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
        let name = symbol_table
            .as_symbol()
            .expect("expected symbol table to implement Symbol")
            .name();
        Self {
            name,
            symbol_table,
            symbols,
        }
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
    /// This function will search for the symbol relative to the current symbol table, for example:
    ///
    /// * `::foo::bar::baz` will be resolved relative to the nearest parent symbol table which
    ///   corresponds to a prefix of the path, falling back to the root symbol table if there is
    ///   no common prefix.
    /// * `bar::baz` is presumed to be in a child symbol table named `bar`, in which the symbol
    ///   `baz` will be resolved.
    /// * `baz` will be resolved in the current symbol table as a child of the symbol table op
    pub fn lookup_symbol_ref(&self, _attr: &SymbolNameAttr) -> Option<OperationRef> {
        todo!()
    }
}

impl<'a> From<&'a Operation> for SymbolManager<'a> {
    fn from(symbol_table: &'a Operation) -> Self {
        let name = assert_symbol_table(symbol_table);
        Self {
            name,
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
    /// The name associated with this symbol table
    ///
    /// All symbols defined within this table, are qualified with this name
    #[allow(unused)]
    name: SymbolName,
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
        let name = symbol_table
            .symbol_name_if_symbol()
            .expect("expected symbol table to implement Symbol trait");
        Self {
            name,
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
    /// This function will search for the symbol relative to the current symbol table, for example:
    ///
    /// * `::foo::bar::baz` will be resolved relative to the nearest parent symbol table which
    ///   corresponds to a prefix of the path, falling back to the root symbol table if there is
    ///   no common prefix.
    /// * `bar::baz` is presumed to be in a child symbol table named `bar`, in which the symbol
    ///   `baz` will be resolved.
    /// * `baz` will be resolved in the current symbol table as a child of the symbol table op
    pub fn lookup_symbol_ref(&self, _attr: &SymbolNameAttr) -> Option<OperationRef> {
        todo!()
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
    pub fn insert_new(&mut self, symbol: SymbolRef, ip: Option<InsertionPoint>) -> bool {
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
    pub fn insert(&mut self, symbol: SymbolRef, ip: Option<InsertionPoint>) -> SymbolName {
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
            let mut body = self.symbol_table.region_mut(0);
            let mut block = body.entry_mut();
            let has_terminator = block.has_terminator();
            let block_ref = block.as_block_ref();
            let ops = block.body_mut();
            let (mut cursor, placement) = match ip {
                Some(ip) => match ip.at {
                    crate::ProgramPoint::Block(b) => {
                        assert_eq!(
                            b, block_ref,
                            "invalid insertion point: referenced block is not in this symbol table"
                        );
                        // Move the insertion point before the terminator, if there is one
                        match ip.placement {
                            crate::Insert::After if has_terminator => {
                                (ops.back_mut(), crate::Insert::Before)
                            }
                            crate::Insert::After => (ops.back_mut(), crate::Insert::After),
                            crate::Insert::Before => (ops.front_mut(), crate::Insert::Before),
                        }
                    }
                    crate::ProgramPoint::Op(op) => {
                        assert!(
                            op.borrow().parent().is_some_and(|b| b == block_ref),
                            "invalid insertion point: referenced op is not a child of this symbol \
                             table"
                        );
                        let is_terminator =
                            has_terminator && ops.back().as_pointer().is_some_and(|o| o == op);
                        match ip.placement {
                            // The caller _explicitly_ requested this, raise an assertion if the op being
                            // inserted is not a valid terminator
                            crate::Insert::After if is_terminator => {
                                assert!(
                                    op.borrow().implements::<dyn Terminator>(),
                                    "cannot insert a symbol after the terminator of its parent \
                                     symbol table, if it is not itself a valid terminator"
                                );
                                (ops.back_mut(), crate::Insert::After)
                            }
                            placement => (unsafe { ops.cursor_mut_from_ptr(op) }, placement),
                        }
                    }
                },
                None => {
                    if has_terminator {
                        (ops.back_mut(), crate::Insert::Before)
                    } else {
                        (ops.back_mut(), crate::Insert::After)
                    }
                }
            };

            if matches!(placement, crate::Insert::Before) {
                cursor.insert_before(symbol_op.clone());
            } else {
                cursor.insert_after(symbol_op.clone());
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
        self.replace_all_symbol_uses(op.clone(), to)?;

        // Remove op with old name, change name, add with new name.
        //
        // The order is important here due to how `remove` and `insert` rely on the op name.
        self.remove(op.clone());
        {
            op.borrow_mut().set_name(to);
        }
        self.insert(op.clone(), None);

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
                .get_typed_attribute_mut::<SymbolNameAttr>(user.attr)
                .expect("invalid symbol use");
            symbol_name_attr.name = to;
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
        let name = assert_symbol_table(&*symbol_table);
        let symbols = SymbolMap::build(&*symbol_table).into();
        Self {
            name,
            symbol_table,
            symbols,
        }
    }
}

/// Assert that `op` is a valid [SymbolTable] implementation
///
/// Returns the symbol name of the op when successful
fn assert_symbol_table(op: &Operation) -> SymbolName {
    let symbol = op.as_symbol().expect("expected operation to implement the Symbol trait");
    assert_eq!(op.num_regions(), 1, "expected operation to have a single region");
    assert!(
        op.region(0).body().iter().has_single_element(),
        "expected single-region, single-block operation"
    );

    symbol.name()
}
