use miden_stdlib_sys::Felt;

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
