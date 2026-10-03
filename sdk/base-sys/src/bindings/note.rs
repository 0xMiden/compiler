extern crate alloc;

use alloc::vec::Vec;

use miden_stdlib_sys::{ElementPtr, Felt, Word};

use super::{
    AccountId, MAX_ATTACHMENT_WORDS, MAX_ATTACHMENTS_PER_NOTE, NoteType, Recipient, Tag,
    assert_attachment_count, attachment_scheme_u16,
};
use crate::raw::protocol::note as raw;

const MAX_NOTE_STORAGE_ITEMS: usize = 1024;

/// Returns the MAST root digest of the note script defined by the current crate.
///
/// Macro plumbing behind the `get_entrypoint_root()` associated method that `#[note]` generates
/// on the note input type — call that method instead of this function. It lives here because
/// the underlying weak extern requires `feature(linkage)`, which user crates do not enable.
///
/// This is a compiler intrinsic: the call compiles to a MASM `procref` of the crate's
/// `#[note_script]` entrypoint export, so the digest is the note script root observed by the
/// transaction kernel when the note is executed. The digest is computed at assembly time.
///
/// Compilation fails if the current project does not define a `#[note_script]` entrypoint.
///
/// Must not be called from code reachable from the `#[note_script]` entrypoint itself: the note
/// script's MAST root would then depend on its own digest, and assembly fails with a call-graph
/// cycle error. Inside a running note script, use [`active_note::get_script_root`] instead.
///
/// [`active_note::get_script_root`]: crate::bindings::active_note::get_script_root
#[doc(hidden)]
pub fn __entrypoint_root() -> Word {
    #[cfg(all(target_family = "wasm", miden))]
    {
        use core::mem::MaybeUninit;

        use miden_stdlib_sys::WordAligned;

        unsafe extern "C" {
            // The name must stay in lockstep with the stub's `export_name`
            // (`stubs/intrinsics.rs`) and `SCRIPT_ROOT_STUB_NAME` in the compiler frontend
            // (`frontend/wasm/src/intrinsics/note.rs`).
            #[linkage = "extern_weak"]
            #[link_name = "intrinsics::note::script_root"]
            fn extern_note_script_root(ptr: *mut Word);
        }
        unsafe {
            let mut ret_area = WordAligned::new(MaybeUninit::<Word>::uninit());
            extern_note_script_root(ret_area.as_mut_ptr());
            ret_area.into_inner().assume_init()
        }
    }
    #[cfg(not(all(target_family = "wasm", miden)))]
    {
        unimplemented!(
            "`intrinsics::note::script_root` is only available when compiled for the Miden VM"
        )
    }
}

/// Returns the element address of `storage` for the protocol's note storage procedures, which
/// read it a word at a time: `0` for empty storage, else the word-aligned address of its first
/// element.
fn storage_ptr(storage: &[Felt]) -> ElementPtr<Felt> {
    if storage.is_empty() {
        return ElementPtr::new(0);
    }
    let ptr = ElementPtr::from_ptr(storage.as_ptr().cast_mut());
    // Vec storage comes from the SDK allocator, which only produces word-aligned pointers.
    assert_eq!(ptr.addr() % 4, 0, "storage pointer must be word-aligned");
    ptr
}

/// Computes and stores a note recipient from serial number, script root, and storage elements.
///
/// This maps to `miden::protocol::note::compute_and_store_recipient`, which also inserts the
/// provided storage into the advice map under the storage commitment used by the returned
/// recipient digest.
///
/// Panics if `storage` contains more than 1024 elements.
pub fn compute_and_store_recipient(
    serial_num: Word,
    script_root: Word,
    storage: Vec<Felt>,
) -> Recipient {
    assert!(
        storage.len() <= MAX_NOTE_STORAGE_ITEMS,
        "note storage cannot contain more than {MAX_NOTE_STORAGE_ITEMS} items"
    );
    // The bound above makes the length fit in the protocol's `u16`.
    let num_storage_items = storage.len() as u16;
    let recipient = unsafe {
        raw::compute_and_store_recipient(
            storage_ptr(&storage),
            num_storage_items,
            serial_num,
            script_root,
        )
    };
    recipient.into()
}

/// Builds a note recipient from the provided serial number, script root, and storage elements.
///
/// This is retained as an SDK-friendly alias for [`compute_and_store_recipient`].
pub fn build_recipient(serial_num: Word, script_root: Word, storage: Vec<Felt>) -> Recipient {
    compute_and_store_recipient(serial_num, script_root, storage)
}

/// Computes the commitment to the provided note storage elements.
///
/// Panics if `storage` contains more than 1024 elements.
pub fn compute_storage_commitment(storage: &[Felt]) -> Word {
    assert!(
        storage.len() <= MAX_NOTE_STORAGE_ITEMS,
        "note storage cannot contain more than {MAX_NOTE_STORAGE_ITEMS} items"
    );
    // The bound above makes the length fit in the protocol's `u16`.
    let num_storage_items = storage.len() as u16;
    unsafe { raw::compute_storage_commitment(storage_ptr(storage), num_storage_items) }
}

/// Loads the attachment commitments committed to by `attachments_commitment` from the advice map.
///
/// The advice map must contain the preimage committed to by `attachments_commitment`.
///
/// # Panics
///
/// Panics if the preimage is not a whole number of words or holds more commitments than a note
/// can have attachments.
pub fn load_attachment_commitments(attachments_commitment: Word) -> Vec<Word> {
    load_attachment_words(attachments_commitment, MAX_ATTACHMENTS_PER_NOTE)
}

/// Loads the attachment committed to by `attachment_commitment` from the advice map.
///
/// The advice map must contain the attachment elements committed to by `attachment_commitment`.
///
/// # Panics
///
/// Panics if the attachment is not a whole number of words or exceeds the protocol's attachment
/// size limit.
pub fn load_attachment(attachment_commitment: Word) -> Vec<Word> {
    load_attachment_words(attachment_commitment, MAX_ATTACHMENT_WORDS)
}

/// Loads the attachment at `attachment_idx` of an attachment commitment list from the advice map.
///
/// The advice map must contain the selected attachment elements.
///
/// # Panics
///
/// Panics if `attachment_commitments` holds more entries than a note can have attachments, if
/// `attachment_idx` is out of bounds for it, or under the conditions of [`load_attachment`].
pub fn load_indexed_attachment(attachment_commitments: &[Word], attachment_idx: u32) -> Vec<Word> {
    assert_attachment_count(attachment_commitments.len());
    load_attachment(attachment_commitments[attachment_idx as usize])
}

/// Loads and authenticates a bounded word preimage using the public core library primitives.
fn load_attachment_words(commitment: Word, max_words: usize) -> Vec<Word> {
    use miden_stdlib_sys::{adv_load_preimage, intrinsics::advice::adv_push_mapvaln};

    let num_elements = adv_push_mapvaln(commitment).as_canonical_u64();
    assert!(
        num_elements <= (max_words * 4) as u64,
        "attachment preimage exceeds protocol limit"
    );
    assert_eq!(num_elements % 4, 0, "attachment must contain whole words");
    let elements = adv_load_preimage(Felt::from_u32((num_elements / 4) as u32), commitment);
    // The whole-word assertion above guarantees there is no remainder chunk.
    elements.as_chunks::<4>().0.iter().map(|word| Word::new(*word)).collect()
}

/// Computes a note recipient from serial number, script root, and storage commitment.
pub fn compute_recipient(
    serial_num: Word,
    script_root: Word,
    storage_commitment: Word,
) -> Recipient {
    raw::compute_recipient(serial_num, script_root, storage_commitment).into()
}

/// Extracts the sender account ID from a note metadata header word.
pub fn metadata_into_sender(metadata: Word) -> AccountId {
    raw::metadata_into_sender(metadata).into()
}

/// Extracts the four attachment schemes encoded in a note metadata header word.
pub fn metadata_into_attachment_schemes(metadata: Word) -> Word {
    let (first, second, third, fourth) = raw::metadata_into_attachment_schemes(metadata);
    Word::new([first, second, third, fourth].map(Felt::from))
}

/// Extracts the note type encoded in a note metadata header word.
pub fn metadata_into_note_type(metadata: Word) -> NoteType {
    raw::metadata_into_note_type(metadata).into()
}

/// Extracts the note tag encoded in a note metadata header word.
pub fn metadata_into_tag(metadata: Word) -> Tag {
    raw::metadata_into_tag(metadata).into()
}

/// Searches a metadata header word for `attachment_scheme`.
///
/// # Panics
///
/// Panics if `attachment_scheme` does not fit in a `u16`: the protocol cannot store an attachment
/// under such a scheme, so asking for one is a caller bug.
pub fn find_attachment_idx(attachment_scheme: Felt, metadata: Word) -> Option<u32> {
    let (found, index) =
        raw::find_attachment_idx(attachment_scheme_u16(attachment_scheme), metadata);
    found.then_some(index.into())
}
