use core::any::Any;

use crate::UnsafeRef;

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

    /// Check if `id` is associated with an entry of type `T` in this table
    fn has_symbol_of_type<T>(&self, id: &Self::Key) -> bool
    where
        T: Symbol<Id = Self::Key>,
    {
        self.get::<T>(id)
    }

    /// Get the entry for `id` in this table
    fn get<T>(&self, id: &Self::Key) -> Option<UnsafeRef<T>>
    where
        T: Symbol<Id = Self::Key>;

    /// Insert `entry` in the symbol table.
    ///
    /// Returns `true` if successful, or `false` if an entry already exists
    fn insert<T>(&self, entry: UnsafeRef<T>) -> bool
    where
        T: Symbol<Id = Self::Key>;

    /// Remove the symbol `id`, and return the entry if one was present.
    fn remove<T>(&self, id: &Self::Key) -> Option<UnsafeRef<T>>
    where
        T: Symbol<Id = Self::Key>;
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
