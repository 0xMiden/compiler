#![allow(dead_code)]

extern crate alloc;
use alloc::vec::Vec;

#[cfg(all(target_family = "wasm", miden))]
use crate::felt;
use crate::intrinsics::{Felt, Word};

#[cfg(all(target_family = "wasm", miden))]
unsafe extern "C" {

    /// Moves an arbitrary number of words from the advice stack to memory.
    ///
    /// Input: [num_words, write_ptr, ...]
    /// Output: [R0, R1, C, write_ptr', ...]
    ///
    /// Where R0, R1, C are the final hasher state, and `write_ptr'` points to the end of the
    /// copied words.
    ///
    /// Cycles:
    /// - Even num_words: 43 + 9 * num_words / 2
    /// - Odd num_words: 60 + 9 * round_down(num_words / 2)
    #[link_name = "miden::core::mem::pipe_words_to_memory"]
    fn extern_pipe_words_to_memory(num_words: Felt, write_ptr: *mut Felt, out_ptr: *mut Felt);

    /// Moves an even number of words from the advice stack to memory.
    ///
    /// Input: [R0, R1, C, write_ptr, end_ptr, ...]
    /// Output: [R0', R1', C', write_ptr, ...]
    ///
    /// Where:
    /// - The words R0, R1, and C are the hasher state (R0 on top)
    /// - C is the capacity
    /// - R0, R1 are the rate portion of the state
    /// - The value num_words = end_ptr - write_ptr must be positive and even
    ///
    /// Cycles: 9 + 6 * (num_words / 2)
    #[link_name = "miden::core::mem::pipe_double_words_to_memory"]
    fn extern_pipe_double_words_to_memory(
        r00: Felt,
        r01: Felt,
        r02: Felt,
        r03: Felt,
        r10: Felt,
        r11: Felt,
        r12: Felt,
        r13: Felt,
        c0: Felt,
        c1: Felt,
        c2: Felt,
        c3: Felt,
        write_ptr: *mut Felt,
        end_ptr: *mut Felt,
        out_ptr: *mut Felt,
    );

    /// Moves an arbitrary number of words from the advice stack to memory and asserts it matches the commitment.
    ///
    /// Input: [num_words, write_ptr, COM, ...]
    /// Output: [write_ptr', ...]
    ///
    /// Cycles:
    /// - Even num_words: 58 + 9 * (num_words / 2)
    /// - Odd num_words: 75 + 9 * round_down(num_words / 2)
    #[link_name = "miden::core::mem::pipe_preimage_to_memory"]
    pub(crate) fn extern_pipe_preimage_to_memory(
        num_words: Felt,
        write_ptr: *mut Felt,
        com0: Felt,
        com1: Felt,
        com2: Felt,
        com3: Felt,
    ) -> i32;
}

/// Reads an arbitrary number of words `num_words` from the advice stack and returns them along with
/// the digest of all read words.
///
/// Cycles:
/// - Even num_words: 43 + 9 * num_words / 2
/// - Odd num_words: 60 + 9 * round_down(num_words / 2)
#[cfg(all(target_family = "wasm", miden))]
pub fn pipe_words_to_memory(num_words: Felt) -> (Word, Vec<Felt>) {
    #[repr(C)]
    struct Result {
        r0: Word,
        r1: Word,
        c: Word,
        write_ptr: *mut Felt,
    }

    unsafe {
        let num_words_usize =
            usize::try_from(num_words.as_u64()).expect("num_words must fit in usize");
        let num_felts = num_words_usize.checked_mul(4).expect("num_words too large");

        let mut ret_area = ::core::mem::MaybeUninit::<Result>::uninit();
        let mut buf: Vec<Felt> = Vec::with_capacity(num_felts);

        let rust_write_ptr = buf.as_mut_ptr().addr();
        let rust_write_ptr_u32 = u32::try_from(rust_write_ptr).expect("write_ptr must fit in u32");
        assert_eq!(rust_write_ptr_u32 % 4, 0, "write_ptr must be word-aligned");
        let miden_write_ptr = rust_write_ptr_u32 / 4;

        extern_pipe_words_to_memory(
            num_words,
            miden_write_ptr as usize as *mut Felt,
            ret_area.as_mut_ptr() as *mut Felt,
        );
        buf.set_len(num_felts);
        let Result { r1, .. } = ret_area.assume_init();
        (r1.reverse(), buf)
    }
}

/// Reads an arbitrary number of words `num_words` from the advice stack and returns them along with
/// sequantial RPO hash of all read words.
#[cfg(not(all(target_family = "wasm", miden)))]
pub fn pipe_words_to_memory(_num_words: Felt) -> (Word, Vec<Felt>) {
    unimplemented!("miden::core::mem bindings are only available when targeting the Miden VM")
}

/// Returns an even number of words from the advice stack along with the RPO hash of all read words.
///
/// Cycles: 9 + 6 * (num_words / 2)
#[cfg(all(target_family = "wasm", miden))]
pub fn pipe_double_words_to_memory(num_words: Felt) -> (Word, Vec<Felt>) {
    #[repr(C)]
    struct Result {
        r0: Word,
        r1: Word,
        c: Word,
        write_ptr: *mut Felt,
    }

    let num_words_usize = usize::try_from(num_words.as_u64()).expect("num_words must fit in usize");
    let num_felts = num_words_usize.checked_mul(4).expect("num_words too large");

    let mut buf: Vec<Felt> = Vec::with_capacity(num_felts);

    let rust_write_ptr = buf.as_mut_ptr().addr();
    let rust_write_ptr_u32 = u32::try_from(rust_write_ptr).expect("write_ptr must fit in u32");
    assert_eq!(rust_write_ptr_u32 % 4, 0, "write_ptr must be word-aligned");
    let miden_write_ptr = rust_write_ptr_u32 / 4;
    let num_felts_u32 = u32::try_from(num_felts).expect("num_felts must fit in u32");
    let miden_end_ptr = miden_write_ptr + num_felts_u32;

    // Place for returned R0, R1, C, write_ptr
    let mut ret_area = ::core::mem::MaybeUninit::<Result>::uninit();
    let zero = felt!(0);
    unsafe {
        extern_pipe_double_words_to_memory(
            zero,
            zero,
            zero,
            zero, // R0
            zero,
            zero,
            zero,
            zero, // R1
            zero,
            zero,
            zero,
            zero, // C
            miden_write_ptr as usize as *mut Felt,
            miden_end_ptr as usize as *mut Felt,
            ret_area.as_mut_ptr() as *mut Felt,
        );
        buf.set_len(num_felts);
        let Result { r1, .. } = ret_area.assume_init();
        (r1.reverse(), buf)
    }
}

/// Returns an even number of words from the advice stack along with the RPO hash of all read words.
#[cfg(not(all(target_family = "wasm", miden)))]
pub fn pipe_double_words_to_memory(_num_words: Felt) -> (Word, Vec<Felt>) {
    unimplemented!("miden::core::mem bindings are only available when targeting the Miden VM")
}

/// Pops an arbitrary number of words from the advice stack and asserts it matches the commitment.
/// Returns a Vec containing the loaded words.
#[inline]
#[cfg(all(target_family = "wasm", miden))]
pub fn adv_load_preimage(num_words: Felt, commitment: Word) -> Vec<Felt> {
    // Allocate a Vec with the specified capacity
    let num_words_usize = num_words.as_canonical_u64() as usize;
    let num_felts = num_words_usize * 4;
    let mut result: Vec<Felt> = Vec::with_capacity(num_felts);

    let result_miden_ptr = (result.as_mut_ptr() as usize) / 4;
    unsafe {
        // Call pipe_preimage_to_memory to load words from advice stack
        extern_pipe_preimage_to_memory(
            num_words,
            result_miden_ptr as *mut Felt,
            commitment[3],
            commitment[2],
            commitment[1],
            commitment[0],
        );

        // Set the length of the Vec to match what was loaded
        result.set_len(num_felts);
    }

    result
}

/// Pops an arbitrary number of words from the advice stack and asserts it matches the commitment.
/// Returns a Vec containing the loaded words.
#[cfg(not(all(target_family = "wasm", miden)))]
#[inline]
pub fn adv_load_preimage(_num_words: Felt, _commitment: Word) -> Vec<Felt> {
    unimplemented!("miden::core::mem bindings are only available when targeting the Miden VM")
}
