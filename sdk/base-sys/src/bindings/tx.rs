use miden_stdlib_sys::Felt;

use super::types::{Asset, NoteId, NoteType, Recipient, Tag};

#[link(wasm_import_module = "miden:core-import/tx@1.0.0")]
extern "C" {
    #[link_name = "create-note"]
    pub fn extern_tx_create_note(
        asset_f0: Felt,
        asset_f1: Felt,
        asset_f2: Felt,
        asset_f3: Felt,
        tag: Tag,
        note_type: NoteType,
        recipient_f0: Felt,
        recipient_f1: Felt,
        recipient_f2: Felt,
        recipient_f3: Felt,
    ) -> NoteId;
}

/// Creates a new note.  asset is the asset to be included in the note.  tag is
/// the tag to be included in the note.  recipient is the recipient of the note.
/// Returns the id of the created note.
pub fn create_note(asset: Asset, tag: Tag, note_type: NoteType, recipient: Recipient) -> NoteId {
    unsafe {
        extern_tx_create_note(
            asset.inner[0],
            asset.inner[1],
            asset.inner[2],
            asset.inner[3],
            tag,
            note_type,
            recipient.inner[0],
            recipient.inner[1],
            recipient.inner[2],
            recipient.inner[3],
        )
    }
}
