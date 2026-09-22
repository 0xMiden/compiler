extern crate alloc;
use alloc::vec::Vec;

use miden_stdlib_sys::{Felt, Word, WordAligned};

use super::{
    MAX_ATTACHMENT_WORDS, MAX_ATTACHMENTS_PER_NOTE, assert_attachment_count,
    assert_attachment_word_count,
    types::{
        AccountId, Asset, NoteId, NoteIdx, NoteMetadata, RawAccountId, RawCommitmentWithCount,
        RawFoundIndex, Recipient,
    },
};

#[allow(improper_ctypes)]
unsafe extern "C" {
    #[cfg_attr(target_family = "wasm", linkage = "extern_weak")]
    #[link_name = "miden::protocol::input_note::get_initial_assets_info"]
    fn extern_input_note_get_initial_assets_info(
        note_index: Felt,
        ptr: *mut RawCommitmentWithCount,
    );
    #[cfg_attr(target_family = "wasm", linkage = "extern_weak")]
    #[link_name = "miden::protocol::input_note::get_initial_assets"]
    fn extern_input_note_get_initial_assets(dest_ptr: *mut Felt, note_index: Felt) -> usize;
    #[cfg_attr(target_family = "wasm", linkage = "extern_weak")]
    #[link_name = "miden::protocol::input_note::get_recipient"]
    fn extern_input_note_get_recipient(note_index: Felt, ptr: *mut Recipient);
    #[cfg_attr(target_family = "wasm", linkage = "extern_weak")]
    #[link_name = "miden::protocol::input_note::get_metadata"]
    fn extern_input_note_get_metadata(note_index: Felt, ptr: *mut NoteMetadata);
    #[cfg_attr(target_family = "wasm", linkage = "extern_weak")]
    #[link_name = "miden::protocol::input_note::get_sender"]
    fn extern_input_note_get_sender(note_index: Felt, ptr: *mut RawAccountId);
    #[cfg_attr(target_family = "wasm", linkage = "extern_weak")]
    #[link_name = "miden::protocol::input_note::get_storage_info"]
    fn extern_input_note_get_storage_info(note_index: Felt, ptr: *mut RawCommitmentWithCount);
    #[cfg_attr(target_family = "wasm", linkage = "extern_weak")]
    #[link_name = "miden::protocol::input_note::get_script_root"]
    fn extern_input_note_get_script_root(note_index: Felt, ptr: *mut Word);
    #[cfg_attr(target_family = "wasm", linkage = "extern_weak")]
    #[link_name = "miden::protocol::input_note::get_serial_number"]
    fn extern_input_note_get_serial_number(note_index: Felt, ptr: *mut Word);
    #[cfg_attr(target_family = "wasm", linkage = "extern_weak")]
    #[link_name = "miden::protocol::input_note::get_attachments_commitment"]
    fn extern_input_note_get_attachments_commitment(note_index: Felt, ptr: *mut Word);
    #[cfg_attr(target_family = "wasm", linkage = "extern_weak")]
    #[link_name = "miden::protocol::input_note::write_attachment_commitments_to_memory"]
    fn extern_input_note_write_attachment_commitments_to_memory(
        dest_ptr: *mut Felt,
        note_index: Felt,
    ) -> usize;
    #[cfg_attr(target_family = "wasm", linkage = "extern_weak")]
    #[link_name = "miden::protocol::input_note::write_attachment_to_memory"]
    fn extern_input_note_write_attachment_to_memory(
        dest_ptr: *mut Felt,
        attachment_idx: Felt,
        note_index: Felt,
    ) -> usize;
    #[cfg_attr(target_family = "wasm", linkage = "extern_weak")]
    #[link_name = "miden::protocol::input_note::find_attachment"]
    fn extern_input_note_find_attachment(
        attachment_scheme: Felt,
        note_index: Felt,
        ptr: *mut RawFoundIndex,
    );
    #[cfg_attr(target_family = "wasm", linkage = "extern_weak")]
    #[link_name = "miden::protocol::input_note::get_initial_num_assets"]
    fn extern_input_note_get_initial_num_assets(note_index: Felt) -> Felt;
    #[cfg_attr(target_family = "wasm", linkage = "extern_weak")]
    #[link_name = "miden::protocol::input_note::get_asset"]
    fn extern_input_note_get_asset(asset_index: Felt, note_index: Felt, ptr: *mut Asset);
    #[cfg_attr(target_family = "wasm", linkage = "extern_weak")]
    #[link_name = "miden::protocol::input_note::remove_asset"]
    fn extern_input_note_remove_asset(
        asset_id_0: Felt,
        asset_id_1: Felt,
        asset_id_2: Felt,
        asset_id_3: Felt,
        asset_value_0: Felt,
        asset_value_1: Felt,
        asset_value_2: Felt,
        asset_value_3: Felt,
        note_index: Felt,
        ptr: *mut Word,
    );
    #[cfg_attr(target_family = "wasm", linkage = "extern_weak")]
    #[link_name = "miden::protocol::input_note::get_note_id"]
    fn extern_input_note_get_note_id(note_index: Felt, ptr: *mut NoteId);
    #[cfg_attr(target_family = "wasm", linkage = "extern_weak")]
    #[link_name = "miden::protocol::input_note::find_note"]
    fn extern_input_note_find_note(
        note_id_0: Felt,
        note_id_1: Felt,
        note_id_2: Felt,
        note_id_3: Felt,
        ptr: *mut RawFoundIndex,
    );
}

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
    unsafe {
        let mut ret_area =
            WordAligned::new(::core::mem::MaybeUninit::<RawCommitmentWithCount>::uninit());
        extern_input_note_get_initial_assets_info(note_index.inner, ret_area.as_mut_ptr());
        let raw = ret_area.into_inner().assume_init();
        InputNoteAssetsInfo {
            commitment: raw.commitment,
            num_assets: raw.num_items(),
        }
    }
}

/// Returns the initial assets contained in the input note at `note_index`.
///
/// These are the note's assets at creation time, unaffected by in-transaction removal.
pub fn get_initial_assets(note_index: NoteIdx) -> Vec<Asset> {
    const MAX_ASSETS: usize = 256;
    let mut assets: Vec<Asset> = Vec::with_capacity(MAX_ASSETS);
    let num_assets = unsafe {
        let ptr = (assets.as_mut_ptr() as usize) / 4;
        extern_input_note_get_initial_assets(ptr as *mut Felt, note_index.inner)
    };
    unsafe {
        assets.set_len(num_assets);
    }
    assets
}

/// Returns the recipient of the input note at `note_index`.
pub fn get_recipient(note_index: NoteIdx) -> Recipient {
    unsafe {
        let mut ret_area = WordAligned::new(::core::mem::MaybeUninit::<Recipient>::uninit());
        extern_input_note_get_recipient(note_index.inner, ret_area.as_mut_ptr());
        ret_area.into_inner().assume_init()
    }
}

/// Returns the metadata header of the input note at `note_index`.
pub fn get_metadata(note_index: NoteIdx) -> NoteMetadata {
    unsafe {
        let mut ret_area = WordAligned::new(::core::mem::MaybeUninit::<NoteMetadata>::uninit());
        extern_input_note_get_metadata(note_index.inner, ret_area.as_mut_ptr());
        ret_area.into_inner().assume_init()
    }
}

/// Returns the sender of the input note at `note_index`.
pub fn get_sender(note_index: NoteIdx) -> AccountId {
    unsafe {
        let mut ret_area = WordAligned::new(::core::mem::MaybeUninit::<RawAccountId>::uninit());
        extern_input_note_get_sender(note_index.inner, ret_area.as_mut_ptr());
        ret_area.into_inner().assume_init().into_account_id()
    }
}

/// Returns the storage commitment and storage item count for the input note at `note_index`.
pub fn get_storage_info(note_index: NoteIdx) -> InputNoteStorageInfo {
    unsafe {
        let mut ret_area =
            WordAligned::new(::core::mem::MaybeUninit::<RawCommitmentWithCount>::uninit());
        extern_input_note_get_storage_info(note_index.inner, ret_area.as_mut_ptr());
        let raw = ret_area.into_inner().assume_init();
        InputNoteStorageInfo {
            commitment: raw.commitment,
            num_storage_items: raw.num_items(),
        }
    }
}

/// Returns the script root of the input note at `note_index`.
pub fn get_script_root(note_index: NoteIdx) -> Word {
    unsafe {
        let mut ret_area = WordAligned::new(::core::mem::MaybeUninit::<Word>::uninit());
        extern_input_note_get_script_root(note_index.inner, ret_area.as_mut_ptr());
        ret_area.into_inner().assume_init()
    }
}

/// Returns the serial number of the input note at `note_index`.
pub fn get_serial_number(note_index: NoteIdx) -> Word {
    unsafe {
        let mut ret_area = WordAligned::new(::core::mem::MaybeUninit::<Word>::uninit());
        extern_input_note_get_serial_number(note_index.inner, ret_area.as_mut_ptr());
        ret_area.into_inner().assume_init()
    }
}

/// Returns the commitment over all attachments of the input note at `note_index`.
pub fn get_attachments_commitment(note_index: NoteIdx) -> Word {
    unsafe {
        let mut ret_area = WordAligned::new(::core::mem::MaybeUninit::<Word>::uninit());
        extern_input_note_get_attachments_commitment(note_index.inner, ret_area.as_mut_ptr());
        ret_area.into_inner().assume_init()
    }
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
        let ptr = (commitments.as_mut_ptr() as usize) / 4;
        extern_input_note_write_attachment_commitments_to_memory(ptr as *mut Felt, note_index.inner)
    };
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
        let ptr = (attachment.as_mut_ptr() as usize) / 4;
        extern_input_note_write_attachment_to_memory(
            ptr as *mut Felt,
            Felt::from_u32(attachment_idx),
            note_index.inner,
        )
    };
    assert_attachment_word_count(num_words);
    unsafe {
        attachment.set_len(num_words);
    }
    attachment
}

/// Searches the input note metadata for `attachment_scheme`.
pub fn find_attachment(note_index: NoteIdx, attachment_scheme: Felt) -> Option<u32> {
    unsafe {
        let mut ret_area = WordAligned::new(::core::mem::MaybeUninit::<RawFoundIndex>::uninit());
        extern_input_note_find_attachment(
            attachment_scheme,
            note_index.inner,
            ret_area.as_mut_ptr(),
        );
        ret_area.into_inner().assume_init().into_attachment_index()
    }
}

/// Returns the number of assets the input note at `note_index` was created with.
///
/// The count is unaffected by in-transaction removal.
#[inline]
pub fn get_initial_num_assets(note_index: NoteIdx) -> u32 {
    // The transaction kernel guarantees asset counts fit in a u32.
    let count = unsafe { extern_input_note_get_initial_num_assets(note_index.inner) };
    count.as_canonical_u64() as u32
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
    unsafe {
        let mut ret_area = WordAligned::new(::core::mem::MaybeUninit::<Asset>::uninit());
        extern_input_note_get_asset(
            Felt::from_u32(asset_index),
            note_index.inner,
            ret_area.as_mut_ptr(),
        );
        ret_area.into_inner().assume_init()
    }
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
    unsafe {
        let mut ret_area = WordAligned::new(::core::mem::MaybeUninit::<Word>::uninit());
        extern_input_note_remove_asset(
            asset.key[0],
            asset.key[1],
            asset.key[2],
            asset.key[3],
            asset.value[0],
            asset.value[1],
            asset.value[2],
            asset.value[3],
            note_index.inner,
            ret_area.as_mut_ptr(),
        );
        ret_area.into_inner().assume_init()
    }
}

/// Returns the ID of the input note at `note_index`, as cached by the transaction prologue.
pub fn get_note_id(note_index: NoteIdx) -> NoteId {
    unsafe {
        let mut ret_area = WordAligned::new(::core::mem::MaybeUninit::<NoteId>::uninit());
        extern_input_note_get_note_id(note_index.inner, ret_area.as_mut_ptr());
        ret_area.into_inner().assume_init()
    }
}

/// Returns the index of the input note with the given ID, or `None` when the transaction does not
/// consume it.
///
/// # Panics
///
/// Panics if the host's answer contradicts the transaction's input notes: it reports an index
/// whose note has a different ID, or reports the note as absent although it is consumed.
pub fn find_note(note_id: NoteId) -> Option<NoteIdx> {
    unsafe {
        let mut ret_area = WordAligned::new(::core::mem::MaybeUninit::<RawFoundIndex>::uninit());
        let id = note_id.inner;
        extern_input_note_find_note(id[0], id[1], id[2], id[3], ret_area.as_mut_ptr());
        ret_area.into_inner().assume_init().into_note_index()
    }
}
