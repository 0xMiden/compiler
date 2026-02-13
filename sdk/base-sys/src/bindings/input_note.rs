extern crate alloc;
use alloc::vec::Vec;

use miden_stdlib_sys::{Felt, Word};

use super::types::{AccountId, Asset, NoteIdx, NoteMetadata, Recipient};

#[allow(improper_ctypes)]
unsafe extern "C" {
    #[link_name = "miden::protocol::input_note::get_assets_info"]
    pub fn extern_input_note_get_assets_info(note_index: Felt, ptr: *mut (Word, Felt));

    #[link_name = "miden::protocol::input_note::get_assets"]
    pub fn extern_input_note_get_assets(dest_ptr: *mut Felt, note_index: Felt) -> usize;

    #[link_name = "miden::protocol::input_note::get_recipient"]
    pub fn extern_input_note_get_recipient(note_index: Felt, ptr: *mut Recipient);

    #[link_name = "miden::protocol::input_note::get_metadata"]
    pub fn extern_input_note_get_metadata(note_index: Felt, ptr: *mut NoteMetadata);

    #[link_name = "miden::protocol::input_note::get_sender"]
    pub fn extern_input_note_get_sender(note_index: Felt, ptr: *mut AccountId);

    #[link_name = "miden::protocol::input_note::get_inputs_info"]
    pub fn extern_input_note_get_inputs_info(note_index: Felt, ptr: *mut (Word, Felt));

    #[link_name = "miden::protocol::input_note::get_script_root"]
    pub fn extern_input_note_get_script_root(note_index: Felt, ptr: *mut Word);

    #[link_name = "miden::protocol::input_note::get_serial_number"]
    pub fn extern_input_note_get_serial_number(note_index: Felt, ptr: *mut Word);
}

/// Contains summary information about the assets stored in an input note.
pub struct InputNoteAssetsInfo {
    pub commitment: Word,
    pub num_assets: Felt,
}

/// Contains summary information about the inputs stored in an input note.
pub struct InputNoteInputsInfo {
    pub commitment: Word,
    pub num_inputs: Felt,
}

/// Returns the assets commitment and asset count for the input note at `note_index`.
pub fn get_assets_info(note_index: NoteIdx) -> InputNoteAssetsInfo {
    unsafe {
        let mut ret_area = ::core::mem::MaybeUninit::<(Word, Felt)>::uninit();
        extern_input_note_get_assets_info(note_index.inner, ret_area.as_mut_ptr());
        let (commitment, num_assets) = ret_area.assume_init();
        InputNoteAssetsInfo {
            commitment: commitment.reversed(),
            num_assets,
        }
    }
}

/// Returns the assets contained in the input note at `note_index`.
pub fn get_assets(note_index: NoteIdx) -> Vec<Asset> {
    const MAX_ASSETS: usize = 256;
    let mut assets: Vec<Asset> = Vec::with_capacity(MAX_ASSETS);
    let num_assets = unsafe {
        let ptr = (assets.as_mut_ptr() as usize) / 4;
        extern_input_note_get_assets(ptr as *mut Felt, note_index.inner)
    };
    unsafe {
        assets.set_len(num_assets);
    }
    assets
}

/// Returns the recipient of the input note at `note_index`.
pub fn get_recipient(note_index: NoteIdx) -> Recipient {
    unsafe {
        let mut ret_area = ::core::mem::MaybeUninit::<Recipient>::uninit();
        extern_input_note_get_recipient(note_index.inner, ret_area.as_mut_ptr());
        let mut recipient = ret_area.assume_init();
        recipient.inner = recipient.inner.reversed();
        recipient
    }
}

/// Returns the attachment and metadata header of the input note at `note_index`.
pub fn get_metadata(note_index: NoteIdx) -> NoteMetadata {
    unsafe {
        let mut ret_area = ::core::mem::MaybeUninit::<NoteMetadata>::uninit();
        extern_input_note_get_metadata(note_index.inner, ret_area.as_mut_ptr());
        ret_area.assume_init().reversed()
    }
}

/// Returns the sender of the input note at `note_index`.
pub fn get_sender(note_index: NoteIdx) -> AccountId {
    unsafe {
        let mut ret_area = ::core::mem::MaybeUninit::<AccountId>::uninit();
        extern_input_note_get_sender(note_index.inner, ret_area.as_mut_ptr());
        ret_area.assume_init()
    }
}

/// Returns the inputs commitment and input count for the input note at `note_index`.
pub fn get_inputs_info(note_index: NoteIdx) -> InputNoteInputsInfo {
    unsafe {
        let mut ret_area = ::core::mem::MaybeUninit::<(Word, Felt)>::uninit();
        extern_input_note_get_inputs_info(note_index.inner, ret_area.as_mut_ptr());
        let (commitment, num_inputs) = ret_area.assume_init();
        InputNoteInputsInfo {
            commitment: commitment.reversed(),
            num_inputs,
        }
    }
}

/// Returns the script root of the input note at `note_index`.
pub fn get_script_root(note_index: NoteIdx) -> Word {
    unsafe {
        let mut ret_area = ::core::mem::MaybeUninit::<Word>::uninit();
        extern_input_note_get_script_root(note_index.inner, ret_area.as_mut_ptr());
        ret_area.assume_init().reversed()
    }
}

/// Returns the serial number of the input note at `note_index`.
pub fn get_serial_number(note_index: NoteIdx) -> Word {
    unsafe {
        let mut ret_area = ::core::mem::MaybeUninit::<Word>::uninit();
        extern_input_note_get_serial_number(note_index.inner, ret_area.as_mut_ptr());
        ret_area.assume_init().reversed()
    }
}
