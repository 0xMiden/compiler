extern crate alloc;

use alloc::vec::Vec;

use miden_field_repr::FromFeltRepr;
use miden_stdlib_sys::{Digest, Felt, Word, felt, hash_elements, intrinsics::crypto::merge};

/// Packs a scalar felt into the low limb of a protocol word.
fn padded_word_from_felt(value: Felt) -> Word {
    Word::new([felt!(0), felt!(0), felt!(0), value])
}

/// Extracts a scalar felt from a protocol word with zero-padded high limbs.
fn felt_from_padded_word(value: Word) -> Result<Felt, &'static str> {
    if value[0] != felt!(0) || value[1] != felt!(0) || value[2] != felt!(0) {
        return Err("expected zero padding in the upper three felts");
    }

    Ok(value[3])
}

/// Unique identifier for a Miden account, composed of two field elements.
#[derive(Copy, Clone, Debug, PartialEq, Eq, FromFeltRepr)]
pub struct AccountId {
    pub prefix: Felt,
    pub suffix: Felt,
}

impl AccountId {
    /// Creates a new AccountId from prefix and suffix Felt values.
    pub fn new(prefix: Felt, suffix: Felt) -> Self {
        Self { prefix, suffix }
    }
}

impl From<AccountId> for Word {
    #[inline]
    fn from(value: AccountId) -> Self {
        Word::from([felt!(0), felt!(0), value.suffix, value.prefix])
    }
}

impl TryFrom<Word> for AccountId {
    type Error = &'static str;

    #[inline]
    fn try_from(value: Word) -> Result<Self, Self::Error> {
        if value[0] != felt!(0) || value[1] != felt!(0) {
            return Err("expected zero padding in the upper two felts");
        }

        Ok(Self {
            prefix: value[3],
            suffix: value[2],
        })
    }
}

/// A fungible or non-fungible asset encoded as separate vault key and value words.
///
/// The `key` identifies the asset in the account vault and the `value` stores the corresponding
/// asset contents. This matches the v0.14 protocol/base ABI.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[repr(C)]
pub struct Asset {
    /// The asset's vault key.
    pub key: Word,
    /// The asset's vault value.
    pub value: Word,
}

impl Asset {
    /// Creates a new [`Asset`] from its key and value words.
    pub fn new(key: impl Into<Word>, value: impl Into<Word>) -> Self {
        Self {
            key: key.into(),
            value: value.into(),
        }
    }
}

impl From<Asset> for (Word, Word) {
    fn from(val: Asset) -> Self {
        (val.key, val.value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[repr(transparent)]
pub struct Recipient {
    pub inner: Word,
}

impl Recipient {
    /// Computes a recipient digest from the provided components.
    ///
    /// This matches the Miden protocol note recipient digest:
    /// `hash(hash(hash(serial_num, [0; 4]), script_root), inputs_commitment)`.
    ///
    /// Where `inputs_commitment` is the RPO256 hash of the provided `inputs`.
    pub fn compute(serial_num: Word, script_digest: Digest, inputs: Vec<Felt>) -> Self {
        let empty_word = Word::empty();

        let serial_num_hash = merge([Digest::from_word(serial_num), Digest::from_word(empty_word)]);
        let merge_script = merge([serial_num_hash, script_digest]);
        let digest: Word = merge([merge_script, hash_elements(inputs)]).into();

        Self { inner: digest }
    }
}

/// The note metadata returned by `*_note::get_metadata` procedures.
///
/// In the Miden protocol, metadata retrieval returns both the note attachment and the metadata
/// header as separate words.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[repr(C)]
pub struct NoteMetadata {
    /// The attachment of the note.
    pub attachment: Word,
    /// The metadata header of the note.
    pub header: Word,
}

impl NoteMetadata {
    /// Creates a new [`NoteMetadata`] from attachment and header.
    pub fn new(attachment: Word, header: Word) -> Self {
        Self { attachment, header }
    }
}

impl From<[Felt; 4]> for Recipient {
    fn from(value: [Felt; 4]) -> Self {
        Recipient {
            inner: Word::from(value),
        }
    }
}

impl From<Word> for Recipient {
    fn from(value: Word) -> Self {
        Recipient { inner: value }
    }
}

impl From<Recipient> for Word {
    #[inline]
    fn from(value: Recipient) -> Self {
        value.inner
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(transparent)]
pub struct Tag {
    pub inner: Felt,
}

impl From<Felt> for Tag {
    fn from(value: Felt) -> Self {
        Tag { inner: value }
    }
}

impl From<Tag> for Word {
    #[inline]
    fn from(value: Tag) -> Self {
        padded_word_from_felt(value.inner)
    }
}

impl TryFrom<Word> for Tag {
    type Error = &'static str;

    #[inline]
    fn try_from(value: Word) -> Result<Self, Self::Error> {
        Ok(Tag {
            inner: felt_from_padded_word(value)?,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(transparent)]
pub struct NoteIdx {
    pub inner: Felt,
}

impl From<NoteIdx> for Word {
    #[inline]
    fn from(value: NoteIdx) -> Self {
        padded_word_from_felt(value.inner)
    }
}

impl TryFrom<Word> for NoteIdx {
    type Error = &'static str;

    #[inline]
    fn try_from(value: Word) -> Result<Self, Self::Error> {
        Ok(NoteIdx {
            inner: felt_from_padded_word(value)?,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(transparent)]
pub struct NoteType {
    pub inner: Felt,
}

impl From<Felt> for NoteType {
    fn from(value: Felt) -> Self {
        NoteType { inner: value }
    }
}

impl From<NoteType> for Word {
    #[inline]
    fn from(value: NoteType) -> Self {
        padded_word_from_felt(value.inner)
    }
}

impl TryFrom<Word> for NoteType {
    type Error = &'static str;

    #[inline]
    fn try_from(value: Word) -> Result<Self, Self::Error> {
        Ok(NoteType {
            inner: felt_from_padded_word(value)?,
        })
    }
}

/// The partial hash of a storage slot name.
///
/// A slot id consists of two field elements: a `prefix` and a `suffix`.
///
/// Slot ids uniquely identify slots in account storage and are used by the host functions exposed
/// via `miden::protocol::*`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct StorageSlotId {
    suffix: Felt,
    prefix: Felt,
}

impl StorageSlotId {
    /// Creates a new [`StorageSlotId`] from the provided felts.
    ///
    /// Note: this constructor takes `(suffix, prefix)` to match the values returned by
    /// `miden_protocol::account::StorageSlotId::{suffix,prefix}`.
    pub fn new(suffix: Felt, prefix: Felt) -> Self {
        Self { suffix, prefix }
    }

    /// Creates a new [`StorageSlotId`] from the provided felts in host-call order.
    ///
    /// Host functions take the `prefix` first and then the `suffix`.
    pub fn from_prefix_suffix(prefix: Felt, suffix: Felt) -> Self {
        Self { suffix, prefix }
    }

    /// Returns the `(prefix, suffix)` pair in host-call order.
    pub fn to_prefix_suffix(&self) -> (Felt, Felt) {
        (self.prefix, self.suffix)
    }

    /// Returns the `(suffix, prefix)` pair in storage-slot order.
    pub fn to_suffix_prefix(&self) -> (Felt, Felt) {
        (self.suffix, self.prefix)
    }

    /// Returns the suffix of the [`StorageSlotId`].
    pub fn suffix(&self) -> Felt {
        self.suffix
    }

    /// Returns the prefix of the [`StorageSlotId`].
    pub fn prefix(&self) -> Felt {
        self.prefix
    }
}
