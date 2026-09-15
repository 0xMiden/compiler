extern crate alloc;
use alloc::vec::Vec;

use miden_stdlib_sys::{Felt, Word, WordAligned};

use super::{
    AccountId, Asset, MAX_ATTACHMENT_WORDS, MAX_ATTACHMENTS_PER_NOTE, NoteId, NoteMetadata,
    RawAccountId, RawAttachmentLocation, RawCommitmentWithCount, Recipient,
    assert_attachment_count, assert_attachment_word_count,
};

#[allow(improper_ctypes)]
unsafe extern "C" {
    // NOTE: In protocol v0.14, note "inputs" are exposed via `active_note::get_storage`.
    #[cfg_attr(target_family = "wasm", linkage = "extern_weak")]
    #[link_name = "miden::protocol::active_note::get_storage"]
    fn extern_note_get_storage(ptr: *mut Felt) -> usize;
    #[cfg_attr(target_family = "wasm", linkage = "extern_weak")]
    #[link_name = "miden::protocol::active_note::get_initial_assets"]
    fn extern_note_get_initial_assets(ptr: *mut Felt) -> usize;
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
    fn extern_note_find_attachment(attachment_scheme: Felt, ptr: *mut RawAttachmentLocation);
    #[cfg_attr(target_family = "wasm", linkage = "extern_weak")]
    #[link_name = "miden::protocol::active_note::get_initial_assets_info"]
    fn extern_active_note_get_initial_assets_info(ptr: *mut RawCommitmentWithCount);
    #[cfg_attr(target_family = "wasm", linkage = "extern_weak")]
    #[link_name = "miden::protocol::active_note::get_initial_num_assets"]
    fn extern_active_note_get_initial_num_assets() -> Felt;
    #[cfg_attr(target_family = "wasm", linkage = "extern_weak")]
    #[link_name = "miden::protocol::active_note::get_asset"]
    fn extern_active_note_get_asset(asset_index: Felt, ptr: *mut Asset);
    #[cfg_attr(target_family = "wasm", linkage = "extern_weak")]
    #[link_name = "miden::protocol::active_note::remove_asset"]
    fn extern_active_note_remove_asset(
        asset_id_0: Felt,
        asset_id_1: Felt,
        asset_id_2: Felt,
        asset_id_3: Felt,
        asset_value_0: Felt,
        asset_value_1: Felt,
        asset_value_2: Felt,
        asset_value_3: Felt,
        ptr: *mut Word,
    );
    #[cfg_attr(target_family = "wasm", linkage = "extern_weak")]
    #[link_name = "miden::protocol::active_note::get_note_id"]
    fn extern_active_note_get_note_id(ptr: *mut NoteId);
    #[cfg_attr(target_family = "wasm", linkage = "extern_weak")]
    #[link_name = "miden::protocol::active_note::get_storage_info"]
    fn extern_active_note_get_storage_info(ptr: *mut RawCommitmentWithCount);
}

/// Contains summary information about the assets stored in the active note.
pub struct ActiveNoteAssetsInfo {
    /// The commitment over the assets the note was created with.
    pub commitment: Word,
    /// The number of assets the note was created with.
    pub num_assets: u32,
}

/// Contains summary information about the storage stored in the active note.
pub struct ActiveNoteStorageInfo {
    /// The commitment over the note's storage.
    pub commitment: Word,
    /// The number of storage items the note was created with.
    pub num_storage_items: u32,
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

/// Get the initial assets of the currently executing note.
///
/// These are the note's assets at creation time, unaffected by in-transaction removal.
pub fn get_initial_assets() -> Vec<Asset> {
    const MAX_INPUTS: usize = 256;
    let mut inputs: Vec<Asset> = Vec::with_capacity(MAX_INPUTS);
    let num_inputs = unsafe {
        let ptr = (inputs.as_mut_ptr() as usize) / 4;
        extern_note_get_initial_assets(ptr as *mut Felt)
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
    assert_attachment_count(num_attachments);
    unsafe {
        commitments.set_len(num_attachments);
    }
    commitments
}

/// Writes the selected attachment to memory and returns it as protocol words.
pub fn write_attachment_to_memory(attachment_idx: u32) -> Vec<Word> {
    let mut attachment: Vec<Word> = Vec::with_capacity(MAX_ATTACHMENT_WORDS);
    let num_words = unsafe {
        let ptr = (attachment.as_mut_ptr() as usize) / 4;
        extern_note_write_attachment_to_memory(ptr as *mut Felt, Felt::from_u32(attachment_idx))
    };
    assert_attachment_word_count(num_words);
    unsafe {
        attachment.set_len(num_words);
    }
    attachment
}

/// Searches the active note metadata for `attachment_scheme`.
pub fn find_attachment(attachment_scheme: Felt) -> Option<u32> {
    unsafe {
        let mut ret_area =
            WordAligned::new(::core::mem::MaybeUninit::<RawAttachmentLocation>::uninit());
        extern_note_find_attachment(attachment_scheme, ret_area.as_mut_ptr());
        ret_area.into_inner().assume_init().into_attachment_index()
    }
}

/// Returns the initial assets commitment and asset count of the active note.
///
/// These describe the note's assets at creation time, unaffected by in-transaction removal.
pub fn get_initial_assets_info() -> ActiveNoteAssetsInfo {
    unsafe {
        let mut ret_area =
            WordAligned::new(::core::mem::MaybeUninit::<RawCommitmentWithCount>::uninit());
        extern_active_note_get_initial_assets_info(ret_area.as_mut_ptr());
        let raw = ret_area.into_inner().assume_init();
        ActiveNoteAssetsInfo {
            commitment: raw.commitment,
            // The transaction kernel guarantees asset counts fit in a u32.
            num_assets: raw.count.as_canonical_u64() as u32,
        }
    }
}

/// Returns the number of assets the active note was created with.
///
/// The count is unaffected by in-transaction removal.
#[inline]
pub fn get_initial_num_assets() -> u32 {
    // The transaction kernel guarantees asset counts fit in a u32.
    let count = unsafe { extern_active_note_get_initial_num_assets() };
    count.as_canonical_u64() as u32
}

/// Returns the asset at `asset_index` in the active note.
///
/// The asset is returned as it currently is: an asset that was already removed from the note reads
/// back with both words empty.
///
/// # Panics
///
/// Panics if `asset_index` is out of bounds for the note.
pub fn get_asset(asset_index: u32) -> Asset {
    unsafe {
        let mut ret_area = WordAligned::new(::core::mem::MaybeUninit::<Asset>::uninit());
        extern_active_note_get_asset(Felt::from_u32(asset_index), ret_area.as_mut_ptr());
        ret_area.into_inner().assume_init()
    }
}

/// Removes `asset` from the active note and returns the asset value left in the note.
///
/// The returned value is empty when the entire asset was removed.
pub fn remove_asset(asset: Asset) -> Word {
    unsafe {
        let mut ret_area = WordAligned::new(::core::mem::MaybeUninit::<Word>::uninit());
        extern_active_note_remove_asset(
            asset.key[0],
            asset.key[1],
            asset.key[2],
            asset.key[3],
            asset.value[0],
            asset.value[1],
            asset.value[2],
            asset.value[3],
            ret_area.as_mut_ptr(),
        );
        ret_area.into_inner().assume_init()
    }
}

/// Returns the ID of the active note, as cached by the transaction prologue.
pub fn get_note_id() -> NoteId {
    unsafe {
        let mut ret_area = WordAligned::new(::core::mem::MaybeUninit::<NoteId>::uninit());
        extern_active_note_get_note_id(ret_area.as_mut_ptr());
        ret_area.into_inner().assume_init()
    }
}

/// Returns the storage commitment and storage item count of the active note.
pub fn get_storage_info() -> ActiveNoteStorageInfo {
    unsafe {
        let mut ret_area =
            WordAligned::new(::core::mem::MaybeUninit::<RawCommitmentWithCount>::uninit());
        extern_active_note_get_storage_info(ret_area.as_mut_ptr());
        let raw = ret_area.into_inner().assume_init();
        ActiveNoteStorageInfo {
            commitment: raw.commitment,
            // The transaction kernel guarantees storage item counts fit in a u32.
            num_storage_items: raw.count.as_canonical_u64() as u32,
        }
    }
}

/// Trait that provides active-note operations for note scripts.
///
/// This trait is automatically implemented for the note input struct marked with the `#[note]`
/// macro, so a `#[note_script]` entrypoint can call the operations directly on `self`, e.g.
/// `self.get_sender()`.
///
/// The operations read the note that is currently executing. Call them only during note-script
/// execution: a note value constructed outside of it (for example in a `#[note_constructor]`)
/// has no active note, and the transaction kernel rejects the calls at run time.
///
/// `get_storage` is intentionally not part of this trait: the `#[note]` macro decodes the note
/// storage into the struct fields, so the values are available directly on `self`.
///
/// An inherent method of the note struct with the same name shadows the trait method; the trait
/// method stays reachable with UFCS, e.g. `<MyNote as ActiveNote>::get_sender(&note)`.
pub trait ActiveNote {
    /// Get the initial assets of the currently executing note.
    ///
    /// These are the note's assets at creation time, unaffected by in-transaction removal.
    #[inline]
    fn get_initial_assets(&self) -> Vec<Asset> {
        get_initial_assets()
    }

    /// Returns the sender [`AccountId`] of the note that is currently executing.
    #[inline]
    fn get_sender(&self) -> AccountId {
        get_sender()
    }

    /// Returns the recipient of the note that is currently executing.
    #[inline]
    fn get_recipient(&self) -> Recipient {
        get_recipient()
    }

    /// Returns the script root of the currently executing note.
    #[inline]
    fn get_script_root(&self) -> Word {
        get_script_root()
    }

    /// Returns the serial number of the currently executing note.
    #[inline]
    fn get_serial_number(&self) -> Word {
        get_serial_number()
    }

    /// Returns the metadata header of the note that is currently executing.
    #[inline]
    fn get_metadata(&self) -> NoteMetadata {
        get_metadata()
    }

    /// Returns whether the note currently executing is public.
    #[inline]
    fn is_public(&self) -> bool {
        is_public()
    }

    /// Returns whether the note currently executing is private.
    #[inline]
    fn is_private(&self) -> bool {
        is_private()
    }

    /// Returns the commitment over all attachments of the note currently executing.
    #[inline]
    fn get_attachments_commitment(&self) -> Word {
        get_attachments_commitment()
    }

    /// Writes attachment commitments to memory and returns them as protocol words.
    #[inline]
    fn write_attachment_commitments_to_memory(&self) -> Vec<Word> {
        write_attachment_commitments_to_memory()
    }

    /// Writes the selected attachment to memory and returns it as protocol words.
    #[inline]
    fn write_attachment_to_memory(&self, attachment_idx: u32) -> Vec<Word> {
        write_attachment_to_memory(attachment_idx)
    }

    /// Searches the active note metadata for `attachment_scheme`.
    #[inline]
    fn find_attachment(&self, attachment_scheme: Felt) -> Option<u32> {
        find_attachment(attachment_scheme)
    }

    /// Returns the initial assets commitment and asset count of the note that is currently
    /// executing.
    ///
    /// These describe the note's assets at creation time, unaffected by in-transaction removal.
    #[inline]
    fn get_initial_assets_info(&self) -> ActiveNoteAssetsInfo {
        get_initial_assets_info()
    }

    /// Returns the number of assets the currently executing note was created with.
    ///
    /// The count is unaffected by in-transaction removal.
    #[inline]
    fn get_initial_num_assets(&self) -> u32 {
        get_initial_num_assets()
    }

    /// Returns the asset at `asset_index` in the note that is currently executing.
    ///
    /// The asset is returned as it currently is: an asset that was already removed from the note
    /// reads back with both words empty.
    ///
    /// # Panics
    ///
    /// Panics if `asset_index` is out of bounds for the note.
    #[inline]
    fn get_asset(&self, asset_index: u32) -> Asset {
        get_asset(asset_index)
    }

    /// Returns the ID of the note that is currently executing.
    #[inline]
    fn get_note_id(&self) -> NoteId {
        get_note_id()
    }

    /// Returns the storage commitment and storage item count of the currently executing note.
    #[inline]
    fn get_storage_info(&self) -> ActiveNoteStorageInfo {
        get_storage_info()
    }

    /// Removes `asset` from the currently executing note and returns the asset value left in it.
    ///
    /// The returned value is empty when the entire asset was removed. This mutates the note's
    /// assets in the transaction kernel, so the note script needs a `mut self` receiver to call it.
    #[inline]
    fn remove_asset(&mut self, asset: Asset) -> Word {
        remove_asset(asset)
    }
}
