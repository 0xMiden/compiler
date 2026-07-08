extern crate alloc;
use alloc::vec::Vec;

use miden_stdlib_sys::{Felt, Word, WordAligned};

use super::{AccountId, Asset, AttachmentLocation, NoteMetadata, RawAccountId, Recipient};

const MAX_ATTACHMENTS_PER_NOTE: usize = 4;
const MAX_ATTACHMENT_WORDS: usize = 256;

#[allow(improper_ctypes)]
unsafe extern "C" {
    // NOTE: In protocol v0.14, note "inputs" are exposed via `active_note::get_storage`.
    #[cfg_attr(target_family = "wasm", linkage = "extern_weak")]
    #[link_name = "miden::protocol::active_note::get_storage"]
    fn extern_note_get_storage(ptr: *mut Felt) -> usize;
    #[cfg_attr(target_family = "wasm", linkage = "extern_weak")]
    #[link_name = "miden::protocol::active_note::get_assets"]
    fn extern_note_get_assets(ptr: *mut Felt) -> usize;
    #[cfg_attr(target_family = "wasm", linkage = "extern_weak")]
    #[link_name = "miden::protocol::active_note::get_sender"]
    fn extern_note_get_sender(ptr: *mut RawAccountId);
    #[cfg_attr(target_family = "wasm", linkage = "extern_weak")]
    #[link_name = "miden::protocol::active_note::get_recipient"]
    fn extern_note_get_recipient(ptr: *mut Recipient);
    #[cfg_attr(target_family = "wasm", linkage = "extern_weak")]
    #[link_name = "miden::protocol::active_note::get_script_root"]
    fn extern_note_get_script_root(ptr: *mut Word);
    #[cfg_attr(target_family = "wasm", linkage = "extern_weak")]
    #[link_name = "miden::protocol::active_note::get_serial_number"]
    fn extern_note_get_serial_number(ptr: *mut Word);
    #[cfg_attr(target_family = "wasm", linkage = "extern_weak")]
    #[link_name = "miden::protocol::active_note::get_metadata"]
    fn extern_note_get_metadata(ptr: *mut NoteMetadata);
    #[cfg_attr(target_family = "wasm", linkage = "extern_weak")]
    #[link_name = "miden::protocol::active_note::is_public"]
    fn extern_note_is_public() -> Felt;
    #[cfg_attr(target_family = "wasm", linkage = "extern_weak")]
    #[link_name = "miden::protocol::active_note::is_private"]
    fn extern_note_is_private() -> Felt;
    #[cfg_attr(target_family = "wasm", linkage = "extern_weak")]
    #[link_name = "miden::protocol::active_note::get_attachments_commitment"]
    fn extern_note_get_attachments_commitment(ptr: *mut Word);
    #[cfg_attr(target_family = "wasm", linkage = "extern_weak")]
    #[link_name = "miden::protocol::active_note::write_attachment_commitments_to_memory"]
    fn extern_note_write_attachment_commitments_to_memory(dest_ptr: *mut Felt) -> usize;
    #[cfg_attr(target_family = "wasm", linkage = "extern_weak")]
    #[link_name = "miden::protocol::active_note::write_attachment_to_memory"]
    fn extern_note_write_attachment_to_memory(dest_ptr: *mut Felt, attachment_idx: Felt) -> usize;
    #[cfg_attr(target_family = "wasm", linkage = "extern_weak")]
    #[link_name = "miden::protocol::active_note::find_attachment"]
    fn extern_note_find_attachment(attachment_scheme: Felt, ptr: *mut AttachmentLocation);
}

/// Returns the storage of the currently executing note.
///
/// # Examples
///
/// Parse a note storage layout into domain types:
///
/// ```rust,ignore
/// use miden::{active_note, AccountId, Asset};
///
/// let storage = active_note::get_storage();
///
/// // Example layout: first two values store a target `AccountId`.
/// let target = AccountId::from(storage[0], storage[1]);
/// ```
pub fn get_storage() -> Vec<Felt> {
    const MAX_INPUTS: usize = 1024;
    let mut inputs: Vec<Felt> = Vec::with_capacity(MAX_INPUTS);
    let num_inputs = unsafe {
        // Ensure the pointer is a valid Miden pointer
        //
        // NOTE: This relies on the fact that BumpAlloc makes all allocations
        // minimally word-aligned. Each word consists of 4 elements of 4 bytes.
        // Since Miden VM is field element-addressable, to get a Miden address from a Rust address,
        // we divide it by 4 to get the address in field elements.
        let ptr = (inputs.as_mut_ptr() as usize) / 4;
        // The protocol `active_note::get_storage` procedure writes the note's storage into memory
        // starting at `dest_ptr` and returns the number of storage items written.
        extern_note_get_storage(ptr as *mut Felt)
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
        let mut ret_area = WordAligned::new(::core::mem::MaybeUninit::<RawAccountId>::uninit());
        extern_note_get_sender(ret_area.as_mut_ptr());
        ret_area.into_inner().assume_init().into_account_id()
    }
}

/// Returns the recipient of the note that is currently executing.
pub fn get_recipient() -> Recipient {
    unsafe {
        let mut ret_area = WordAligned::new(::core::mem::MaybeUninit::<Recipient>::uninit());
        extern_note_get_recipient(ret_area.as_mut_ptr());
        ret_area.into_inner().assume_init()
    }
}

/// Returns the script root of the currently executing note.
pub fn get_script_root() -> Word {
    unsafe {
        let mut ret_area = WordAligned::new(::core::mem::MaybeUninit::<Word>::uninit());
        extern_note_get_script_root(ret_area.as_mut_ptr());
        ret_area.into_inner().assume_init()
    }
}

/// Returns the serial number of the currently executing note.
pub fn get_serial_number() -> Word {
    unsafe {
        let mut ret_area = WordAligned::new(::core::mem::MaybeUninit::<Word>::uninit());
        extern_note_get_serial_number(ret_area.as_mut_ptr());
        ret_area.into_inner().assume_init()
    }
}

/// Returns the metadata header of the note that is currently executing.
pub fn get_metadata() -> NoteMetadata {
    unsafe {
        let mut ret_area = WordAligned::new(::core::mem::MaybeUninit::<NoteMetadata>::uninit());
        extern_note_get_metadata(ret_area.as_mut_ptr());
        ret_area.into_inner().assume_init()
    }
}

/// Returns whether the note currently executing is public.
#[inline]
pub fn is_public() -> bool {
    unsafe { extern_note_is_public() != Felt::new(0).unwrap() }
}

/// Returns whether the note currently executing is private.
#[inline]
pub fn is_private() -> bool {
    unsafe { extern_note_is_private() != Felt::new(0).unwrap() }
}

/// Returns the commitment over all attachments of the note currently executing.
pub fn get_attachments_commitment() -> Word {
    unsafe {
        let mut ret_area = WordAligned::new(::core::mem::MaybeUninit::<Word>::uninit());
        extern_note_get_attachments_commitment(ret_area.as_mut_ptr());
        ret_area.into_inner().assume_init()
    }
}

/// Writes attachment commitments to memory and returns them as protocol words.
pub fn write_attachment_commitments_to_memory() -> Vec<Word> {
    let mut commitments: Vec<Word> = Vec::with_capacity(MAX_ATTACHMENTS_PER_NOTE);
    let num_attachments = unsafe {
        let ptr = (commitments.as_mut_ptr() as usize) / 4;
        extern_note_write_attachment_commitments_to_memory(ptr as *mut Felt)
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

/// Writes the selected attachment to memory and returns it as protocol words.
pub fn write_attachment_to_memory(attachment_idx: Felt) -> Vec<Word> {
    let mut attachment: Vec<Word> = Vec::with_capacity(MAX_ATTACHMENT_WORDS);
    let num_words = unsafe {
        let ptr = (attachment.as_mut_ptr() as usize) / 4;
        extern_note_write_attachment_to_memory(ptr as *mut Felt, attachment_idx)
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

/// Searches the active note metadata for `attachment_scheme`.
pub fn find_attachment(attachment_scheme: Felt) -> AttachmentLocation {
    unsafe {
        let mut ret_area =
            WordAligned::new(::core::mem::MaybeUninit::<AttachmentLocation>::uninit());
        extern_note_find_attachment(attachment_scheme, ret_area.as_mut_ptr());
        ret_area.into_inner().assume_init()
    }
}
