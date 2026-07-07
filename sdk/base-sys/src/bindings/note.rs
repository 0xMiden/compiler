extern crate alloc;

use alloc::vec::Vec;

use miden_stdlib_sys::{Felt, Word};

use super::{AccountId, AttachmentLocation, NoteType, RawAccountId, Recipient, Tag};

const MAX_NOTE_STORAGE_ITEMS: usize = 1024;
const MAX_ATTACHMENTS_PER_NOTE: usize = 4;
const MAX_ATTACHMENT_WORDS: usize = 256;

#[allow(improper_ctypes)]
unsafe extern "C" {
    #[cfg_attr(target_family = "wasm", linkage = "extern_weak")]
    #[link_name = "miden::protocol::note::compute_and_store_recipient"]
    fn extern_note_build_recipient(
        storage_ptr: *mut Felt,
        num_storage_items: usize,
        serial_num_f0: Felt,
        serial_num_f1: Felt,
        serial_num_f2: Felt,
        serial_num_f3: Felt,
        script_root_f0: Felt,
        script_root_f1: Felt,
        script_root_f2: Felt,
        script_root_f3: Felt,
        ptr: *mut Recipient,
    );
    #[cfg_attr(target_family = "wasm", linkage = "extern_weak")]
    #[link_name = "miden::protocol::note::compute_storage_commitment"]
    fn extern_note_compute_storage_commitment(
        storage_ptr: *const Felt,
        num_storage_items: usize,
        ptr: *mut Word,
    );
    #[cfg_attr(target_family = "wasm", linkage = "extern_weak")]
    #[link_name = "miden::protocol::note::write_attachment_commitments_to_memory"]
    fn extern_note_write_attachment_commitments_to_memory(
        attachments_commitment_f0: Felt,
        attachments_commitment_f1: Felt,
        attachments_commitment_f2: Felt,
        attachments_commitment_f3: Felt,
        dest_ptr: *mut Felt,
    ) -> usize;
    #[cfg_attr(target_family = "wasm", linkage = "extern_weak")]
    #[link_name = "miden::protocol::note::write_attachment_to_memory"]
    fn extern_note_write_attachment_to_memory(
        attachment_commitment_f0: Felt,
        attachment_commitment_f1: Felt,
        attachment_commitment_f2: Felt,
        attachment_commitment_f3: Felt,
        dest_ptr: *mut Felt,
    ) -> usize;
    #[cfg_attr(target_family = "wasm", linkage = "extern_weak")]
    #[link_name = "miden::protocol::note::write_indexed_attachment_to_memory"]
    fn extern_note_write_indexed_attachment_to_memory(
        num_attachments: Felt,
        attachment_commitments_ptr: *const Felt,
        attachment_idx: Felt,
        dest_ptr: *mut Felt,
    ) -> usize;
    #[cfg_attr(target_family = "wasm", linkage = "extern_weak")]
    #[link_name = "miden::protocol::note::compute_recipient"]
    fn extern_note_compute_recipient(
        serial_num_f0: Felt,
        serial_num_f1: Felt,
        serial_num_f2: Felt,
        serial_num_f3: Felt,
        script_root_f0: Felt,
        script_root_f1: Felt,
        script_root_f2: Felt,
        script_root_f3: Felt,
        storage_commitment_f0: Felt,
        storage_commitment_f1: Felt,
        storage_commitment_f2: Felt,
        storage_commitment_f3: Felt,
        ptr: *mut Recipient,
    );
    #[cfg_attr(target_family = "wasm", linkage = "extern_weak")]
    #[link_name = "miden::protocol::note::metadata_into_sender"]
    fn extern_note_metadata_into_sender(
        metadata_f0: Felt,
        metadata_f1: Felt,
        metadata_f2: Felt,
        metadata_f3: Felt,
        ptr: *mut RawAccountId,
    );
    #[cfg_attr(target_family = "wasm", linkage = "extern_weak")]
    #[link_name = "miden::protocol::note::metadata_into_attachment_schemes"]
    fn extern_note_metadata_into_attachment_schemes(
        metadata_f0: Felt,
        metadata_f1: Felt,
        metadata_f2: Felt,
        metadata_f3: Felt,
        ptr: *mut Word,
    );
    #[cfg_attr(target_family = "wasm", linkage = "extern_weak")]
    #[link_name = "miden::protocol::note::metadata_into_note_type"]
    fn extern_note_metadata_into_note_type(
        metadata_f0: Felt,
        metadata_f1: Felt,
        metadata_f2: Felt,
        metadata_f3: Felt,
    ) -> Felt;
    #[cfg_attr(target_family = "wasm", linkage = "extern_weak")]
    #[link_name = "miden::protocol::note::metadata_into_tag"]
    fn extern_note_metadata_into_tag(
        metadata_f0: Felt,
        metadata_f1: Felt,
        metadata_f2: Felt,
        metadata_f3: Felt,
    ) -> Felt;
    #[cfg_attr(target_family = "wasm", linkage = "extern_weak")]
    #[link_name = "miden::protocol::note::find_attachment_idx"]
    fn extern_note_find_attachment_idx(
        attachment_scheme: Felt,
        metadata_f0: Felt,
        metadata_f1: Felt,
        metadata_f2: Felt,
        metadata_f3: Felt,
        ptr: *mut AttachmentLocation,
    );
    #[cfg_attr(target_family = "wasm", linkage = "extern_weak")]
    #[link_name = "intrinsics::note::script_root"]
    fn extern_note_script_root(entrypoint_fn_ref: usize, ptr: *mut Word);
}

/// Returns the MAST root digest of the note script whose entrypoint is referenced by
/// `entrypoint_fn_ref` (a Rust function reference to the crate's `#[note_script]` entrypoint,
/// cast to `usize`).
///
/// Macro plumbing behind the `get_entrypoint_root()` associated method that `#[note]` generates
/// on the note input type — call that method instead of this function. It lives here because
/// the underlying weak extern requires `feature(linkage)`, which user crates do not enable.
///
/// The compiler resolves the reference to its function-table slot and repoints the slot at the
/// note-script export, so the returned word is the note script root observed by the transaction
/// kernel; the digest itself is resolved at assembly time via `procref`.
///
/// Compilation fails if the current project does not define a `#[note_script]` entrypoint, or if
/// `entrypoint_fn_ref` is not a compile-time constant function reference.
#[doc(hidden)]
#[inline(always)]
pub fn __entrypoint_root_from_fn_ref(entrypoint_fn_ref: usize) -> Word {
    unsafe {
        let mut ret_area = ::core::mem::MaybeUninit::<Word>::uninit();
        extern_note_script_root(entrypoint_fn_ref, ret_area.as_mut_ptr());
        ret_area.assume_init()
    }
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

    let rust_ptr = if storage.is_empty() {
        0
    } else {
        storage.as_ptr().addr() as u32
    };
    let miden_ptr = rust_ptr / 4;

    // Vec storage comes from the SDK allocator, which only produces word-aligned pointers.
    assert_eq!(miden_ptr % 4, 0, "storage pointer must be word-aligned");

    unsafe {
        let mut ret_area = ::core::mem::MaybeUninit::<Recipient>::uninit();
        extern_note_build_recipient(
            miden_ptr as *mut Felt,
            storage.len(),
            serial_num[0],
            serial_num[1],
            serial_num[2],
            serial_num[3],
            script_root[0],
            script_root[1],
            script_root[2],
            script_root[3],
            ret_area.as_mut_ptr(),
        );
        ret_area.assume_init()
    }
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

    let rust_ptr = if storage.is_empty() {
        0
    } else {
        storage.as_ptr().addr() as u32
    };
    let miden_ptr = rust_ptr / 4;

    assert_eq!(miden_ptr % 4, 0, "storage pointer must be word-aligned");

    unsafe {
        let mut ret_area = ::core::mem::MaybeUninit::<Word>::uninit();
        extern_note_compute_storage_commitment(
            miden_ptr as *const Felt,
            storage.len(),
            ret_area.as_mut_ptr(),
        );
        ret_area.assume_init()
    }
}

/// Writes attachment commitments from the advice map to memory.
///
/// The advice map must contain the preimage committed to by `attachments_commitment`.
pub fn write_attachment_commitments_to_memory(attachments_commitment: Word) -> Vec<Word> {
    let mut commitments: Vec<Word> = Vec::with_capacity(MAX_ATTACHMENTS_PER_NOTE);
    let num_attachments = unsafe {
        let ptr = (commitments.as_mut_ptr().addr() / 4) as u32;
        extern_note_write_attachment_commitments_to_memory(
            attachments_commitment[0],
            attachments_commitment[1],
            attachments_commitment[2],
            attachments_commitment[3],
            ptr as *mut Felt,
        )
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

/// Writes one attachment from the advice map to memory.
///
/// The advice map must contain the attachment elements committed to by `attachment_commitment`.
pub fn write_attachment_to_memory(attachment_commitment: Word) -> Vec<Word> {
    let mut attachment: Vec<Word> = Vec::with_capacity(MAX_ATTACHMENT_WORDS);
    let num_words = unsafe {
        let ptr = (attachment.as_mut_ptr().addr() / 4) as u32;
        extern_note_write_attachment_to_memory(
            attachment_commitment[0],
            attachment_commitment[1],
            attachment_commitment[2],
            attachment_commitment[3],
            ptr as *mut Felt,
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

/// Writes the indexed attachment from an attachment commitment list to memory.
///
/// The advice map must contain the selected attachment elements.
pub fn write_indexed_attachment_to_memory(
    attachment_commitments: &[Word],
    attachment_idx: Felt,
) -> Vec<Word> {
    assert!(
        attachment_commitments.len() <= MAX_ATTACHMENTS_PER_NOTE,
        "note cannot contain more than {MAX_ATTACHMENTS_PER_NOTE} attachments"
    );

    let mut attachment: Vec<Word> = Vec::with_capacity(MAX_ATTACHMENT_WORDS);
    let num_words = unsafe {
        let commitments_ptr = if attachment_commitments.is_empty() {
            0
        } else {
            (attachment_commitments.as_ptr().addr() / 4) as u32
        };
        let dest_ptr = (attachment.as_mut_ptr().addr() / 4) as u32;
        extern_note_write_indexed_attachment_to_memory(
            Felt::from_u32(attachment_commitments.len() as u32),
            commitments_ptr as *const Felt,
            attachment_idx,
            dest_ptr as *mut Felt,
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

/// Computes a note recipient from serial number, script root, and storage commitment.
pub fn compute_recipient(
    serial_num: Word,
    script_root: Word,
    storage_commitment: Word,
) -> Recipient {
    unsafe {
        let mut ret_area = ::core::mem::MaybeUninit::<Recipient>::uninit();
        extern_note_compute_recipient(
            serial_num[0],
            serial_num[1],
            serial_num[2],
            serial_num[3],
            script_root[0],
            script_root[1],
            script_root[2],
            script_root[3],
            storage_commitment[0],
            storage_commitment[1],
            storage_commitment[2],
            storage_commitment[3],
            ret_area.as_mut_ptr(),
        );
        ret_area.assume_init()
    }
}

/// Extracts the sender account ID from a note metadata header word.
pub fn metadata_into_sender(metadata: Word) -> AccountId {
    unsafe {
        let mut ret_area = ::core::mem::MaybeUninit::<RawAccountId>::uninit();
        extern_note_metadata_into_sender(
            metadata[0],
            metadata[1],
            metadata[2],
            metadata[3],
            ret_area.as_mut_ptr(),
        );
        ret_area.assume_init().into_account_id()
    }
}

/// Extracts the four attachment schemes encoded in a note metadata header word.
pub fn metadata_into_attachment_schemes(metadata: Word) -> Word {
    unsafe {
        let mut ret_area = ::core::mem::MaybeUninit::<Word>::uninit();
        extern_note_metadata_into_attachment_schemes(
            metadata[0],
            metadata[1],
            metadata[2],
            metadata[3],
            ret_area.as_mut_ptr(),
        );
        ret_area.assume_init()
    }
}

/// Extracts the note type encoded in a note metadata header word.
pub fn metadata_into_note_type(metadata: Word) -> NoteType {
    unsafe {
        NoteType::from(extern_note_metadata_into_note_type(
            metadata[0],
            metadata[1],
            metadata[2],
            metadata[3],
        ))
    }
}

/// Extracts the note tag encoded in a note metadata header word.
pub fn metadata_into_tag(metadata: Word) -> Tag {
    unsafe {
        Tag::from(extern_note_metadata_into_tag(
            metadata[0],
            metadata[1],
            metadata[2],
            metadata[3],
        ))
    }
}

/// Searches a metadata header word for `attachment_scheme`.
pub fn find_attachment_idx(attachment_scheme: Felt, metadata: Word) -> AttachmentLocation {
    unsafe {
        let mut ret_area = ::core::mem::MaybeUninit::<AttachmentLocation>::uninit();
        extern_note_find_attachment_idx(
            attachment_scheme,
            metadata[0],
            metadata[1],
            metadata[2],
            metadata[3],
            ret_area.as_mut_ptr(),
        );
        ret_area.assume_init()
    }
}
