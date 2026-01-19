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

// Remove WIT import module and resolve via a linker stub instead. The stub will export
// the MASM symbol `intrinsics::crypto::hmerge`, and the frontend will lower its
// unreachable body to a MASM exec.
#[cfg(all(target_family = "wasm", miden))]
unsafe extern "C" {
    /// Computes the hash of two digests using the Rescue Prime Optimized (RPO)
    /// permutation in 2-to-1 mode.
    ///
    /// This is the `hmerge` instruction in the Miden VM.
    ///
    /// Input: Pointer to an array of two digests (8 field elements total)
    /// Output: One digest (4 field elements) written to the result pointer
    #[link_name = "intrinsics::crypto::hmerge"]
    fn extern_hmerge(
        // Pointer to array of two digests
        digests_ptr: *const Felt,
        // Result pointer
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
        let result_ptr = ret_area.as_mut_ptr().addr() as u32;

        let digests_ptr = digests.as_ptr().addr() as u32;
        extern_hmerge(digests_ptr as *const Felt, result_ptr as *mut Felt);

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
