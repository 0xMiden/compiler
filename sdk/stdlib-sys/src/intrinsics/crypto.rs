//! Cryptographic intrinsics for the Miden VM

use crate::{Felt, Word};

pub type Digest = crate::Word;

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
pub fn merge(digests: &[Word; 2]) -> Word {
    unsafe {
        let mut result = [Felt::from_u32(0); 4];
        let ptr = result.as_mut_ptr();

        let d1: [Felt; 4] = digests[0].clone().into();
        let d2: [Felt; 4] = digests[1].clone().into();

        extern_hmerge(
            d1[0].inner,
            d1[1].inner,
            d1[2].inner,
            d1[3].inner,
            d2[0].inner,
            d2[1].inner,
            d2[2].inner,
            d2[3].inner,
            ptr,
        );

        Word::from(result)
    }
}
