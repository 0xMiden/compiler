extern crate alloc;
use alloc::vec::Vec;

use miden_stdlib_sys::{Felt, Word, WordAligned};

use super::types::{
    AccountId, Asset, AttachmentLocation, NoteIdx, NoteMetadata, RawAccountId, Recipient,
};

const MAX_ATTACHMENTS_PER_NOTE: usize = 4;
const MAX_ATTACHMENT_WORDS: usize = 256;

#[allow(improper_ctypes)]
unsafe extern "C" {
    #[cfg_attr(target_family = "wasm", linkage = "extern_weak")]
    #[link_name = "miden::protocol::input_note::get_assets_info"]
    fn extern_input_note_get_assets_info(note_index: Felt, ptr: *mut (Word, Felt));
    #[cfg_attr(target_family = "wasm", linkage = "extern_weak")]
    #[link_name = "miden::protocol::input_note::get_assets"]
    fn extern_input_note_get_assets(dest_ptr: *mut Felt, note_index: Felt) -> usize;
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
    fn extern_input_note_get_storage_info(note_index: Felt, ptr: *mut (Word, Felt));
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
    #[link_name = "miden::protocol::input_note::get_attachments_commitment_raw"]
    fn extern_input_note_get_attachments_commitment_raw(
        is_active_note: Felt,
        note_index: Felt,
        ptr: *mut Word,
    );
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
        ptr: *mut AttachmentLocation,
    );
}

/// Contains summary information about the assets stored in an input note.
pub struct InputNoteAssetsInfo {
    pub commitment: Word,
    pub num_assets: Felt,
}

/// Contains summary information about the storage stored in an input note.
pub struct InputNoteStorageInfo {
    pub commitment: Word,
    pub num_storage_items: Felt,
}

/// Returns the assets commitment and asset count for the input note at `note_index`.
pub fn get_assets_info(note_index: NoteIdx) -> InputNoteAssetsInfo {
    unsafe {
        let mut ret_area = WordAligned::new(::core::mem::MaybeUninit::<(Word, Felt)>::uninit());
        extern_input_note_get_assets_info(note_index.inner, ret_area.as_mut_ptr());
        let (commitment, num_assets) = ret_area.into_inner().assume_init();
        InputNoteAssetsInfo {
            commitment,
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
        let mut ret_area = WordAligned::new(::core::mem::MaybeUninit::<(Word, Felt)>::uninit());
        extern_input_note_get_storage_info(note_index.inner, ret_area.as_mut_ptr());
        let (commitment, num_storage_items) = ret_area.into_inner().assume_init();
        InputNoteStorageInfo {
            commitment,
            num_storage_items,
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

/// Returns the attachment commitment using the protocol's shared active/indexed input-note path.
pub fn get_attachments_commitment_raw(is_active_note: Felt, note_index: NoteIdx) -> Word {
    unsafe {
        let mut ret_area = WordAligned::new(::core::mem::MaybeUninit::<Word>::uninit());
        extern_input_note_get_attachments_commitment_raw(
            is_active_note,
            note_index.inner,
            ret_area.as_mut_ptr(),
        );
        ret_area.into_inner().assume_init()
    }
}

/// Writes attachment commitments to memory and returns them as protocol words.
pub fn write_attachment_commitments_to_memory(note_index: NoteIdx) -> Vec<Word> {
    let mut commitments: Vec<Word> = Vec::with_capacity(MAX_ATTACHMENTS_PER_NOTE);
    let num_attachments = unsafe {
        let ptr = (commitments.as_mut_ptr() as usize) / 4;
        extern_input_note_write_attachment_commitments_to_memory(ptr as *mut Felt, note_index.inner)
    };
    assert!(
        num_attachments <= MAX_ATTACHMENTS_PER_NOTE,
        "note cannot contain more than {MAX_ATTACHMENTS_PER_NOTE} attachments"
    );
    unsafe {
        commitments.set_len(num_attachments);
    }
    commitments
}

/// Writes the selected input-note attachment to memory and returns it as protocol words.
pub fn write_attachment_to_memory(note_index: NoteIdx, attachment_idx: Felt) -> Vec<Word> {
    let mut attachment: Vec<Word> = Vec::with_capacity(MAX_ATTACHMENT_WORDS);
    let num_words = unsafe {
        let ptr = (attachment.as_mut_ptr() as usize) / 4;
        extern_input_note_write_attachment_to_memory(
            ptr as *mut Felt,
            attachment_idx,
            note_index.inner,
        )
    };
    assert!(
        num_words <= MAX_ATTACHMENT_WORDS,
        "note attachment cannot contain more than {MAX_ATTACHMENT_WORDS} words"
    );
    unsafe {
        attachment.set_len(num_words);
    }
    attachment
}

/// Searches the input note metadata for `attachment_scheme`.
pub fn find_attachment(note_index: NoteIdx, attachment_scheme: Felt) -> AttachmentLocation {
    unsafe {
        let mut ret_area =
            WordAligned::new(::core::mem::MaybeUninit::<AttachmentLocation>::uninit());
        extern_input_note_find_attachment(
            attachment_scheme,
            note_index.inner,
            ret_area.as_mut_ptr(),
        );
        ret_area.into_inner().assume_init()
    }
}
