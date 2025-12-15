use miden_base_sys::bindings::{storage, StorageCommitmentRoot};
use miden_stdlib_sys::Word;

pub trait ValueAccess<V> {
    fn read(&self) -> V;
    fn write(&mut self, value: V) -> (StorageCommitmentRoot, V);
}

pub struct Value {
    pub slot: u8,
}

impl<V: Into<Word> + From<Word>> ValueAccess<V> for Value {
    /// Returns an item value from the account storage.
    #[inline(always)]
    fn read(&self) -> V {
        storage::get_item(self.slot).into()
    }

    /// Sets an item `value` in the account storage and returns (new_root, old_value)
    /// Where:
    /// - new_root is the new storage commitment.
    /// - old_value is the previous value of the item.
    #[inline(always)]
    fn write(&mut self, value: V) -> (StorageCommitmentRoot, V) {
        let (root, old_word) = storage::set_item(self.slot, value.into());
        (root, old_word.into())
    }
}

pub trait StorageMapAccess<K, V> {
    /// Returns a map item value for `key` from the account storage.
    fn get(&self, key: &K) -> V;
    /// Sets a map item `value` for `key` in the account storage and returns (old_root, old_value)
    fn set(&mut self, key: K, value: V) -> (StorageCommitmentRoot, V);
}

pub struct StorageMap {
    pub slot: u8,
}

impl<K: Into<Word> + AsRef<Word>, V: From<Word> + Into<Word>> StorageMapAccess<K, V>
    for StorageMap
{
    /// Returns a map item value from the account storage.
    #[inline(always)]
    fn get(&self, key: &K) -> V {
        storage::get_map_item(self.slot, key.as_ref()).into()
    }

    /// Sets a map item `value` in the account storage and returns (old_root, old_value)
    /// Where:
    /// - old_root is the old map root.
    /// - old_value is the previous value of the item.
    #[inline(always)]
    fn set(&mut self, key: K, value: V) -> (StorageCommitmentRoot, V) {
        let (root, old_word) = storage::set_map_item(self.slot, key.into(), value.into());
        (root, old_word.into())
    }
}
