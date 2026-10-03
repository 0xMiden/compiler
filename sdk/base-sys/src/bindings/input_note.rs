extern crate alloc;
use alloc::vec::Vec;

use miden_stdlib_sys::{ElementPtr, Felt, Word};

use super::{
    MAX_ATTACHMENT_WORDS, MAX_ATTACHMENTS_PER_NOTE, assert_attachment_count,
    assert_attachment_word_count, attachment_index_u8, note_index_and_scheme_u16,
    types::{AccountId, Asset, NoteId, NoteIdx, NoteMetadata, Recipient},
};
use crate::raw::protocol::{input_note as raw, types as raw_types};

/// Contains summary information about the assets stored in an input note.
pub struct InputNoteAssetsInfo {
    pub commitment: Word,
    pub num_assets: u32,
}

/// Contains summary information about the storage stored in an input note.
pub struct InputNoteStorageInfo {
    pub commitment: Word,
    pub num_storage_items: u32,
}

/// Returns the initial assets commitment and asset count for the input note at `note_index`.
///
/// These describe the note's assets at creation time, unaffected by in-transaction removal.
pub fn get_initial_assets_info(note_index: NoteIdx) -> InputNoteAssetsInfo {
    let (commitment, num_assets) = raw::get_initial_assets_info(note_index.to_u16());
    InputNoteAssetsInfo {
        commitment,
        num_assets: num_assets.into(),
    }
}

/// Returns the initial assets contained in the input note at `note_index`.
///
/// These are the note's assets at creation time, unaffected by in-transaction removal.
pub fn get_initial_assets(note_index: NoteIdx) -> Vec<Asset> {
    const MAX_ASSETS: usize = 256;
    let mut assets: Vec<Asset> = Vec::with_capacity(MAX_ASSETS);
    let num_assets = unsafe {
        raw::get_initial_assets(
            ElementPtr::from_ptr(assets.as_mut_ptr().cast::<raw_types::Asset>()),
            note_index.to_u16(),
        )
    };
    unsafe {
        assets.set_len(num_assets.into());
    }
    assets
}

/// Returns the recipient of the input note at `note_index`.
pub fn get_recipient(note_index: NoteIdx) -> Recipient {
    raw::get_recipient(note_index.to_u16()).into()
}

/// Returns the metadata header of the input note at `note_index`.
pub fn get_metadata(note_index: NoteIdx) -> NoteMetadata {
    NoteMetadata::new(raw::get_metadata(note_index.to_u16()))
}

/// Returns the sender of the input note at `note_index`.
pub fn get_sender(note_index: NoteIdx) -> AccountId {
    raw::get_sender(note_index.to_u16()).into()
}

/// Returns the storage commitment and storage item count for the input note at `note_index`.
pub fn get_storage_info(note_index: NoteIdx) -> InputNoteStorageInfo {
    let (commitment, num_storage_items) = raw::get_storage_info(note_index.to_u16());
    InputNoteStorageInfo {
        commitment,
        num_storage_items: num_storage_items.into(),
    }
}

/// Returns the script root of the input note at `note_index`.
pub fn get_script_root(note_index: NoteIdx) -> Word {
    raw::get_script_root(note_index.to_u16())
}

/// Returns the serial number of the input note at `note_index`.
pub fn get_serial_number(note_index: NoteIdx) -> Word {
    raw::get_serial_number(note_index.to_u16())
}

/// Returns the commitment over all attachments of the input note at `note_index`.
pub fn get_attachments_commitment(note_index: NoteIdx) -> Word {
    raw::get_attachments_commitment(note_index.to_u16())
}

/// Returns the attachment commitment of the active note when `is_active_note` is one, or of
/// the indexed input note when it is zero.
///
/// # Panics
///
/// Panics if `is_active_note` is neither zero nor one.
pub fn get_attachments_commitment_raw(is_active_note: Felt, note_index: NoteIdx) -> Word {
    assert!(
        is_active_note == Felt::from_u32(0) || is_active_note == Felt::from_u32(1),
        "is_active_note must be zero or one"
    );
    if is_active_note == Felt::from_u32(1) {
        super::active_note::get_attachments_commitment()
    } else {
        get_attachments_commitment(note_index)
    }
}

/// Returns the attachment commitments of the input note at `note_index`.
///
/// The name mirrors the kernel procedure, which fills the buffer this function returns.
pub fn write_attachment_commitments_to_memory(note_index: NoteIdx) -> Vec<Word> {
    let mut commitments: Vec<Word> = Vec::with_capacity(MAX_ATTACHMENTS_PER_NOTE);
    let num_attachments = unsafe {
        raw::write_attachment_commitments_to_memory(
            ElementPtr::from_ptr(commitments.as_mut_ptr()),
            note_index.to_u16(),
        )
    };
    let num_attachments = num_attachments.into();
    assert_attachment_count(num_attachments);
    unsafe {
        commitments.set_len(num_attachments);
    }
    commitments
}

/// Returns the attachment at `attachment_idx` of the input note at `note_index` as protocol words.
///
/// The name mirrors the kernel procedure, which fills the buffer this function returns.
pub fn write_attachment_to_memory(note_index: NoteIdx, attachment_idx: u32) -> Vec<Word> {
    let mut attachment: Vec<Word> = Vec::with_capacity(MAX_ATTACHMENT_WORDS);
    let num_words = unsafe {
        raw::write_attachment_to_memory(
            ElementPtr::from_ptr(attachment.as_mut_ptr()),
            attachment_index_u8(attachment_idx),
            note_index.to_u16(),
        )
    };
    let num_words = num_words.into();
    assert_attachment_word_count(num_words);
    unsafe {
        attachment.set_len(num_words);
    }
    attachment
}

/// Searches the input note metadata for `attachment_scheme`.
///
/// # Panics
///
/// Panics if `note_index` is out of bounds for the transaction's input notes, or if
/// `attachment_scheme` does not fit in a `u16`: the protocol cannot store an attachment under
/// such a scheme, so asking for one is a caller bug.
pub fn find_attachment(note_index: NoteIdx, attachment_scheme: Felt) -> Option<u32> {
    let (note_index, attachment_scheme) = note_index_and_scheme_u16(note_index, attachment_scheme);
    let (found, index) = raw::find_attachment(attachment_scheme, note_index);
    found.then_some(index.into())
}

/// Returns the number of assets the input note at `note_index` was created with.
///
/// The count is unaffected by in-transaction removal.
#[inline]
pub fn get_initial_num_assets(note_index: NoteIdx) -> u32 {
    raw::get_initial_num_assets(note_index.to_u16()).into()
}

/// Returns the asset at `asset_index` in the input note at `note_index`.
///
/// The asset is returned as it currently is: an asset that was already removed from the note reads
/// back with both words empty.
///
/// # Panics
///
/// Panics if either index is out of bounds.
pub fn get_asset(note_index: NoteIdx, asset_index: u32) -> Asset {
    let asset_index = u8::try_from(asset_index).expect("asset index exceeds u8");
    raw::get_asset(asset_index, note_index.to_u16()).into()
}

/// Removes `asset` from the input note at `note_index` and returns the asset value left in it.
///
/// The returned value is empty when the entire asset was removed.
///
/// # Panics
///
/// Panics if `note_index` is out of bounds, if the call does not originate from the native
/// account's context, if the asset is not present in the note, if a non-composable asset is not
/// present with the exact value, if the note holds less of a fungible asset than is removed, if the
/// asset id is empty or malformed, or if the asset's composition is `Custom`.
pub fn remove_asset(note_index: NoteIdx, asset: Asset) -> Word {
    raw::remove_asset(asset.into(), note_index.to_u16())
}

/// Returns the ID of the input note at `note_index`, as cached by the transaction prologue.
pub fn get_note_id(note_index: NoteIdx) -> NoteId {
    raw::get_note_id(note_index.to_u16()).into()
}

/// Returns the index of the input note with the given ID, or `None` when the transaction does not
/// consume it.
///
/// # Panics
///
/// Panics if the host's answer contradicts the transaction's input notes: it reports an index
/// whose note has a different ID, or reports the note as absent although it is consumed.
pub fn find_note(note_id: NoteId) -> Option<NoteIdx> {
    let (found, index) = raw::find_note(note_id.inner);
    found.then(|| index.into())
}
