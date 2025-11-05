use miden_stdlib_sys::{Felt, Word};

use super::types::{Asset, NoteIdx, NoteType, Recipient, Tag};

#[allow(improper_ctypes)]
extern "C" {
    #[link_name = "miden::tx::create_note"]
    pub fn extern_tx_create_note(
        tag: Tag,
        aux: Felt,
        note_type: NoteType,
        execution_hint: Felt,
        recipient_f0: Felt,
        recipient_f1: Felt,
        recipient_f2: Felt,
        recipient_f3: Felt,
    ) -> NoteIdx;

    #[link_name = "miden::tx::add_asset_to_note"]
    pub fn extern_tx_add_asset_to_note(
        asset_f0: Felt,
        asset_f1: Felt,
        asset_f2: Felt,
        asset_f3: Felt,
        note_idx: NoteIdx,
        result: *mut (Asset, NoteIdx),
    );

    #[link_name = "miden::tx::get_block_number"]
    pub fn extern_tx_get_block_number() -> Felt;

    #[link_name = "miden::tx::get_input_notes_commitment"]
    pub fn extern_tx_get_input_notes_commitment(ptr: *mut Word);

    #[link_name = "miden::tx::get_output_notes_commitment"]
    pub fn extern_tx_get_output_notes_commitment(ptr: *mut Word);
}

/// Creates a new note.  asset is the asset to be included in the note.  tag is
/// the tag to be included in the note.  recipient is the recipient of the note.
/// Returns the id of the created note.
pub fn create_note(
    tag: Tag,
    aux: Felt,
    note_type: NoteType,
    execution_hint: Felt,
    recipient: Recipient,
) -> NoteIdx {
    unsafe {
        extern_tx_create_note(
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

/// Adds the asset to the note specified by the index.
///
/// # Arguments
/// * `asset` - The asset to be added to the note
/// * `note_idx` - The index of the note to which the asset will be added
///
/// # Returns
/// A tuple containing the same asset and note_idx
pub fn add_asset_to_note(asset: Asset, note_idx: NoteIdx) -> (Asset, NoteIdx) {
    unsafe {
        let mut ret_area = ::core::mem::MaybeUninit::<(Asset, NoteIdx)>::uninit();
        extern_tx_add_asset_to_note(
            asset.inner[3],
            asset.inner[2],
            asset.inner[1],
            asset.inner[0],
            note_idx,
            ret_area.as_mut_ptr(),
        );

        let (asset, note_idx) = ret_area.assume_init();
        (asset.reverse(), note_idx)
    }
}

/// Returns the current block number.
pub fn get_block_number() -> Felt {
    unsafe { extern_tx_get_block_number() }
}

/// Returns the input notes commitment digest.
pub fn get_input_notes_commitment() -> Word {
    unsafe {
        let mut ret_area = ::core::mem::MaybeUninit::<Word>::uninit();
        extern_tx_get_input_notes_commitment(ret_area.as_mut_ptr());
        ret_area.assume_init().reverse()
    }
}

/// Returns the output notes commitment digest.
pub fn get_output_notes_commitment() -> Word {
    unsafe {
        let mut ret_area = ::core::mem::MaybeUninit::<Word>::uninit();
        extern_tx_get_output_notes_commitment(ret_area.as_mut_ptr());
        ret_area.assume_init().reverse()
    }
}
