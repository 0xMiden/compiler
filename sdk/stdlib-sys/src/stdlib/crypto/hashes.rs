//! Contains procedures for computing hashes using BLAKE3 and SHA256 hash
//! functions. The input and output elements are assumed to contain one 32-bit
//! value per element.

use alloc::vec::Vec;

use crate::{
    felt,
    intrinsics::{Digest, Felt, Word, assert_eq},
};

unsafe extern "C" {
    /// Computes BLAKE3 1-to-1 hash.
    ///
    /// Input: 32-bytes stored in the first 8 elements of the stack (32 bits per element).
    /// Output: A 32-byte digest stored in the first 8 elements of stack (32 bits per element).
    /// The output is passed back to the caller via a pointer.
    #[link_name = "std::crypto::hashes::blake3::hash_1to1"]
    fn extern_blake3_hash_1to1(
        e1: u32,
        e2: u32,
        e3: u32,
        e4: u32,
        e5: u32,
        e6: u32,
        e7: u32,
        e8: u32,
        ptr: *mut u8,
    );

    /// Computes BLAKE3 2-to-1 hash.
    ///
    /// Input: 64-bytes stored in the first 16 elements of the stack (32 bits per element).
    /// Output: A 32-byte digest stored in the first 8 elements of stack (32 bits per element)
    /// The output is passed back to the caller via a pointer.
    #[link_name = "std::crypto::hashes::blake3::hash_2to1"]
    fn extern_blake3_hash_2to1(
        e1: u32,
        e2: u32,
        e3: u32,
        e4: u32,
        e5: u32,
        e6: u32,
        e7: u32,
        e8: u32,
        e9: u32,
        e10: u32,
        e11: u32,
        e12: u32,
        e13: u32,
        e14: u32,
        e15: u32,
        e16: u32,
        ptr: *mut u8,
    );
}

unsafe extern "C" {
    /// Computes SHA256 1-to-1 hash.
    ///
    /// Input: 32-bytes stored in the first 8 elements of the stack (32 bits per element).
    /// Output: A 32-byte digest stored in the first 8 elements of stack (32 bits per element).
    /// The output is passed back to the caller via a pointer.
    #[link_name = "std::crypto::hashes::sha256::hash_1to1"]
    fn extern_sha256_hash_1to1(
        e1: u32,
        e2: u32,
        e3: u32,
        e4: u32,
        e5: u32,
        e6: u32,
        e7: u32,
        e8: u32,
        ptr: *mut u8,
    );

    /// Computes SHA256 2-to-1 hash.
    ///
    /// Input: 64-bytes stored in the first 16 elements of the stack (32 bits per element).
    /// Output: A 32-byte digest stored in the first 8 elements of stack (32 bits per element).
    /// The output is passed back to the caller via a pointer.
    #[link_name = "std::crypto::hashes::sha256::hash_2to1"]
    fn extern_sha256_hash_2to1(
        e1: u32,
        e2: u32,
        e3: u32,
        e4: u32,
        e5: u32,
        e6: u32,
        e7: u32,
        e8: u32,
        e9: u32,
        e10: u32,
        e11: u32,
        e12: u32,
        e13: u32,
        e14: u32,
        e15: u32,
        e16: u32,
        ptr: *mut u8,
    );
}

unsafe extern "C" {
    /// Computes the hash of a sequence of field elements using the Rescue Prime Optimized (RPO)
    /// hash function.
    ///
    /// This maps to the `std::crypto::rpo::hash_memory` procedure in the Miden stdlib.
    ///
    /// Input: A pointer to the memory location and the number of elements to hash
    /// Output: One digest (4 field elements)
    /// The output is passed back to the caller via a pointer.
    #[link_name = "std::crypto::hashes::rpo::hash_memory"]
    pub fn extern_hash_memory(ptr: u32, num_elements: u32, result_ptr: *mut Felt);

    /// Computes the hash of a sequence of words using the Rescue Prime Optimized (RPO) hash
    /// function.
    ///
    /// This maps to the `std::crypto::hashes::rpo::hash_memory_words` procedure in the Miden
    /// stdlib.
    ///
    /// Input: The start and end addresses (in field elements) of the words to hash.
    /// Output: One digest (4 field elements)
    /// The output is passed back to the caller via a pointer.
    #[link_name = "std::crypto::hashes::rpo::hash_memory_words"]
    pub fn extern_hash_memory_words(start_addr: u32, end_addr: u32, result_ptr: *mut Felt);
}

/// Hashes a 32-byte input to a 32-byte output using the given hash function.
#[inline(always)]
fn hash_1to1(
    input: [u8; 32],
    extern_hash_1to1: unsafe extern "C" fn(u32, u32, u32, u32, u32, u32, u32, u32, *mut u8),
) -> [u8; 32] {
    use crate::intrinsics::WordAligned;
    let input = unsafe { core::mem::transmute::<[u8; 32], [u32; 8]>(input) };
    unsafe {
        let mut ret_area = ::core::mem::MaybeUninit::<WordAligned<[u8; 32]>>::uninit();
        let ptr = ret_area.as_mut_ptr() as *mut u8;
        extern_hash_1to1(
            input[0], input[1], input[2], input[3], input[4], input[5], input[6], input[7], ptr,
        );
        ret_area.assume_init().into_inner()
    }
}

/// Hashes a 64-byte input to a 32-byte output using the given hash function.
#[inline(always)]
fn hash_2to1(
    input: [u8; 64],
    extern_hash_2to1: unsafe extern "C" fn(
        u32,
        u32,
        u32,
        u32,
        u32,
        u32,
        u32,
        u32,
        u32,
        u32,
        u32,
        u32,
        u32,
        u32,
        u32,
        u32,
        *mut u8,
    ),
) -> [u8; 32] {
    let input = unsafe { core::mem::transmute::<[u8; 64], [u32; 16]>(input) };
    unsafe {
        let mut ret_area = ::core::mem::MaybeUninit::<[u8; 32]>::uninit();
        let ptr = ret_area.as_mut_ptr() as *mut u8;
        extern_hash_2to1(
            input[0], input[1], input[2], input[3], input[4], input[5], input[6], input[7],
            input[8], input[9], input[10], input[11], input[12], input[13], input[14], input[15],
            ptr,
        );
        ret_area.assume_init()
    }
}

/// Hashes a 32-byte input to a 32-byte output using the BLAKE3 hash function.
#[inline]
pub fn blake3_hash_1to1(input: [u8; 32]) -> [u8; 32] {
    hash_1to1(input, extern_blake3_hash_1to1)
}

/// Hashes a 64-byte input to a 32-byte output using the BLAKE3 hash function.
#[inline]
pub fn blake3_hash_2to1(input: [u8; 64]) -> [u8; 32] {
    hash_2to1(input, extern_blake3_hash_2to1)
}

/// Hashes a 32-byte input to a 32-byte output using the SHA256 hash function.
#[inline]
pub fn sha256_hash_1to1(input: [u8; 32]) -> [u8; 32] {
    use crate::intrinsics::WordAligned;

    let swapped_words = {
        let mut be_bytes = input;
        // The SHA-2 family is specified over big-endian 32-bit words. The Miden ABI mirrors that
        // spec, so each lane we pass across the boundary must be encoded as a big-endian word.
        // Our public Rust API uses `[u8; 32]` in native little-endian order, so we convert the bytes
        // here before calling into the ABI.
        for chunk in be_bytes.chunks_exact_mut(4) {
            chunk.reverse();
        }
        unsafe { core::mem::transmute::<[u8; 32], [u32; 8]>(be_bytes) }
    };

    let [w0, w1, w2, w3, w4, w5, w6, w7] = swapped_words;

    unsafe {
        let mut ret_area = ::core::mem::MaybeUninit::<WordAligned<[u8; 32]>>::uninit();
        let ptr = ret_area.as_mut_ptr() as *mut u8;
        extern_sha256_hash_1to1(w0, w1, w2, w3, w4, w5, w6, w7, ptr);
        let mut output = ret_area.assume_init().into_inner();
        // The extern returns the digest as big-endian words as well; flip each lane so callers see
        // the conventional Rust `[u8; 32]` ordering.
        for chunk in output.chunks_exact_mut(4) {
            chunk.reverse();
        }
        output
    }
}

/// Hashes a 64-byte input to a 32-byte output using the SHA256 hash function.
#[inline]
pub fn sha256_hash_2to1(input: [u8; 64]) -> [u8; 32] {
    use crate::intrinsics::WordAligned;

    let swapped_words = {
        let mut be_bytes = input;
        // Same story as `sha256_hash_1to1`: adjust the byte layout so the ABI receives big-endian
        // 32-bit words.
        for chunk in be_bytes.chunks_exact_mut(4) {
            chunk.reverse();
        }
        unsafe { core::mem::transmute::<[u8; 64], [u32; 16]>(be_bytes) }
    };

    let [w0, w1, w2, w3, w4, w5, w6, w7, w8, w9, w10, w11, w12, w13, w14, w15] = swapped_words;

    unsafe {
        let mut ret_area = ::core::mem::MaybeUninit::<WordAligned<[u8; 32]>>::uninit();
        let ptr = ret_area.as_mut_ptr() as *mut u8;
        extern_sha256_hash_2to1(
            w0, w1, w2, w3, w4, w5, w6, w7, w8, w9, w10, w11, w12, w13, w14, w15, ptr,
        );
        let mut output = ret_area.assume_init().into_inner();
        // Restore the little-endian byte layout expected by Rust callers.
        for chunk in output.chunks_exact_mut(4) {
            chunk.reverse();
        }
        output
    }
}

/// Computes the hash of a sequence of field elements using the Rescue Prime Optimized (RPO)
/// hash function.
///
/// This maps to the `std::crypto::rpo::hash_memory` procedure in the Miden stdlib and to the
/// `std::crypto::hashes::rpo::hash_memory_words` word-optimized variant when the input length is a
/// multiple of 4.
///
/// # Arguments
/// * `elements` - A Vec of field elements to be hashed
#[inline]
pub fn hash_elements(elements: Vec<Felt>) -> Digest {
    let rust_ptr = elements.as_ptr().addr() as u32;
    let element_count = elements.len();
    let num_elements = element_count as u32;

    unsafe {
        let mut ret_area = core::mem::MaybeUninit::<Word>::uninit();
        let result_ptr = ret_area.as_mut_ptr() as *mut Felt;
        let miden_ptr = rust_ptr / 4;
        // Since our BumpAlloc produces word-aligned allocations the pointer should be word-aligned
        assert_eq(Felt::from_u32(miden_ptr % 4), felt!(0));

        if element_count.is_multiple_of(4) {
            let start_addr = miden_ptr;
            let end_addr = start_addr + num_elements;
            extern_hash_memory_words(start_addr, end_addr, result_ptr);
        } else {
            extern_hash_memory(miden_ptr, num_elements, result_ptr);
        }

        Digest::from_word(ret_area.assume_init().reverse())
    }
}

/// Computes the hash of a sequence of words using the Rescue Prime Optimized (RPO)
/// hash function.
///
/// This maps to the `std::crypto::hashes::rpo::hash_memory_words` procedure in the Miden stdlib.
///
/// # Arguments
/// * `words` - A slice of words to be hashed
#[inline]
pub fn hash_words(words: &[Word]) -> Digest {
    let rust_ptr = words.as_ptr().addr() as u32;

    unsafe {
        let mut ret_area = core::mem::MaybeUninit::<Word>::uninit();
        let result_ptr = ret_area.as_mut_ptr() as *mut Felt;
        let miden_ptr = rust_ptr / 4;
        // It's safe to assume the `words` ptr is word-aligned.
        assert_eq(Felt::from_u32(miden_ptr % 4), felt!(0));

        let start_addr = miden_ptr;
        let end_addr = start_addr + (words.len() as u32 * 4);
        extern_hash_memory_words(start_addr, end_addr, result_ptr);

        Digest::from_word(ret_area.assume_init().reverse())
    }
}
