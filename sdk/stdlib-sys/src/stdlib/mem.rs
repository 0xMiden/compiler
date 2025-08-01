#![allow(dead_code)]

extern crate alloc;
use alloc::vec::Vec;

use crate::{
    felt,
    intrinsics::{Felt, Word},
};

#[link(wasm_import_module = "miden:core-stdlib/stdlib-mem@1.0.0")]
extern "C" {

    /// Moves an arbitrary number of words from the advice stack to memory.
    ///
    /// Input: [num_words, write_ptr, ...]
    /// Output: [HASH, write_ptr', ...]
    ///
    /// Where HASH is the sequential RPO hash of all copied words.
    ///
    /// Cycles:
    /// - Even num_words: 48 + 9 * num_words / 2
    /// - Odd num_words: 65 + 9 * round_down(num_words / 2)
    #[link_name = "pipe-words-to-memory"]
    fn extern_pipe_words_to_memory(num_words: Felt, ptr: *mut Felt, out_ptr: *mut Felt);

    /// Moves an even number of words from the advice stack to memory.
    ///
    /// Input: [C, B, A, write_ptr, end_ptr, ...]
    /// Output: [C, B, A, write_ptr, ...]
    ///
    /// Where:
    /// - The words C, B, and A are the RPO hasher state
    /// - A is the capacity
    /// - C, B are the rate portion of the state
    /// - The value num_words = end_ptr - write_ptr must be positive and even
    ///
    /// Cycles: 10 + 9 * num_words / 2
    #[link_name = "pipe-double-words-to-memory"]
    fn extern_pipe_double_words_to_memory(
        c0: Felt,
        c1: Felt,
        c2: Felt,
        c3: Felt,
        b0: Felt,
        b1: Felt,
        b2: Felt,
        b3: Felt,
        a0: Felt,
        a1: Felt,
        a2: Felt,
        a3: Felt,
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
    #[link_name = "pipe-preimage-to-memory"]
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
/// sequantial RPO hash of all read words.
///
/// Cycles:
/// - Even num_words: 48 + 9 * num_words / 2
/// - Odd num_words: 65 + 9 * round_down(num_words / 2)
pub fn pipe_words_to_memory(num_words: Felt) -> (Word, Vec<Felt>) {
    struct Result {
        hash: Word,
        write_ptr: *mut Felt,
    }

    unsafe {
        // Place for returned HASH, write_ptr
        let mut ret_area = ::core::mem::MaybeUninit::<Result>::uninit();
        let mut buf: Vec<Felt> = Vec::with_capacity((num_words.as_u64() * 4) as usize);
        extern_pipe_words_to_memory(
            num_words,
            buf.as_mut_ptr(),
            ret_area.as_mut_ptr() as *mut Felt,
        );
        let Result { hash, .. } = ret_area.assume_init();
        (hash, buf)
    }
}

/// Returns an even number of words from the advice stack along with the RPO hash of all read words.
///
/// Cycles: 10 + 9 * num_words / 2
pub fn pipe_double_words_to_memory(num_words: Felt) -> (Word, Vec<Felt>) {
    struct Result {
        c: Word,
        b: Word,
        a: Word,
        write_ptr: *mut Felt,
    }

    let num_words_in_felts = num_words.as_u64() as usize * 4;
    let mut buf: Vec<Felt> = Vec::with_capacity(num_words_in_felts);
    let write_ptr = buf.as_mut_ptr();
    let end_ptr = unsafe { write_ptr.add(num_words_in_felts) };
    // Place for returned C, B, A, write_ptr
    let mut ret_area = ::core::mem::MaybeUninit::<Result>::uninit();
    let zero = felt!(0);
    unsafe {
        extern_pipe_double_words_to_memory(
            zero,
            zero,
            zero,
            zero,
            zero,
            zero,
            zero,
            zero,
            zero,
            zero,
            zero,
            zero,
            write_ptr,
            end_ptr,
            ret_area.as_mut_ptr() as *mut Felt,
        );
        let Result { b, .. } = ret_area.assume_init();
        // B (second) is the hash (see https://github.com/0xMiden/miden-vm/blob/3a957f7c90176914bda2139f74bff9e5700d59ac/stdlib/asm/crypto/hashes/native.masm#L1-L16 )
        (b, buf)
    }
}

/// Pops an arbitrary number of words from the advice stack and asserts it matches the commitment.
/// Returns a Vec containing the loaded words.
#[inline]
pub fn adv_load_preimage(num_words: Felt, commitment: Word) -> Vec<Felt> {
    // Allocate a Vec with the specified capacity
    let num_words_usize = num_words.as_u64() as usize;
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
