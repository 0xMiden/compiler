extern crate alloc;

use alloc::vec::Vec;

use miden_felt_repr::FromFeltRepr;
use miden_stdlib_sys::{Digest, Felt, Word, felt, hash_elements, intrinsics::crypto::merge};

/// Unique identifier for a Miden account, composed of two field elements.
#[derive(Copy, Clone, Debug, PartialEq, Eq, FromFeltRepr)]
pub struct AccountId {
    pub prefix: Felt,
    pub suffix: Felt,
}

impl AccountId {
    /// Creates a new AccountId from prefix and suffix Felt values.
    pub const fn new(prefix: Felt, suffix: Felt) -> Self {
        Self { prefix, suffix }
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
    pub(crate) fn reverse(&self) -> Self {
        Self {
            inner: self.inner.reverse(),
        }
    }
}

impl From<Word> for Asset {
    fn from(value: Word) -> Self {
        Self::new(value)
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
    /// The `padded_inputs` must be padded with ZEROs to the next multiple of 8 (i.e. 2-word
    /// aligned). For example, to pass two inputs `a` and `b`, use:
    /// `vec![a, b, felt!(0), felt!(0), felt!(0), felt!(0), felt!(0), felt!(0)]`.
    ///
    /// # Panics
    /// Panics if `padded_inputs.len()` is not a multiple of 8.
    pub fn compute(serial_num: Word, script_digest: Digest, padded_inputs: Vec<Felt>) -> Self {
        assert!(
            padded_inputs.len().is_multiple_of(8),
            "`padded_inputs` length must be a multiple of 8"
        );

        let empty_word = Word::new([felt!(0), felt!(0), felt!(0), felt!(0)]);

        let serial_num_hash = merge([Digest::from_word(serial_num), Digest::from_word(empty_word)]);
        let merge_script = merge([serial_num_hash, script_digest]);
        let digest: Word = merge([merge_script, hash_elements(padded_inputs)]).into();

        Self { inner: digest }
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(transparent)]
pub struct NoteIdx {
    pub inner: Felt,
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

#[derive(Clone, Debug, PartialEq, Eq)]
#[repr(transparent)]
pub struct StorageCommitmentRoot(Word);

impl StorageCommitmentRoot {
    #[inline]
    pub(crate) fn reverse(&self) -> StorageCommitmentRoot {
        Self(self.0.reverse())
    }
}
