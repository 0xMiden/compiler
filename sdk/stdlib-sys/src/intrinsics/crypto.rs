//! Cryptographic intrinsics for the Miden VM

use crate::{Felt, Word};

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

#[link(wasm_import_module = "miden:core-intrinsics/intrinsics-crypto@1.0.0")]
extern "C" {
    /// Computes the hash of two digests using the Rescue Prime Optimized (RPO)
    /// permutation in 2-to-1 mode.
    ///
    /// This is the `hmerge` instruction in the Miden VM.
    ///
    /// Input: Two digests (4 field elements each)
    /// Output: One digest (4 field elements)
    /// The output is passed back to the caller via a pointer.
    #[link_name = "hmerge"]
    fn extern_hmerge(
        // First digest (4 felts)
        d1_0: f32,
        d1_1: f32,
        d1_2: f32,
        d1_3: f32,
        // Second digest (4 felts)
        d2_0: f32,
        d2_1: f32,
        d2_2: f32,
        d2_3: f32,
        // Result pointer
        ptr: *mut Felt,
    );
}

/// Computes the hash of two digests using the Rescue Prime Optimized (RPO)
/// permutation in 2-to-1 mode.
///
/// This directly maps to the `hmerge` VM instruction.
#[inline]
pub fn merge(digests: [Digest; 2]) -> Digest {
    unsafe {
        let mut ret_area = ::core::mem::MaybeUninit::<Word>::uninit();
        let ptr = ret_area.as_mut_ptr() as *mut Felt;
        // VM hmerge intstruction expects [B, A] order
        // see
        // https://0xmiden.github.io/miden-docs/imported/miden-vm/src/user_docs/assembly/cryptographic_operations.html
        extern_hmerge(
            digests[1].inner[0].inner,
            digests[1].inner[1].inner,
            digests[1].inner[2].inner,
            digests[1].inner[3].inner,
            digests[0].inner[0].inner,
            digests[0].inner[1].inner,
            digests[0].inner[2].inner,
            digests[0].inner[3].inner,
            ptr,
        );

        // Word::from(result)
        Digest::from_word(ret_area.assume_init())
    }
}
