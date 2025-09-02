use miden_stdlib_sys::Felt;

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
