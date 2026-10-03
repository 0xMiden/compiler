//! Contains procedures for computing hashes using BLAKE3 and SHA256 hash
//! functions. The input and output elements are assumed to contain one 32-bit
//! value per element.

use alloc::vec::Vec;
use core::convert::Infallible;

use crate::{
    ElementPtr, Felt, Word,
    raw::core::crypto::hashes::{blake3, poseidon2 as raw_poseidon2, sha256},
};

/// A cryptographic digest representing a 256-bit hash value.
///
/// This is a wrapper around `Word` which contains 4 field elements.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Debug)]
#[repr(transparent)]
pub struct Digest {
    pub inner: Word,
}

impl Digest {
    /// Creates a new `Digest` from a `[Felt; 4]` array.
    #[inline]
    pub fn new(felts: [Felt; 4]) -> Self {
        Self {
            inner: Word::from(felts),
        }
    }

    /// Creates a new `Digest` from a `Word`.
    #[inline]
    pub const fn from_word(word: Word) -> Self {
        Self { inner: word }
    }
}

// Infallible, but `TryFrom<Word>` like the other word-backed SDK types (`AccountId`, `Tag`, ...).
#[allow(clippy::infallible_try_from)]
impl TryFrom<Word> for Digest {
    type Error = Infallible;

    #[inline]
    fn try_from(word: Word) -> Result<Self, Self::Error> {
        Ok(Self::from_word(word))
    }
}

impl From<Digest> for Word {
    #[inline]
    fn from(digest: Digest) -> Self {
        digest.inner
    }
}

impl From<[Felt; 4]> for Digest {
    #[inline]
    fn from(felts: [Felt; 4]) -> Self {
        Self::new(felts)
    }
}

impl From<Digest> for [Felt; 4] {
    #[inline]
    fn from(digest: Digest) -> Self {
        (&digest.inner).into()
    }
}

/// The Poseidon2 2-to-1 `merge` of the core library.
///
/// The module is crate-visible, so the crate root's glob re-export of this module does not make
/// `merge` public at the root. Its public path is `intrinsics::crypto::merge` (see `lib.rs`),
/// where it lived before the compiler intrinsics moved to `miden-intrinsics-sys`.
pub(crate) mod poseidon2 {
    use super::Digest;
    use crate::raw::core::crypto::hashes::poseidon2;

    /// Computes the hash of two digests using the Poseidon2 permutation in 2-to-1 mode: the hash
    /// of `digests[0] || digests[1]`.
    ///
    /// This maps to the `miden::core::crypto::hashes::poseidon2::merge` procedure, which takes
    /// the digests in the order given.
    ///
    /// # Arguments
    /// * `digests` - An array of two digests to be merged.
    #[inline]
    pub fn merge(digests: [Digest; 2]) -> Digest {
        let [a, b] = digests;
        Digest::from_word(poseidon2::merge(a.inner, b.inner))
    }
}

/// Encodes 32 bytes as 8 little-endian u32 lanes.
#[inline(always)]
fn bytes_to_u32_le_8(input: [u8; 32]) -> [u32; 8] {
    core::array::from_fn(|i| {
        let off = i * 4;
        u32::from_le_bytes([input[off], input[off + 1], input[off + 2], input[off + 3]])
    })
}

/// Encodes 64 bytes as 16 little-endian u32 lanes.
#[inline(always)]
fn bytes_to_u32_le_16(input: [u8; 64]) -> [u32; 16] {
    core::array::from_fn(|i| {
        let off = i * 4;
        u32::from_le_bytes([input[off], input[off + 1], input[off + 2], input[off + 3]])
    })
}

/// Encodes 32 bytes as 8 big-endian u32 lanes.
#[inline(always)]
fn bytes_to_u32_be_8(input: [u8; 32]) -> [u32; 8] {
    core::array::from_fn(|i| {
        let off = i * 4;
        u32::from_be_bytes([input[off], input[off + 1], input[off + 2], input[off + 3]])
    })
}

/// Encodes 64 bytes as 16 big-endian u32 lanes.
#[inline(always)]
fn bytes_to_u32_be_16(input: [u8; 64]) -> [u32; 16] {
    core::array::from_fn(|i| {
        let off = i * 4;
        u32::from_be_bytes([input[off], input[off + 1], input[off + 2], input[off + 3]])
    })
}

/// Splits 16 lanes into the two 8-lane digests a 2-to-1 hash takes, in order.
#[inline(always)]
fn split_lanes(lanes: [u32; 16]) -> ([u32; 8], [u32; 8]) {
    (core::array::from_fn(|i| lanes[i]), core::array::from_fn(|i| lanes[8 + i]))
}

/// Decodes 8 u32 lanes into 32 bytes, each lane with `to_bytes`.
#[inline(always)]
fn lanes_to_bytes(lanes: [u32; 8], to_bytes: fn(u32) -> [u8; 4]) -> [u8; 32] {
    core::array::from_fn(|i| to_bytes(lanes[i / 4])[i % 4])
}

/// Hashes a 32-byte input to a 32-byte output using the BLAKE3 hash function.
#[inline]
pub fn blake3_hash(input: [u8; 32]) -> [u8; 32] {
    let limbs = bytes_to_u32_le_8(input);
    let digest = blake3::hash(blake3::Digest { limbs });
    lanes_to_bytes(digest.limbs, u32::to_le_bytes)
}

/// Hashes a 64-byte input to a 32-byte output using the BLAKE3 hash function.
#[inline]
pub fn blake3_merge(input: [u8; 64]) -> [u8; 32] {
    let (a, b) = split_lanes(bytes_to_u32_le_16(input));
    let digest = blake3::merge(blake3::Digest { limbs: a }, blake3::Digest { limbs: b });
    lanes_to_bytes(digest.limbs, u32::to_le_bytes)
}

/// Hashes a 32-byte input to a 32-byte output using the SHA256 hash function.
#[inline]
pub fn sha256_hash(input: [u8; 32]) -> [u8; 32] {
    let limbs = bytes_to_u32_be_8(input);
    let digest = sha256::hash(sha256::Digest { limbs });
    lanes_to_bytes(digest.limbs, u32::to_be_bytes)
}

/// Hashes a 64-byte input to a 32-byte output using the SHA256 hash function.
#[inline]
pub fn sha256_merge(input: [u8; 64]) -> [u8; 32] {
    let (a, b) = split_lanes(bytes_to_u32_be_16(input));
    let digest = sha256::merge(sha256::Digest { limbs: a }, sha256::Digest { limbs: b });
    lanes_to_bytes(digest.limbs, u32::to_be_bytes)
}

/// Computes the hash of a sequence of field elements using the Poseidon2 hash function.
///
/// This maps to the `miden::core::crypto::hashes::poseidon2::hash_elements` procedure and to the
/// `miden::core::crypto::hashes::poseidon2::hash_words` word-optimized variant when the input
/// length is a multiple of 4.
///
/// # Arguments
/// * `elements` - A Vec of field elements to be hashed
#[inline]
pub fn hash_elements(elements: Vec<Felt>) -> Digest {
    // Both procedures only read the elements. `BumpAlloc` word-aligns allocations, which
    // `hash_words` requires; `ElementPtr::from_ptr` checks that the address is element-aligned.
    let start = ElementPtr::from_ptr(elements.as_ptr() as *mut Felt);
    let word = if elements.len().is_multiple_of(4) {
        // One past the last element; the elements fit in memory, so the address does not
        // overflow.
        let end = ElementPtr::new(start.addr() + elements.len() as u32);
        unsafe { raw_poseidon2::hash_words(start, end) }
    } else {
        unsafe { raw_poseidon2::hash_elements(start, elements.len() as u32) }
    };
    Digest::from_word(word)
}

/// Computes the hash of a sequence of words using the Poseidon2 hash function.
///
/// This maps to the `miden::core::crypto::hashes::poseidon2::hash_words` procedure.
///
/// # Arguments
/// * `words` - A slice of words to be hashed
#[inline]
pub fn hash_words(words: &[Word]) -> Digest {
    // The procedure only reads the words, which are word-aligned as a `Word` is. The end is one
    // past the last word, four elements a word on; the words fit in memory, so the address does
    // not overflow.
    let start = ElementPtr::from_ptr(words.as_ptr() as *mut Felt);
    let end = ElementPtr::new(start.addr() + 4 * words.len() as u32);
    let word = unsafe { raw_poseidon2::hash_words(start, end) };
    Digest::from_word(word)
}
