use miden_base_sys::bindings::{StorageSlotId, storage};
use miden_stdlib_sys::{Digest, Felt, Word, felt};

/// Packs a scalar felt into the low limb of a storage word.
fn padded_word_from_felt(value: Felt) -> Word {
    Word::new([felt!(0), felt!(0), felt!(0), value])
}

/// Extracts a scalar felt from a storage word with zero-padded high limbs.
fn felt_from_padded_word(value: Word) -> Result<Felt, &'static str> {
    if value[0] != felt!(0) || value[1] != felt!(0) || value[2] != felt!(0) {
        return Err("expected zero padding in the upper three felts");
    }

    Ok(value[3])
}

/// A type that can be stored in (or loaded from) account storage.
///
/// Storage slots and map items store a single [`Word`]. Implementations must define a reversible
/// conversion between the Rust type and a [`Word`].
pub trait WordValue: Sized {
    /// Converts the value into the single storage word used by the host.
    fn try_into_word(self) -> Result<Word, &'static str>;

    /// Reconstructs the value from the single storage word returned by the host.
    fn try_from_word(word: Word) -> Result<Self, &'static str>;
}

impl WordValue for Word {
    fn try_into_word(self) -> Result<Word, &'static str> {
        Ok(self)
    }

    fn try_from_word(word: Word) -> Result<Self, &'static str> {
        Ok(word)
    }
}

impl WordValue for Felt {
    fn try_into_word(self) -> Result<Word, &'static str> {
        Ok(padded_word_from_felt(self))
    }

    fn try_from_word(word: Word) -> Result<Self, &'static str> {
        felt_from_padded_word(word)
    }
}

impl WordValue for Digest {
    fn try_into_word(self) -> Result<Word, &'static str> {
        Ok(self.into())
    }

    fn try_from_word(word: Word) -> Result<Self, &'static str> {
        Ok(word.try_into().unwrap())
    }
}

impl WordValue for miden_base_sys::bindings::AccountId {
    fn try_into_word(self) -> Result<Word, &'static str> {
        Ok(self.into())
    }

    fn try_from_word(word: Word) -> Result<Self, &'static str> {
        word.try_into()
    }
}

impl WordValue for miden_base_sys::bindings::Recipient {
    fn try_into_word(self) -> Result<Word, &'static str> {
        Ok(self.into())
    }

    fn try_from_word(word: Word) -> Result<Self, &'static str> {
        Ok(word.into())
    }
}

impl WordValue for miden_base_sys::bindings::Tag {
    fn try_into_word(self) -> Result<Word, &'static str> {
        Ok(self.into())
    }

    fn try_from_word(word: Word) -> Result<Self, &'static str> {
        word.try_into()
    }
}

impl WordValue for miden_base_sys::bindings::NoteIdx {
    fn try_into_word(self) -> Result<Word, &'static str> {
        Ok(self.into())
    }

    fn try_from_word(word: Word) -> Result<Self, &'static str> {
        word.try_into()
    }
}

impl WordValue for miden_base_sys::bindings::NoteType {
    fn try_into_word(self) -> Result<Word, &'static str> {
        Ok(self.into())
    }

    fn try_from_word(word: Word) -> Result<Self, &'static str> {
        word.try_into()
    }
}

/// A type that can be used as a key in a storage map.
///
/// Map keys are passed by value for lookups to avoid requiring `Clone` just to materialize a
/// [`Word`] for the host call.
pub trait WordKey: Copy {
    /// Converts the key into the single storage word passed to the host.
    fn try_into_word(self) -> Result<Word, &'static str>;
}

impl WordKey for Word {
    fn try_into_word(self) -> Result<Word, &'static str> {
        Ok(self)
    }
}

impl WordKey for Felt {
    fn try_into_word(self) -> Result<Word, &'static str> {
        Ok(padded_word_from_felt(self))
    }
}

impl WordKey for miden_base_sys::bindings::AccountId {
    fn try_into_word(self) -> Result<Word, &'static str> {
        Ok(self.into())
    }
}

impl WordKey for miden_base_sys::bindings::Tag {
    fn try_into_word(self) -> Result<Word, &'static str> {
        Ok(self.into())
    }
}

impl WordKey for miden_base_sys::bindings::NoteIdx {
    fn try_into_word(self) -> Result<Word, &'static str> {
        Ok(self.into())
    }
}

impl WordKey for miden_base_sys::bindings::NoteType {
    fn try_into_word(self) -> Result<Word, &'static str> {
        Ok(self.into())
    }
}

/// Typed access to a single account storage slot.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct Storage<T: WordValue> {
    /// The underlying storage slot id.
    pub slot: StorageSlotId,
    _marker: core::marker::PhantomData<T>,
}

impl<T: WordValue> Storage<T> {
    /// Creates a new typed storage handle for `slot`.
    pub const fn new(slot: StorageSlotId) -> Self {
        Self {
            slot,
            _marker: core::marker::PhantomData,
        }
    }
}

impl<T: WordValue> From<StorageSlotId> for Storage<T> {
    fn from(slot: StorageSlotId) -> Self {
        Self::new(slot)
    }
}

impl<T: WordValue> Storage<T> {
    /// Reads the current value from account storage.
    #[inline(always)]
    pub fn get(&self) -> T {
        T::try_from_word(storage::get_item(self.slot))
            .unwrap_or_else(|_| panic!("storage slot {:?} contained an invalid word", self.slot))
    }

    /// Sets an item `value` in the account storage and returns the previous value.
    #[inline(always)]
    pub fn set(&mut self, value: T) -> T {
        let value = value
            .try_into_word()
            .unwrap_or_else(|_| panic!("failed to convert value for storage slot {:?}", self.slot));
        T::try_from_word(storage::set_item(self.slot, value))
            .unwrap_or_else(|_| panic!("storage slot {:?} contained an invalid word", self.slot))
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
        let key = key.try_into_word().unwrap_or_else(|_| {
            panic!("failed to convert key for storage map slot {:?}", self.slot)
        });
        V::try_from_word(storage::get_map_item(self.slot, &key)).unwrap_or_else(|_| {
            panic!("storage map slot {:?} contained an invalid word", self.slot)
        })
    }

    /// Sets `value` for `key` in the account storage map and returns the previous value.
    ///
    /// This is analogous to `HashMap::insert`, except it always returns a value (the protocol does
    /// not distinguish "missing" from "default").
    #[inline(always)]
    pub fn set(&mut self, key: K, value: V) -> V {
        let key = key.try_into_word().unwrap_or_else(|_| {
            panic!("failed to convert key for storage map slot {:?}", self.slot)
        });
        let value = value.try_into_word().unwrap_or_else(|_| {
            panic!("failed to convert value for storage map slot {:?}", self.slot)
        });
        V::try_from_word(storage::set_map_item(self.slot, key, value)).unwrap_or_else(|_| {
            panic!("storage map slot {:?} contained an invalid word", self.slot)
        })
    }
}
