//! Contains procedures for computing hashes using BLAKE3 and SHA256 hash
//! functions. The input and output elements are assumed to contain one 32-bit
//! value per element.

use alloc::vec::Vec;

use crate::{
    felt,
    intrinsics::{assert_eq, Digest, Felt, Word},
};

#[link(wasm_import_module = "miden:core-import/stdlib-crypto-hashes-blake3@1.0.0")]
extern "C" {
    /// Computes BLAKE3 1-to-1 hash.
    ///
    /// Input: 32-bytes stored in the first 8 elements of the stack (32 bits per element).
    /// Output: A 32-byte digest stored in the first 8 elements of stack (32 bits per element).
    /// The output is passed back to the caller via a pointer.
    #[link_name = "hash-one-to-one"]
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
    #[link_name = "hash-two-to-one"]
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

#[link(wasm_import_module = "miden:core-import/stdlib-crypto-hashes-sha256@1.0.0")]
extern "C" {
    /// Computes SHA256 1-to-1 hash.
    ///
    /// Input: 32-bytes stored in the first 8 elements of the stack (32 bits per element).
    /// Output: A 32-byte digest stored in the first 8 elements of stack (32 bits per element).
    /// The output is passed back to the caller via a pointer.
    #[link_name = "sha256-hash-one-to-one"]
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
    #[link_name = "sha256-hash-two-to-one"]
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

#[link(wasm_import_module = "miden:core-import/stdlib-crypto-hashes-rpo@1.0.0")]
extern "C" {
    /// Computes the hash of a sequence of field elements using the Rescue Prime Optimized (RPO)
    /// hash function.
    ///
    /// This maps to the `std::crypto::rpo::hash_memory` procedure in the Miden stdlib.
    ///
    /// Input: A pointer to the memory location and the number of elements to hash
    /// Output: One digest (4 field elements)
    /// The output is passed back to the caller via a pointer.
    #[link_name = "hash-memory"]
    pub fn extern_hash_memory(ptr: u32, num_elements: u32, result_ptr: *mut Felt);
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
    hash_1to1(input, extern_sha256_hash_1to1)
}

/// Hashes a 64-byte input to a 32-byte output using the SHA256 hash function.
#[inline]
pub fn sha256_hash_2to1(input: [u8; 64]) -> [u8; 32] {
    hash_2to1(input, extern_sha256_hash_2to1)
}

/// Computes the hash of a sequence of field elements using the Rescue Prime Optimized (RPO)
/// hash function.
///
/// This maps to the `std::crypto::rpo::hash_memory` procedure in the Miden stdlib.
///
/// # Arguments
/// * `elements` - A slice of field elements to be hashed
#[inline]
pub fn hash_elements(elements: Vec<Felt>) -> Digest {
    let rust_ptr = elements.as_ptr().addr() as u32;

    unsafe {
        let mut ret_area = core::mem::MaybeUninit::<Word>::uninit();
        let result_ptr = ret_area.as_mut_ptr() as *mut Felt;
        let miden_ptr = rust_ptr / 4;
        // Since our BumpAlloc produces word-aligned allocations the pointer should be word-aligned
        assert_eq(Felt::from_u32(miden_ptr % 4), felt!(0));

        extern_hash_memory(miden_ptr, elements.len() as u32, result_ptr);

        Digest::from_word(ret_area.assume_init().reverse())
    }
}
