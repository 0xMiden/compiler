extern crate alloc;
use alloc::vec::Vec;

use miden_stdlib_sys::{Felt, Word};

use super::types::{Asset, NoteIdx, NoteType, Recipient, Tag};

#[allow(improper_ctypes)]
extern "C" {
    #[link_name = "miden::output_note::create"]
    pub fn extern_output_note_create(
        tag: Tag,
        aux: Felt,
        note_type: NoteType,
        execution_hint: Felt,
        recipient_f0: Felt,
        recipient_f1: Felt,
        recipient_f2: Felt,
        recipient_f3: Felt,
    ) -> NoteIdx;

    #[link_name = "miden::output_note::add_asset"]
    pub fn extern_output_note_add_asset(
        asset_f0: Felt,
        asset_f1: Felt,
        asset_f2: Felt,
        asset_f3: Felt,
        note_idx: NoteIdx,
    );

    #[link_name = "miden::output_note::get_assets_info"]
    pub fn extern_output_note_get_assets_info(note_index: Felt, ptr: *mut (Word, Felt));

    #[link_name = "miden::output_note::get_assets"]
    pub fn extern_output_note_get_assets(dest_ptr: *mut Felt, note_index: Felt) -> usize;

    #[link_name = "miden::output_note::get_recipient"]
    pub fn extern_output_note_get_recipient(note_index: Felt, ptr: *mut Recipient);

    #[link_name = "miden::output_note::get_metadata"]
    pub fn extern_output_note_get_metadata(note_index: Felt, ptr: *mut Word);
}

/// Creates a new output note and returns its index.
pub fn create(
    tag: Tag,
    aux: Felt,
    note_type: NoteType,
    execution_hint: Felt,
    recipient: Recipient,
) -> NoteIdx {
    unsafe {
        extern_output_note_create(
            tag,
            aux,
            note_type,
            execution_hint,
            recipient.inner[3],
            recipient.inner[2],
            recipient.inner[1],
            recipient.inner[0],
        )
    }
}

/// Adds the asset to the output note specified by `note_idx`.
pub fn add_asset(asset: Asset, note_idx: NoteIdx) {
    unsafe {
        extern_output_note_add_asset(
            asset.inner[3],
            asset.inner[2],
            asset.inner[1],
            asset.inner[0],
            note_idx,
        );
    }
}

/// Contains summary information about the assets of an output note.
pub struct OutputNoteAssetsInfo {
    pub commitment: Word,
    pub num_assets: Felt,
}

/// Retrieves the assets commitment and asset count for the output note at `note_index`.
pub fn get_assets_info(note_index: NoteIdx) -> OutputNoteAssetsInfo {
    unsafe {
        let mut ret_area = ::core::mem::MaybeUninit::<(Word, Felt)>::uninit();
        extern_output_note_get_assets_info(note_index.inner, ret_area.as_mut_ptr());
        let (commitment, num_assets) = ret_area.assume_init();
        OutputNoteAssetsInfo {
            commitment: commitment.reverse(),
            num_assets,
        }
    }
}

/// Returns the assets contained in the output note at `note_index`.
pub fn get_assets(note_index: NoteIdx) -> Vec<Asset> {
    const MAX_ASSETS: usize = 256;
    let mut assets: Vec<Asset> = Vec::with_capacity(MAX_ASSETS);
    let num_assets = unsafe {
        let ptr = (assets.as_mut_ptr() as usize) / 4;
        extern_output_note_get_assets(ptr as *mut Felt, note_index.inner)
    };
    unsafe {
        assets.set_len(num_assets);
    }
    assets
}

/// Returns the recipient of the output note at `note_index`.
pub fn get_recipient(note_index: NoteIdx) -> Recipient {
    unsafe {
        let mut ret_area = ::core::mem::MaybeUninit::<Recipient>::uninit();
        extern_output_note_get_recipient(note_index.inner, ret_area.as_mut_ptr());
        let recipient = ret_area.assume_init();
        Recipient {
            inner: recipient.inner.reverse(),
        }
    }
}

/// Returns the metadata of the output note at `note_index`.
pub fn get_metadata(note_index: NoteIdx) -> Word {
    unsafe {
        let mut ret_area = ::core::mem::MaybeUninit::<Word>::uninit();
        extern_output_note_get_metadata(note_index.inner, ret_area.as_mut_ptr());
        ret_area.assume_init().reverse()
    }
}
