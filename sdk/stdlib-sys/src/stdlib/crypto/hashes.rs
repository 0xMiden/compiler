//! Contains procedures for computing hashes using BLAKE3 and SHA256 hash
//! functions. The input and output elements are assumed to contain one 32-bit
//! value per element.

#[cfg(all(target_family = "wasm", miden))]
mod imp {
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
        #[link_name = "miden::core::crypto::hashes::blake3::hash"]
        fn extern_blake3_hash(
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
        #[link_name = "miden::core::crypto::hashes::blake3::merge"]
        fn extern_blake3_merge(
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
        #[link_name = "miden::core::crypto::hashes::sha256::hash"]
        fn extern_sha256_hash(
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
        #[link_name = "miden::core::crypto::hashes::sha256::merge"]
        fn extern_sha256_merge(
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
        /// This maps to the `miden::core::crypto::hashes::poseidon2::hash_elements` procedure.
        ///
        /// Input: A pointer to the memory location and the number of elements to hash
        /// Output: One digest (4 field elements)
        /// The output is passed back to the caller via a pointer.
        #[link_name = "miden::core::crypto::hashes::poseidon2::hash_elements"]
        pub fn extern_hash_elements(ptr: u32, num_elements: u32, result_ptr: *mut Felt);

        /// Computes the hash of a sequence of words using the Rescue Prime Optimized (RPO) hash
        /// function.
        ///
        /// This maps to the `miden::core::crypto::hashes::poseidon2::hash_words` procedure.
        ///
        /// Input: The start and end addresses (in field elements) of the words to hash.
        /// Output: One digest (4 field elements)
        /// The output is passed back to the caller via a pointer.
        #[link_name = "miden::core::crypto::hashes::poseidon2::hash_words"]
        pub fn extern_hash_words(start_addr: u32, end_addr: u32, result_ptr: *mut Felt);
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

    #[inline(always)]
    fn decode_be_lanes_in_place(bytes: &mut [u8]) {
        for chunk in bytes.chunks_exact_mut(4) {
            chunk.reverse();
        }
    }

    /// Hashes a 32-byte input to a 32-byte output using the BLAKE3 hash function.
    #[inline]
    pub fn blake3_hash(input: [u8; 32]) -> [u8; 32] {
        use crate::intrinsics::WordAligned;

        let lanes = bytes_to_u32_le_8(input);
        unsafe {
            let mut ret_area = ::core::mem::MaybeUninit::<WordAligned<[u8; 32]>>::uninit();
            let ptr = ret_area.as_mut_ptr() as *mut u8;
            extern_blake3_hash(
                lanes[0], lanes[1], lanes[2], lanes[3], lanes[4], lanes[5], lanes[6], lanes[7], ptr,
            );
            ret_area.assume_init().into_inner()
        }
    }

    /// Hashes a 64-byte input to a 32-byte output using the BLAKE3 hash function.
    #[inline]
    pub fn blake3_merge(input: [u8; 64]) -> [u8; 32] {
        let lanes = bytes_to_u32_le_16(input);
        unsafe {
            let mut ret_area = ::core::mem::MaybeUninit::<[u8; 32]>::uninit();
            let ptr = ret_area.as_mut_ptr() as *mut u8;
            extern_blake3_merge(
                lanes[0], lanes[1], lanes[2], lanes[3], lanes[4], lanes[5], lanes[6], lanes[7],
                lanes[8], lanes[9], lanes[10], lanes[11], lanes[12], lanes[13], lanes[14],
                lanes[15], ptr,
            );
            ret_area.assume_init()
        }
    }

    /// Hashes a 32-byte input to a 32-byte output using the SHA256 hash function.
    #[inline]
    pub fn sha256_hash(input: [u8; 32]) -> [u8; 32] {
        use crate::intrinsics::WordAligned;

        let lanes = bytes_to_u32_be_8(input);
        unsafe {
            let mut ret_area = ::core::mem::MaybeUninit::<WordAligned<[u8; 32]>>::uninit();
            let ptr = ret_area.as_mut_ptr() as *mut u8;
            extern_sha256_hash(
                lanes[0], lanes[1], lanes[2], lanes[3], lanes[4], lanes[5], lanes[6], lanes[7], ptr,
            );
            let mut output = ret_area.assume_init().into_inner();
            decode_be_lanes_in_place(&mut output);
            output
        }
    }

    /// Hashes a 64-byte input to a 32-byte output using the SHA256 hash function.
    #[inline]
    pub fn sha256_merge(input: [u8; 64]) -> [u8; 32] {
        let lanes = bytes_to_u32_be_16(input);
        unsafe {
            let mut ret_area = ::core::mem::MaybeUninit::<[u8; 32]>::uninit();
            let ptr = ret_area.as_mut_ptr() as *mut u8;
            extern_sha256_merge(
                lanes[0], lanes[1], lanes[2], lanes[3], lanes[4], lanes[5], lanes[6], lanes[7],
                lanes[8], lanes[9], lanes[10], lanes[11], lanes[12], lanes[13], lanes[14],
                lanes[15], ptr,
            );
            let mut output = ret_area.assume_init();
            decode_be_lanes_in_place(&mut output);
            output
        }
    }

    /// Computes the hash of a sequence of field elements using the Rescue Prime Optimized (RPO)
    /// hash function.
    ///
    /// This maps to the `miden::core::crypto::hashes::poseidon2::hash_elements` procedure and to the
    /// `miden::core::crypto::hashes::poseidon2::hash_words` word-optimized variant when the input
    /// length is a multiple of 4.
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
            assert_eq(Felt::new((miden_ptr % 4) as u64), felt!(0));

            if element_count.is_multiple_of(4) {
                let start_addr = miden_ptr;
                let end_addr = start_addr + num_elements;
                extern_hash_words(start_addr, end_addr, result_ptr);
            } else {
                extern_hash_elements(miden_ptr, num_elements, result_ptr);
            }

            Digest::from_word(ret_area.assume_init())
        }
    }

    /// Computes the hash of a sequence of words using the Rescue Prime Optimized (RPO)
    /// hash function.
    ///
    /// This maps to the `miden::core::crypto::hashes::poseidon2::hash_words` procedure.
    ///
    /// # Arguments
    /// * `words` - A slice of words to be hashed
    #[inline]
    pub fn hash_words(words: &[Word]) -> Digest {
        let rust_ptr = words.as_ptr().addr() as u32;

        let miden_ptr = rust_ptr / 4;
        // It's safe to assume the `words` ptr is word-aligned.
        assert_eq(Felt::new((miden_ptr % 4) as u64), felt!(0));

        unsafe {
            let mut ret_area = core::mem::MaybeUninit::<Word>::uninit();
            let result_ptr = ret_area.as_mut_ptr() as *mut Felt;
            let start_addr = miden_ptr;
            let end_addr = start_addr + (words.len() as u32 * 4);
            extern_hash_words(start_addr, end_addr, result_ptr);

            Digest::from_word(ret_area.assume_init())
        }
    }
}

#[cfg(not(all(target_family = "wasm", miden)))]
mod imp {
    use alloc::vec::Vec;

    use crate::intrinsics::{Digest, Felt, Word};

    /// Computes BLAKE3 1-to-1 hash.
    #[inline]
    pub fn blake3_hash(_input: [u8; 32]) -> [u8; 32] {
        unimplemented!(
            "miden::core::crypto::hashes bindings are only available when targeting the Miden VM"
        )
    }

    /// Computes BLAKE3 2-to-1 hash.
    #[inline]
    pub fn blake3_merge(_input: [u8; 64]) -> [u8; 32] {
        unimplemented!(
            "miden::core::crypto::hashes bindings are only available when targeting the Miden VM"
        )
    }

    /// Computes SHA256 1-to-1 hash.
    #[inline]
    pub fn sha256_hash(_input: [u8; 32]) -> [u8; 32] {
        unimplemented!(
            "miden::core::crypto::hashes bindings are only available when targeting the Miden VM"
        )
    }

    /// Computes SHA256 2-to-1 hash.
    #[inline]
    pub fn sha256_merge(_input: [u8; 64]) -> [u8; 32] {
        unimplemented!(
            "miden::core::crypto::hashes bindings are only available when targeting the Miden VM"
        )
    }

    /// Computes the hash of a sequence of field elements using the Rescue Prime Optimized (RPO)
    /// hash function.
    #[inline]
    pub fn hash_elements(_elements: Vec<Felt>) -> Digest {
        unimplemented!(
            "miden::core::crypto::hashes bindings are only available when targeting the Miden VM"
        )
    }

    /// Computes the hash of a sequence of words using the Rescue Prime Optimized (RPO) hash
    /// function.
    #[inline]
    pub fn hash_words(_words: &[Word]) -> Digest {
        unimplemented!(
            "miden::core::crypto::hashes bindings are only available when targeting the Miden VM"
        )
    }

    /// ABI helper for `miden::core::crypto::hashes::poseidon2::hash_elements`.
    #[inline]
    pub fn extern_hash_elements(_ptr: u32, _num_elements: u32, _result_ptr: *mut Felt) {
        unimplemented!(
            "miden::core::crypto::hashes bindings are only available when targeting the Miden VM"
        )
    }

    /// ABI helper for `miden::core::crypto::hashes::poseidon2::hash_words`.
    #[inline]
    pub fn extern_hash_words(_start_addr: u32, _end_addr: u32, _result_ptr: *mut Felt) {
        unimplemented!(
            "miden::core::crypto::hashes bindings are only available when targeting the Miden VM"
        )
    }
}

pub use imp::*;
