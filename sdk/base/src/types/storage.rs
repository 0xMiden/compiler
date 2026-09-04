use miden_base_sys::bindings::{
    AssetAmount, StorageSlotId, felt_from_padded_word, padded_word_from_felt, storage,
};
use miden_stdlib_sys::{Digest, Felt, Word};

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

impl WordValue for AssetAmount {
    fn try_into_word(self) -> Result<Word, &'static str> {
        // Re-validate before serializing so a directly assigned out-of-range felt cannot enter
        // account storage.
        let amount = AssetAmount::try_from(self.as_felt())
            .map_err(|_| "asset amount exceeds the maximum allowed amount")?;
        Ok(padded_word_from_felt(amount.into()))
    }

    fn try_from_word(word: Word) -> Result<Self, &'static str> {
        AssetAmount::try_from(felt_from_padded_word(word)?)
            .map_err(|_| "asset amount exceeds the maximum allowed amount")
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

impl WordKey for AssetAmount {
    fn try_into_word(self) -> Result<Word, &'static str> {
        // Re-validate before serializing so a directly assigned out-of-range felt cannot be
        // used as a storage key.
        let amount = AssetAmount::try_from(self.as_felt())
            .map_err(|_| "asset amount exceeds the maximum allowed amount")?;
        Ok(padded_word_from_felt(amount.into()))
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

/// Typed access to a single account storage value.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct StorageValue<T: WordValue> {
    /// The underlying storage slot id.
    pub slot: StorageSlotId,
    _marker: core::marker::PhantomData<T>,
}

impl<T: WordValue> StorageValue<T> {
    /// Creates a new typed storage-value handle for `slot`.
    pub const fn new(slot: StorageSlotId) -> Self {
        Self {
            slot,
            _marker: core::marker::PhantomData,
        }
    }
}

impl<T: WordValue> From<StorageSlotId> for StorageValue<T> {
    fn from(slot: StorageSlotId) -> Self {
        Self::new(slot)
    }
}

impl<T: WordValue> StorageValue<T> {
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

/// Marker trait implemented by the signature types `#[component_storage]` generates for
/// stored-procedure slots.
///
/// Each `StorageValue<StoredProcedure<fn(..) -> R>>` field expands to a dedicated marker type
/// implementing this trait, so a [`StoredProcedure`] is always tied to exactly one call signature.
/// The trait is sealed behind a hidden supertrait that only the macro expansion implements.
pub trait ProcedureSignature: __stored_procedure_sealed::Sealed {}

/// Supertrait sealing [`ProcedureSignature`]; implemented only by `#[component_storage]`
/// expansions.
#[doc(hidden)]
pub mod __stored_procedure_sealed {
    pub trait Sealed {}
}

/// The MAST root of a sibling account component's procedure, stored in an account storage slot.
///
/// A slot of this type is declared as `StorageValue<StoredProcedure<fn(..) -> R>>` in a
/// `#[component_storage]` struct, which generates a `call` method with exactly that signature
/// (see the `#[component_storage]` documentation). Calling it invokes the procedure whose root is
/// stored in the slot in a new VM context (`dyncall`), the same way a direct call into a sibling
/// component works.
///
/// The root is set from off-chain code, through the sibling package's exports: the SDK offers no
/// constructor, and the guest has no way to obtain procedure roots. The stored root
/// is not validated by the compiler or the VM against the declared signature. A root that names
/// no procedure of the account, or one with a different stack contract, makes the transaction
/// fail or yields wrong in-VM results, but never breaks Rust memory safety in the caller. Calling
/// an unset slot (all-zero root) fails the transaction with a descriptive assertion.
pub struct StoredProcedure<S: ProcedureSignature> {
    root: Word,
    _sig: core::marker::PhantomData<S>,
}

impl<S: ProcedureSignature> StoredProcedure<S> {
    /// Returns true when the slot holds a procedure root, i.e. is not all-zero.
    #[inline(always)]
    pub fn is_set(&self) -> bool {
        self.root != Word::default()
    }

    /// Returns the stored procedure root.
    ///
    /// The root alone grants no way to call the procedure; it is exposed for inspection and
    /// forwarding only.
    #[inline(always)]
    pub fn root(&self) -> Word {
        self.root
    }
}

impl<S: ProcedureSignature> WordValue for StoredProcedure<S> {
    fn try_into_word(self) -> Result<Word, &'static str> {
        Ok(self.root)
    }

    fn try_from_word(word: Word) -> Result<Self, &'static str> {
        Ok(Self {
            root: word,
            _sig: core::marker::PhantomData,
        })
    }
}

// Manual impls: the derives would needlessly bound `S` on the derived traits.
impl<S: ProcedureSignature> Clone for StoredProcedure<S> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<S: ProcedureSignature> Copy for StoredProcedure<S> {}

impl<S: ProcedureSignature> PartialEq for StoredProcedure<S> {
    fn eq(&self, other: &Self) -> bool {
        self.root == other.root
    }
}

impl<S: ProcedureSignature> Eq for StoredProcedure<S> {}

impl<S: ProcedureSignature> core::fmt::Debug for StoredProcedure<S> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("StoredProcedure").field("root", &self.root).finish()
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
