extern crate alloc;
use alloc::vec::Vec;

use crate::{
    ElementPtr, felt,
    intrinsics::{Felt, Word},
    raw::core::{crypto::hashes::poseidon2::State, mem},
};

/// Reads an arbitrary number of words `num_words` from the advice stack and returns them along with
/// the digest of all read words.
///
/// Cycles:
/// - Even num_words: 43 + 9 * num_words / 2
/// - Odd num_words: 60 + 9 * round_down(num_words / 2)
pub fn pipe_words_to_memory(num_words: Felt) -> (Word, Vec<Felt>) {
    let num_words = u32::try_from(num_words.as_canonical_u64()).expect("num_words must fit in u32");
    let num_felts = (num_words as usize).checked_mul(4).expect("num_words too large");
    let mut buf: Vec<Felt> = Vec::with_capacity(num_felts);
    let write_ptr = ElementPtr::from_ptr(buf.as_mut_ptr() as *mut Word);
    // The returned end pointer is not needed: the length is known.
    let (state, _) = unsafe { mem::pipe_words_to_memory(num_words, write_ptr) };
    unsafe { buf.set_len(num_felts) };
    (state.rate0, buf)
}

/// Returns an even number of words from the advice stack along with the Poseidon2 hash of all read
/// words.
///
/// Cycles: 9 + 6 * (num_words / 2)
pub fn pipe_double_words_to_memory(num_words: Felt) -> (Word, Vec<Felt>) {
    let num_words_usize =
        usize::try_from(num_words.as_canonical_u64()).expect("num_words must fit in usize");
    let num_felts = num_words_usize.checked_mul(4).expect("num_words too large");
    let mut buf: Vec<Felt> = Vec::with_capacity(num_felts);
    let write_ptr = ElementPtr::from_ptr(buf.as_mut_ptr() as *mut Word);
    // One past the last word the VM writes, `num_felts` elements on: the allocation holds
    // `num_felts` felts, so the address does not overflow.
    let end_ptr = ElementPtr::new(write_ptr.addr() + num_felts as u32);
    let zero = felt!(0);
    let zero = Word::new([zero, zero, zero, zero]);
    let state = State {
        rate0: zero,
        rate1: zero,
        capacity: zero,
    };
    // The returned write pointer is not needed: the length is known.
    let (state, _) = unsafe { mem::pipe_double_words_to_memory(state, write_ptr, end_ptr) };
    unsafe { buf.set_len(num_felts) };
    (state.rate0, buf)
}

/// Pops `num_words` words from the advice stack and asserts they match the commitment.
/// Returns a Vec containing the loaded words.
///
/// Traps when `num_words` is `2^30` or more: that many words cannot be represented in the
/// 32-bit wasm address space, and truncating the count would under-size the buffer the VM
/// writes into.
///
/// Callers load advice data on hot paths, so the body must stay inlined: an out-of-line copy
/// costs call overhead per load and blocks length-based simplifications at the call site.
#[inline(always)]
#[cfg(all(target_family = "wasm", miden))]
pub fn adv_load_preimage(num_words: Felt, commitment: Word) -> Vec<Felt> {
    // The length feeds an unsafe external write of `num_words` words, so a value whose felt
    // count does not fit the 32-bit wasm `usize` must not be truncated into an undersized
    // buffer. The bound is checked with a native felt comparison — full u64 casts/checked
    // arithmetic lower expensively on the VM — after which the truncating conversions below are
    // provably lossless.
    // 2^30 words = 2^32 felts, the first count whose felt total overflows a 32-bit usize.
    if num_words >= felt!(1073741824) {
        core::arch::wasm32::unreachable()
    }
    let num_words = num_words.as_canonical_u64() as u32;
    let num_felts = num_words as usize * 4;
    let mut result: Vec<Felt> = Vec::with_capacity(num_felts);
    let write_ptr = ElementPtr::from_ptr(result.as_mut_ptr() as *mut Word);
    unsafe {
        // Load the words from the advice provider; the returned end pointer is not needed.
        mem::pipe_preimage_to_memory(num_words, write_ptr, commitment);
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
