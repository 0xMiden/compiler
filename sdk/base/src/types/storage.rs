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

pub trait StorageMapAccess<K, V> {
    /// Returns a map item value for `key` from the account storage.
    fn get(&self, key: &K) -> V;
    /// Sets a map item `value` for `key` in the account storage and returns the old value.
    fn set(&mut self, key: K, value: V) -> V;
}

pub struct StorageMap {
    pub slot: StorageSlotId,
}

impl<V: From<Word> + Into<Word>> StorageMapAccess<Word, V> for StorageMap {
    /// Returns a map item value from the account storage.
    #[inline(always)]
    fn get(&self, key: &Word) -> V {
        storage::get_map_item(self.slot, key).into()
    }

    /// Sets a map item `value` in the account storage and returns the previous value.
    #[inline(always)]
    fn set(&mut self, key: Word, value: V) -> V {
        storage::set_map_item(self.slot, key, value.into()).into()
    }
}
