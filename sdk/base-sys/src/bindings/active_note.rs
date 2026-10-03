extern crate alloc;
use alloc::vec::Vec;

use miden_stdlib_sys::{ElementPtr, Felt, Word};

use super::{
    AccountId, Asset, MAX_ATTACHMENT_WORDS, MAX_ATTACHMENTS_PER_NOTE, NoteId, NoteMetadata,
    Recipient, assert_attachment_count, assert_attachment_word_count, attachment_index_u8,
    attachment_scheme_u16,
};
use crate::raw::protocol::{active_note as raw, types as raw_types};

/// Contains summary information about the assets the active note was created with.
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
    // The protocol `active_note::get_storage` procedure writes the note's storage into memory
    // starting at the buffer and returns the number of storage items written. `BumpAlloc` makes
    // every allocation word-aligned, so the buffer has an element address.
    let num_inputs = unsafe { raw::get_storage(ElementPtr::from_ptr(inputs.as_mut_ptr())) };
    unsafe {
        inputs.set_len(num_inputs.into());
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
        raw::get_initial_assets(ElementPtr::from_ptr(
            inputs.as_mut_ptr().cast::<raw_types::Asset>(),
        ))
    };
    unsafe {
        inputs.set_len(num_inputs.into());
    }
    inputs
}

/// Returns the sender [`AccountId`] of the note that is currently executing.
pub fn get_sender() -> AccountId {
    raw::get_sender().into()
}

/// Returns the recipient of the note that is currently executing.
pub fn get_recipient() -> Recipient {
    raw::get_recipient().into()
}

/// Returns the script root of the currently executing note.
pub fn get_script_root() -> Word {
    raw::get_script_root()
}

/// Returns the serial number of the currently executing note.
pub fn get_serial_number() -> Word {
    raw::get_serial_number()
}

/// Returns the metadata header of the note that is currently executing.
pub fn get_metadata() -> NoteMetadata {
    NoteMetadata::new(raw::get_metadata())
}

/// Returns whether the note currently executing is public.
#[inline]
pub fn is_public() -> bool {
    raw::is_public()
}

/// Returns whether the note currently executing is private.
#[inline]
pub fn is_private() -> bool {
    raw::is_private()
}

/// Returns the commitment over all attachments of the note currently executing.
pub fn get_attachments_commitment() -> Word {
    raw::get_attachments_commitment()
}

/// Returns the attachment commitments of the active note.
///
/// The name mirrors the kernel procedure, which fills the buffer this function returns.
pub fn write_attachment_commitments_to_memory() -> Vec<Word> {
    let mut commitments: Vec<Word> = Vec::with_capacity(MAX_ATTACHMENTS_PER_NOTE);
    let num_attachments = unsafe {
        raw::write_attachment_commitments_to_memory(ElementPtr::from_ptr(commitments.as_mut_ptr()))
    };
    let num_attachments = num_attachments.into();
    assert_attachment_count(num_attachments);
    unsafe {
        commitments.set_len(num_attachments);
    }
    commitments
}

/// Returns the attachment at `attachment_idx` of the active note as protocol words.
///
/// The name mirrors the kernel procedure, which fills the buffer this function returns.
pub fn write_attachment_to_memory(attachment_idx: u32) -> Vec<Word> {
    let mut attachment: Vec<Word> = Vec::with_capacity(MAX_ATTACHMENT_WORDS);
    let num_words = unsafe {
        raw::write_attachment_to_memory(
            ElementPtr::from_ptr(attachment.as_mut_ptr()),
            attachment_index_u8(attachment_idx),
        )
    };
    let num_words = num_words.into();
    assert_attachment_word_count(num_words);
    unsafe {
        attachment.set_len(num_words);
    }
    attachment
}

/// Searches the active note metadata for `attachment_scheme`.
///
/// # Panics
///
/// Panics if `attachment_scheme` does not fit in a `u16`: the protocol cannot store an attachment
/// under such a scheme, so asking for one is a caller bug.
pub fn find_attachment(attachment_scheme: Felt) -> Option<u32> {
    let (found, index) = raw::find_attachment(attachment_scheme_u16(attachment_scheme));
    found.then_some(index.into())
}

/// Returns the initial assets commitment and asset count of the active note.
///
/// These describe the note's assets at creation time, unaffected by in-transaction removal.
pub fn get_initial_assets_info() -> ActiveNoteAssetsInfo {
    let (commitment, num_assets) = raw::get_initial_assets_info();
    ActiveNoteAssetsInfo {
        commitment,
        num_assets: num_assets.into(),
    }
}

/// Returns the number of assets the active note was created with.
///
/// The count is unaffected by in-transaction removal.
#[inline]
pub fn get_initial_num_assets() -> u32 {
    raw::get_initial_num_assets().into()
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
    raw::get_asset(u8::try_from(asset_index).expect("asset index exceeds u8")).into()
}

/// Removes `asset` from the active note and returns the asset value left in the note.
///
/// The returned value is empty when the entire asset was removed.
///
/// # Panics
///
/// Panics if the asset is not present in the note, if a non-composable asset is not present with
/// the exact value, if the note holds less of a fungible asset than is removed, if the asset id is
/// empty or malformed, or if the asset's composition is `Custom`.
pub fn remove_asset(asset: Asset) -> Word {
    raw::remove_asset(asset.into())
}

/// Returns the ID of the active note, as cached by the transaction prologue.
pub fn get_note_id() -> NoteId {
    raw::get_note_id().into()
}

/// Returns the storage commitment and storage item count of the active note.
pub fn get_storage_info() -> ActiveNoteStorageInfo {
    let (commitment, num_storage_items) = raw::get_storage_info();
    ActiveNoteStorageInfo {
        commitment,
        num_storage_items: num_storage_items.into(),
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

    /// Returns the attachment commitments of the active note.
    #[inline]
    fn write_attachment_commitments_to_memory(&self) -> Vec<Word> {
        write_attachment_commitments_to_memory()
    }

    /// Returns the attachment at `attachment_idx` of the active note as protocol words.
    #[inline]
    fn write_attachment_to_memory(&self, attachment_idx: u32) -> Vec<Word> {
        write_attachment_to_memory(attachment_idx)
    }

    /// Searches the active note metadata for `attachment_scheme`.
    ///
    /// # Panics
    ///
    /// Panics under the same conditions as [`find_attachment`].
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
    ///
    /// # Panics
    ///
    /// Panics under the same conditions as [`remove_asset`].
    #[inline]
    fn remove_asset(&mut self, asset: Asset) -> Word {
        remove_asset(asset)
    }
}
