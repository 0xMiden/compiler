extern crate alloc;
use alloc::vec::Vec;

use miden_stdlib_sys::{Felt, Word};

use super::{AccountId, Asset, Recipient};

#[allow(improper_ctypes)]
unsafe extern "C" {
    #[link_name = "miden::active_note::get_inputs"]
    pub fn extern_note_get_inputs(ptr: *mut Felt) -> usize;
    #[link_name = "miden::active_note::get_assets"]
    pub fn extern_note_get_assets(ptr: *mut Felt) -> usize;
    #[link_name = "miden::active_note::get_sender"]
    pub fn extern_note_get_sender(ptr: *mut AccountId);
    #[link_name = "miden::active_note::get_recipient"]
    pub fn extern_note_get_recipient(ptr: *mut Recipient);
    #[link_name = "miden::active_note::get_script_root"]
    pub fn extern_note_get_script_root(ptr: *mut Word);
    #[link_name = "miden::active_note::get_serial_number"]
    pub fn extern_note_get_serial_number(ptr: *mut Word);
    #[link_name = "miden::active_note::get_metadata"]
    pub fn extern_note_get_metadata(ptr: *mut Word);
    #[link_name = "miden::active_note::add_assets_to_account"]
    pub fn extern_note_add_assets_to_account();
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

/// Returns the sender [`AccountId`] of the note that is currently executing.
pub fn get_sender() -> AccountId {
    unsafe {
        let mut ret_area = ::core::mem::MaybeUninit::<AccountId>::uninit();
        extern_note_get_sender(ret_area.as_mut_ptr());
        ret_area.assume_init()
    }
}

/// Returns the recipient of the note that is currently executing.
pub fn get_recipient() -> Recipient {
    unsafe {
        let mut ret_area = ::core::mem::MaybeUninit::<Recipient>::uninit();
        extern_note_get_recipient(ret_area.as_mut_ptr());
        let mut recipient = ret_area.assume_init();
        recipient.inner = recipient.inner.reverse();
        recipient
    }
}

/// Returns the script root of the currently executing note.
pub fn get_script_root() -> Word {
    unsafe {
        let mut ret_area = ::core::mem::MaybeUninit::<Word>::uninit();
        extern_note_get_script_root(ret_area.as_mut_ptr());
        ret_area.assume_init().reverse()
    }
}

/// Returns the serial number of the currently executing note.
pub fn get_serial_number() -> Word {
    unsafe {
        let mut ret_area = ::core::mem::MaybeUninit::<Word>::uninit();
        extern_note_get_serial_number(ret_area.as_mut_ptr());
        ret_area.assume_init().reverse()
    }
}

/// Returns the metadata of the note that is currently executing.
pub fn get_metadata() -> Word {
    unsafe {
        let mut ret_area = ::core::mem::MaybeUninit::<Word>::uninit();
        extern_note_get_metadata(ret_area.as_mut_ptr());
        ret_area.assume_init().reverse()
    }
}

/// Moves all assets from the active note into the active account vault.
#[inline]
pub fn add_assets_to_account() {
    unsafe { extern_note_add_assets_to_account() }
}
