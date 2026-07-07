//! Rust implementation of the Miden protocol batch kernel, compiled to Miden Assembly by the
//! Miden compiler.
//!
//! A transaction batch groups a set of independently-proven transactions so they can later be
//! aggregated into a block by the block kernel. This program validates the per-transaction data
//! supplied via the advice provider, and computes the batch's `INPUT_NOTES_COMMITMENT` and its
//! effective `batch_expiration_block_num`.
//!
//! The implementation mirrors the MASM batch kernel of the protocol repository — the phase
//! structure and checks follow `asm/kernels/batch/{main.masm,lib/*.masm}` of
//! 0xMiden/protocol#2905 (with the expiration running-minimum of 0xMiden/protocol#3019):
//!
//! - [`prologue`]: "unhashes" the layered advice data anchored at the public `BATCH_ID`. Each
//!   layer of advice data is keyed by a hash the previous layer verified, so every element that
//!   makes up `INPUT_NOTES_COMMITMENT` is transitively committed-to:
//!
//!   - `BATCH_ID` (public input)       -> `(tx_id, account_id)` tuple list
//!   - each `tx_id`                    -> per-tx `(INIT, FINAL, INPUT_NOTES_COMMITMENT_i,
//!     OUTPUT_NOTES_COMMITMENT_i)`
//!   - each `INPUT_NOTES_COMMITMENT_i` -> `(NULLIFIER, NOTE_ID_OR_EMPTY)` tuples
//!
//! - [`note_tracker`]: determines intra-batch note erasure and binds the host-provided sorted
//!   note lists to the verified per-transaction notes.
//! - [`epilogue`]: enforces the tracking invariants and computes the output commitments.
//!
//! # Inputs
//!
//! - operand stack: `BLOCK_COMMITMENT` and `BATCH_ID`, one felt per parameter (the MASM kernel's
//!   `[BLOCK_COMMITMENT, BATCH_ID, pad(8)]` public inputs), plus the output pointer.
//! - advice map and stack: see [`prologue::prepare_batch`].
//!
//! # Outputs
//!
//! The MASM kernel's output stack is `[INPUT_NOTES_COMMITMENT, BATCH_NOTE_TREE_ROOT,
//! batch_expiration_block_num, pad(7)]`. Here the two commitment words are written to `out_ptr`
//! and `batch_expiration_block_num` is the return value:
//!
//! - `INPUT_NOTES_COMMITMENT` is the nullifier-sorted sequential hash over the batch's input
//!   notes, excluding notes created and consumed within the batch (i.e. post-erasure).
//! - `BATCH_NOTE_TREE_ROOT` is emitted as the empty word (not yet computed).
//! - `batch_expiration_block_num` is the minimum of every transaction's `expiration_block_num`.
//!
//! TODO: verify BLOCK_COMMITMENT against block header data.
//! TODO: authenticate unauthenticated, non-erased input notes against BLOCK_COMMITMENT's chain
//!       MMR.
//! TODO: emit BATCH_NOTE_TREE_ROOT (the batch note tree SMT root).
//! TODO: aggregate per-account updates and emit a separate ACCOUNT_UPDATES_COMMITMENT output.
//! TODO: recursively verify each transaction's `ExecutionProof`.

#![no_std]
#![no_main]
#![feature(alloc_error_handler)]

extern crate alloc;

#[global_allocator]
static ALLOC: miden_sdk_alloc::BumpAlloc = miden_sdk_alloc::BumpAlloc::new();

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    core::arch::wasm32::unreachable()
}

#[alloc_error_handler]
fn alloc_error(_layout: core::alloc::Layout) -> ! {
    core::arch::wasm32::unreachable()
}

mod epilogue;
mod memory;
mod note_tracker;
mod prologue;

use miden_stdlib_sys::{Felt, Word};

/// Batch kernel program.
///
/// See the crate documentation for the input/output contract.
// The entrypoint is invoked by the VM with its arguments on the operand stack, so `unsafe fn`
// would not communicate anything to its caller; `out_ptr` validity is the executing host's
// responsibility.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[unsafe(no_mangle)]
#[allow(improper_ctypes_definitions)]
pub extern "C" fn entrypoint(
    out_ptr: *mut Felt,
    _block_commitment0: Felt,
    _block_commitment1: Felt,
    _block_commitment2: Felt,
    _block_commitment3: Felt,
    batch_id0: Felt,
    batch_id1: Felt,
    batch_id2: Felt,
    batch_id3: Felt,
) -> Felt {
    // TODO: verify BLOCK_COMMITMENT against block header data via the pipe-and-verify pattern
    // (the MASM kernel drops it the same way).
    let batch_id = Word::from([batch_id0, batch_id1, batch_id2, batch_id3]);

    let mut memory = prologue::prepare_batch(batch_id);

    note_tracker::track_notes(&mut memory);

    let batch_expiration_block_num = memory.batch_expiration_block_num;
    let (input_notes_commitment, batch_note_tree_root) = epilogue::finalize(memory);

    // Lay the output words out at `out_ptr`: [INPUT_NOTES_COMMITMENT, BATCH_NOTE_TREE_ROOT].
    for (offset, felt) in input_notes_commitment
        .as_elements()
        .iter()
        .chain(batch_note_tree_root.as_elements().iter())
        .enumerate()
    {
        unsafe { out_ptr.add(offset).write(*felt) };
    }

    batch_expiration_block_num
}
