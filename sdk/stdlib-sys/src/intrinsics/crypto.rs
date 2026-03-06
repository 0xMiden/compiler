//! Cryptographic intrinsics for the Miden VM.
//!
//! This module provides Rust bindings for cryptographic operations available in the Miden VM.
#![allow(warnings)]

use crate::intrinsics::{Felt, Word};

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

impl From<Word> for Digest {
    #[inline]
    fn from(word: Word) -> Self {
        Self::from_word(word)
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
        digest.inner.into()
    }
}

#[cfg(all(target_family = "wasm", miden))]
unsafe extern "C" {
    /// Merges two words (256-bit digests) via Poseidon2.
    ///
    /// This maps to `miden::core::crypto::hashes::poseidon2::merge`.
    ///
    /// Inputs:  `[A, B, ...]`
    /// Outputs: `[C, ...]` where `C = Poseidon2(A || B)`
    ///
    /// The digest output is returned to the caller via `result_ptr`.
    #[link_name = "miden::core::crypto::hashes::poseidon2::merge"]
    fn extern_poseidon2_merge(
        a0: Felt,
        a1: Felt,
        a2: Felt,
        a3: Felt,
        b0: Felt,
        b1: Felt,
        b2: Felt,
        b3: Felt,
        result_ptr: *mut Felt,
    );
}

/// Computes the hash of two digests using the Rescue Prime Optimized (RPO)
/// permutation in 2-to-1 mode.
///
/// This directly maps to the `hmerge` VM instruction.
///
/// # Arguments
/// * `digests` - An array of two digests to be merged. The function internally
///   reorders them as required by the VM instruction (from [A, B] to [B, A] on the stack).
#[inline]
#[cfg(all(target_family = "wasm", miden))]
pub fn merge(digests: [Digest; 2]) -> Digest {
    unsafe {
        let mut ret_area = ::core::mem::MaybeUninit::<Word>::uninit();
        let result_ptr = ret_area.as_mut_ptr() as *mut Felt;

        extern_poseidon2_merge(
            digests[0].inner.a,
            digests[0].inner.b,
            digests[0].inner.c,
            digests[0].inner.d,
            digests[1].inner.a,
            digests[1].inner.b,
            digests[1].inner.c,
            digests[1].inner.d,
            result_ptr,
        );

        Digest::from_word(ret_area.assume_init())
    }
}

/// Computes the hash of two digests using the Rescue Prime Optimized (RPO) permutation in 2-to-1
/// mode.
#[inline]
#[cfg(not(all(target_family = "wasm", miden)))]
pub fn merge(_digests: [Digest; 2]) -> Digest {
    unimplemented!("crypto intrinsics are only available when targeting the Miden VM")
}
