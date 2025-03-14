use miden_base_sys::bindings::{storage, StorageCommitmentRoot};
use miden_stdlib_sys::Word;

pub trait ValueAccess<V> {
    fn read(&self) -> V;
    fn write(&self, value: V) -> (StorageCommitmentRoot, V);
}

pub struct Value {
    pub slot: u8,
}

impl ValueAccess<Word> for Value {
    /// Returns an item value from the account storage.
    #[inline(always)]
    fn read(&self) -> Word {
        storage::get_item(self.slot)
    }

    /// Sets an item `value` in the account storage and returns (new_root, old_value)
    /// Where:
    /// - new_root is the new storage commitment.
    /// - old_value is the previous value of the item.
    #[inline(always)]
    fn write(&self, value: Word) -> (StorageCommitmentRoot, Word) {
        storage::set_item(self.slot, value)
    }
}

pub trait StorageMapAccess<K, V> {
    fn read(&self, key: &K) -> V;
    fn write(&self, key: K, value: V) -> (StorageCommitmentRoot, V);
}

pub struct StorageMap {
    pub slot: u8,
}

impl StorageMapAccess<Word, Word> for StorageMap {
    /// Returns a map item value from the account storage.
    #[inline(always)]
    fn read(&self, key: &Word) -> Word {
        storage::get_map_item(self.slot, key)
    }

    /// Sets a map item `value` in the account storage and returns (old_root, old_value)
    /// Where:
    /// - old_root is the old map root.
    /// - old_value is the previous value of the item.
    #[inline(always)]
    fn write(&self, key: Word, value: Word) -> (StorageCommitmentRoot, Word) {
        storage::set_map_item(self.slot, key, value)
    }
}
