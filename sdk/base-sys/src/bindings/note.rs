extern crate alloc;
use alloc::vec::Vec;

use miden_stdlib_sys::Felt;

use super::Asset;

#[link(wasm_import_module = "miden:core-base/note@1.0.0")]
extern "C" {
    #[link_name = "get-inputs"]
    pub fn extern_note_get_inputs(ptr: *mut Felt) -> usize;
    #[link_name = "get-assets"]
    pub fn extern_note_get_assets(ptr: *mut Felt) -> usize;
}

/// Get the inputs of the currently executing note.
pub fn get_inputs() -> Vec<Felt> {
    const MAX_INPUTS: usize = 256;
    let mut inputs: Vec<Felt> = Vec::with_capacity(MAX_INPUTS);
    let num_inputs = unsafe {
        // Ensure the pointer is a valid Miden pointer
        //
        // NOTE: This relies on the fact that BumpAlloc makes all allocations
        // minimally word-aligned. Each word consists of 4 elements of 4 bytes.
        // Since Miden VM is field element-addressable, to get a Miden address from a Rust address,
        // we divide it by 4 to get the address in field elements.
        let ptr = (inputs.as_mut_ptr() as usize) / 4;
        // The MASM for this function is here:
        // https://github.com/0xMiden/miden-base/blob/3cbe8d59dcf4ccc9c380b7c8417ac6178fc6b86a/miden-lib/asm/miden/note.masm#L69-L102
        // #! Writes the inputs of the currently execute note into memory starting at the specified
        // address. #!
        // #! Inputs: [dest_ptr]
        // #! Outputs: [num_inputs, dest_ptr]
        // #!
        // #! - dest_ptr is the memory address to write the inputs.
        // Compiler generated adapter code at call site will drop the returned dest_ptr
        // and return the number of inputs
        extern_note_get_inputs(ptr as *mut Felt)
    };
    unsafe {
        inputs.set_len(num_inputs);
    }
    inputs
}

/// Get the assets of the currently executing note.
pub fn get_assets() -> Vec<Asset> {
    const MAX_INPUTS: usize = 256;
    let mut inputs: Vec<Asset> = Vec::with_capacity(MAX_INPUTS);
    let num_inputs = unsafe {
        let ptr = (inputs.as_mut_ptr() as usize) / 4;
        extern_note_get_assets(ptr as *mut Felt)
    };
    unsafe {
        inputs.set_len(num_inputs);
    }
    inputs
}
