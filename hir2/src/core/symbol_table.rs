use core::any::Any;

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
    /// The unique key type associated with entries in this symbol table
    type Key;
    /// The value type of an entry in the symbol table
    type Entry;

    /// Get the entry for `id` in this table
    fn get(&self, id: &Self::Key) -> Option<Self::Entry>;

    /// Insert `entry` in the symbol table.
    ///
    /// Returns `true` if successful, or `false` if an entry already exists
    fn insert(&mut self, entry: Self::Entry) -> bool;

    /// Remove the symbol `id`, and return the entry if one was present.
    fn remove(&mut self, id: &Self::Key) -> Option<Self::Entry>;
}

/// A [Symbol] is an IR entity with an associated _symbol_, or name, which is expected to be unique
/// amongst all other symbols in the same namespace.
///
/// For example, functions are named, and are expected to be unique within the same module,
/// otherwise it would not be possible to unambiguously refer to a function by name. Likewise
/// with modules in a program, etc.
pub trait Symbol: Any {
    type Id: Copy + Clone + PartialEq + Eq + PartialOrd + Ord;

    fn id(&self) -> Self::Id;
}
