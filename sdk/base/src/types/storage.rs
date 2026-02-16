use miden_base_sys::bindings::{StorageSlotId, storage};
use miden_stdlib_sys::Word;

pub trait ValueAccess<V> {
    /// Reads the current value from account storage.
    fn read(&self) -> V;
    /// Writes a new value into account storage and returns the previous value.
    fn write(&mut self, value: V) -> V;
}

pub struct Value {
    pub slot: StorageSlotId,
}

impl<V: Into<Word> + From<Word>> ValueAccess<V> for Value {
    /// Returns an item value from the account storage.
    #[inline(always)]
    fn read(&self) -> V {
        storage::get_item(self.slot).into()
    }

    /// Sets an item `value` in the account storage and returns the previous value.
    #[inline(always)]
    fn write(&mut self, value: V) -> V {
        storage::set_item(self.slot, value.into()).into()
    }
}

/// Typed access to an account storage map.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct StorageMap<K: WordKey, V: WordValue> {
    /// The underlying storage slot id.
    pub slot: StorageSlotId,
    _marker: core::marker::PhantomData<(K, V)>,
}

impl<K: WordKey, V: WordValue> StorageMap<K, V> {
    /// Creates a new typed storage map handle for `slot`.
    pub const fn new(slot: StorageSlotId) -> Self {
        Self {
            slot,
            _marker: core::marker::PhantomData,
        }
    }
}

impl<K: WordKey, V: WordValue> From<StorageSlotId> for StorageMap<K, V> {
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
    pub fn get(&self, key: K) -> V {
        let key: Word = key.try_into().unwrap_or_else(|_| {
            panic!("failed to convert key for storage map slot {:?}", self.slot)
        });
        storage::get_map_item(self.slot, &key).try_into().unwrap_or_else(|_| {
            panic!("storage map slot {:?} contained an invalid word", self.slot)
        })
    }

    /// Sets `value` for `key` in the account storage map and returns the previous value.
    ///
    /// This is analogous to `HashMap::insert`, except it always returns a value (the protocol does
    /// not distinguish "missing" from "default").
    #[inline(always)]
    pub fn set(&mut self, key: K, value: V) -> V {
        let key = key.try_into().unwrap_or_else(|_| {
            panic!("failed to convert key for storage map slot {:?}", self.slot)
        });
        let value = value.try_into().unwrap_or_else(|_| {
            panic!("failed to convert value for storage map slot {:?}", self.slot)
        });
        storage::set_map_item(self.slot, key, value).try_into().unwrap_or_else(|_| {
            panic!("storage map slot {:?} contained an invalid word", self.slot)
        })
    }
}
