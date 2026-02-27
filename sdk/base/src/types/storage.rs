use miden_base_sys::bindings::{StorageSlotId, storage};
use miden_stdlib_sys::Word;

/// A type that can be stored in (or loaded from) account storage.
///
/// Storage slots and map items store a single [`Word`]. Implementations must define a reversible
/// conversion between the Rust type and a [`Word`].
pub trait WordValue: Into<Word> + From<Word> {}

impl<T> WordValue for T where T: Into<Word> + From<Word> {}

/// A type that can be used as a key in a storage map.
///
/// Map keys are passed by reference for lookups (to match `HashMap` ergonomics), so keys must be
/// cheaply clonable.
pub trait WordKey: Clone + Into<Word> {}

impl<T> WordKey for T where T: Clone + Into<Word> {}

/// Typed access to a single account storage slot.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct Storage<T> {
    /// The underlying storage slot id.
    pub slot: StorageSlotId,
    _marker: core::marker::PhantomData<T>,
}

impl<T> Storage<T> {
    /// Creates a new typed storage handle for `slot`.
    pub const fn new(slot: StorageSlotId) -> Self {
        Self {
            slot,
            _marker: core::marker::PhantomData,
        }
    }
}

impl<T> From<StorageSlotId> for Storage<T> {
    fn from(slot: StorageSlotId) -> Self {
        Self::new(slot)
    }
}

impl<T: WordValue> Storage<T> {
    /// Reads the current value from account storage.
    #[inline(always)]
    pub fn get(&self) -> T {
        storage::get_item(self.slot).into()
    }

    /// Sets an item `value` in the account storage and returns the previous value.
    #[inline(always)]
    pub fn set(&mut self, value: T) -> T {
        storage::set_item(self.slot, value.into()).into()
    }
}

/// Typed access to an account storage map.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct StorageMap<K, V> {
    /// The underlying storage slot id.
    pub slot: StorageSlotId,
    _marker: core::marker::PhantomData<(K, V)>,
}

impl<K, V> StorageMap<K, V> {
    /// Creates a new typed storage map handle for `slot`.
    pub const fn new(slot: StorageSlotId) -> Self {
        Self {
            slot,
            _marker: core::marker::PhantomData,
        }
    }
}

impl<K, V> From<StorageSlotId> for StorageMap<K, V> {
    fn from(slot: StorageSlotId) -> Self {
        Self::new(slot)
    }
}

impl<K: WordKey, V: WordValue> StorageMap<K, V> {
    /// Returns the value associated with `key` from the account storage map.
    ///
    /// Note: Unlike `HashMap::get`, this returns `V` by value.
    /// At the protocol layer, absent keys read as the default word value.
    #[inline(always)]
    pub fn get(&self, key: &K) -> V {
        let key: Word = key.clone().into();
        storage::get_map_item(self.slot, &key).into()
    }

    /// Sets `value` for `key` in the account storage map and returns the previous value.
    ///
    /// This is analogous to `HashMap::insert`, except it always returns a value (the protocol does
    /// not distinguish "missing" from "default").
    #[inline(always)]
    pub fn set(&mut self, key: K, value: V) -> V {
        storage::set_map_item(self.slot, key.into(), value.into()).into()
    }
}
