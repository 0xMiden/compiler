#![allow(clippy::infallible_try_from)]

extern crate alloc;

use alloc::vec::Vec;
use core::convert::Infallible;

use miden_field_repr::FromFeltRepr;
use miden_stdlib_sys::{Digest, Felt, Word, hash_elements, intrinsics::crypto::merge};

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
        Word::from([
            Felt::from_u64_unchecked(0),
            Felt::from_u64_unchecked(0),
            value.suffix,
            value.prefix,
        ])
    }
}

impl TryFrom<Word> for AccountId {
    type Error = &'static str;

    #[inline]
    fn try_from(value: Word) -> Result<Self, Self::Error> {
        if value[0] != Felt::from(0u32) || value[1] != Felt::from(0u32) {
            return Err("expected zero padding in the upper two felts");
        }

        Ok(Self {
            prefix: value[3],
            suffix: value[2],
        })
    }
}

/// A fungible or a non-fungible asset.
///
/// All assets are encoded using a single word (4 elements) such that it is easy to determine the
/// type of an asset both inside and outside Miden VM. Specifically:
///
/// Element 1 of the asset will be:
/// - ZERO for a fungible asset.
/// - non-ZERO for a non-fungible asset.
///
/// Element 3 of both asset types is the prefix of an
/// [`AccountId`], which can be used to distinguish assets.
///
/// The methodology for constructing fungible and non-fungible assets is described below.
///
/// # Fungible assets
///
/// - A fungible asset's data layout is: `[amount, 0, faucet_id_suffix, faucet_id_prefix]`.
///
/// # Non-fungible assets
///
/// - A non-fungible asset's data layout is: `[hash0, hash1, hash2, faucet_id_prefix]`.
///
/// The 4 elements of non-fungible assets are computed as follows:
/// - First the asset data is hashed. This compresses an asset of an arbitrary length to 4 field
///   elements: `[hash0, hash1, hash2, hash3]`.
/// - `hash3` is then replaced with the prefix of the faucet ID (`faucet_id_prefix`) which issues
///   the asset: `[hash0, hash1, hash2, faucet_id_prefix]`.
///
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[repr(transparent)]
pub struct Asset {
    pub inner: Word,
}

impl Asset {
    pub fn new(word: impl Into<Word>) -> Self {
        Asset { inner: word.into() }
    }

    pub fn as_word(&self) -> &Word {
        &self.inner
    }

    #[inline]
    pub(crate) fn reversed(&self) -> Self {
        Self {
            inner: self.inner.reversed(),
        }
    }
}

impl TryFrom<Word> for Asset {
    type Error = Infallible;

    fn try_from(value: Word) -> Result<Self, Self::Error> {
        Ok(Self::new(value))
    }
}

impl From<[Felt; 4]> for Asset {
    fn from(value: [Felt; 4]) -> Self {
        Asset::new(Word::from(value))
    }
}

impl From<Asset> for Word {
    fn from(val: Asset) -> Self {
        val.inner
    }
}

impl AsRef<Word> for Asset {
    fn as_ref(&self) -> &Word {
        &self.inner
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

    #[inline]
    pub(crate) fn reverse(self) -> Self {
        Self {
            attachment: self.attachment.reverse(),
            header: self.header.reverse(),
        }
    }
}

impl From<[Felt; 4]> for Recipient {
    fn from(value: [Felt; 4]) -> Self {
        Recipient {
            inner: Word::from(value),
        }
    }
}

impl TryFrom<Word> for Recipient {
    type Error = Infallible;

    fn try_from(value: Word) -> Result<Self, Self::Error> {
        Ok(Recipient { inner: value })
    }
}

impl From<Recipient> for Word {
    #[inline]
    fn from(value: Recipient) -> Self {
        value.inner
    }
}

impl AsRef<Word> for Recipient {
    fn as_ref(&self) -> &Word {
        &self.inner
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
        Word::from(value.inner)
    }
}

impl TryFrom<Word> for Tag {
    type Error = &'static str;

    #[inline]
    fn try_from(value: Word) -> Result<Self, Self::Error> {
        Ok(Tag {
            inner: value.try_into()?,
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
        Word::from(value.inner)
    }
}

impl TryFrom<Word> for NoteIdx {
    type Error = &'static str;

    #[inline]
    fn try_from(value: Word) -> Result<Self, Self::Error> {
        Ok(NoteIdx {
            inner: value.try_into()?,
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
        Word::from(value.inner)
    }
}

impl TryFrom<Word> for NoteType {
    type Error = &'static str;

    #[inline]
    fn try_from(value: Word) -> Result<Self, Self::Error> {
        Ok(NoteType {
            inner: value.try_into()?,
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

    /// Returns the suffix of the [`StorageSlotId`].
    pub fn suffix(&self) -> Felt {
        self.suffix
    }

    /// Returns the prefix of the [`StorageSlotId`].
    pub fn prefix(&self) -> Felt {
        self.prefix
    }
}

#[cfg(test)]
mod tests {
    use super::{AccountId, Felt, NoteIdx, NoteType, Tag, Word};

    #[test]
    fn account_id_try_from_word_rejects_non_zero_padding() {
        let word =
            Word::from([Felt::from(1u32), Felt::from(0u32), Felt::from(2u32), Felt::from(3u32)]);

        assert_eq!(AccountId::try_from(word), Err("expected zero padding in the upper two felts"));
    }

    #[test]
    fn single_felt_wrappers_reject_non_zero_padding() {
        let word =
            Word::from([Felt::from(0u32), Felt::from(1u32), Felt::from(0u32), Felt::from(9u32)]);

        assert_eq!(Tag::try_from(word), Err("expected zero padding in the upper three felts"));
        assert_eq!(NoteIdx::try_from(word), Err("expected zero padding in the upper three felts"));
        assert_eq!(NoteType::try_from(word), Err("expected zero padding in the upper three felts"));
    }
}
