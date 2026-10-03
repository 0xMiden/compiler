extern crate alloc;
use alloc::vec::Vec;

use miden_stdlib_sys::{ElementPtr, Felt, Word};

use super::{
    MAX_ATTACHMENT_WORDS, MAX_ATTACHMENTS_PER_NOTE, assert_attachment_count,
    assert_attachment_word_count, attachment_index_u8, note_index_and_scheme_u16,
    types::{Asset, NoteId, NoteIdx, NoteMetadata, NoteType, Recipient, Tag},
};
use crate::raw::protocol::{output_note as raw, types as raw_types};

/// Creates a new output note and returns its index.
///
/// # Panics
///
/// Panics if `tag` does not fit in the protocol's `u32` note tag, or if `note_type` is neither
/// private (`0`) nor public (`1`).
///
/// # Examples
///
/// Create a note and add a single asset to it:
///
/// ```rust,ignore
/// // before using `Vec`/`vec!`.
/// extern crate alloc;
///
/// use miden::{felt, note, output_note, Asset, NoteType, Tag, Word};
///
/// // Values used to derive the note recipient.
/// let serial_num = Word::from_u64_unchecked(1, 2, 3, 4);
/// let note_script_root = Word::from_u64_unchecked(0, 0, 0, 0);
///
/// let storage = alloc::vec![felt!(0); 2];
/// let recipient = note::build_recipient(serial_num, note_script_root, storage);
///
/// let tag = Tag::from(felt!(0));
/// let note_type = NoteType::from(felt!(1)); // public note type (0b01)
///
/// let note_idx = output_note::create(tag, note_type, recipient);
/// output_note::add_asset(
///     Asset::new(
///         [felt!(0), felt!(0), felt!(0), felt!(1)],
///         [felt!(1), felt!(0), felt!(0), felt!(0)],
///     ),
///     note_idx,
/// );
/// ```
pub fn create(tag: Tag, note_type: NoteType, recipient: Recipient) -> NoteIdx {
    let note_type = raw_types::NoteType::try_from(note_type).expect("unrecognized note type");
    // Converted last, so that its `u64` canonical value is not kept across the note type check.
    let tag = u32::try_from(tag).expect("note tag exceeds u32");
    raw::create(tag, note_type, recipient.inner).into()
}

/// Adds a single-word attachment to the output note specified by `note_idx`.
pub fn add_word_attachment(note_idx: NoteIdx, attachment_scheme: Felt, attachment: Word) {
    let (note_idx, attachment_scheme) = note_index_and_scheme_u16(note_idx, attachment_scheme);
    raw::add_word_attachment(attachment_scheme, attachment, note_idx);
}

/// Adds an attachment commitment to the output note specified by `note_idx`.
///
/// The advice map must contain an entry for the attachment elements committed to by `attachment`.
pub fn add_attachment(note_idx: NoteIdx, attachment_scheme: Felt, attachment: Word) {
    let (note_idx, attachment_scheme) = note_index_and_scheme_u16(note_idx, attachment_scheme);
    raw::add_attachment(attachment_scheme, attachment, note_idx);
}

/// Adds a multi-word attachment from linear memory to the output note specified by `note_idx`.
///
/// Panics if `attachment` is empty or contains more than `MAX_ATTACHMENT_WORDS` (256) words;
/// the kernel rejects both.
pub fn add_attachment_from_memory(note_idx: NoteIdx, attachment_scheme: Felt, attachment: &[Word]) {
    assert!(!attachment.is_empty(), "note attachment cannot be empty");
    assert_attachment_word_count(attachment.len());
    // The bound above makes the length fit in the protocol's `u16`.
    let num_words = attachment.len() as u16;
    let (note_idx, attachment_scheme) = note_index_and_scheme_u16(note_idx, attachment_scheme);
    unsafe {
        raw::add_attachment_from_memory(
            attachment_scheme,
            num_words,
            ElementPtr::from_ptr(attachment.as_ptr().cast_mut()),
            note_idx,
        );
    }
}

/// Adds the asset to the output note specified by `note_idx`.
///
/// # Examples
///
/// ```rust,ignore
/// use miden::{felt, output_note, Asset, NoteIdx, Word};
///
/// // `note_idx` is returned by `output_note::create(...)`.
/// let note_idx: NoteIdx = /* ... */
///
/// let asset = Asset::new(
///     [felt!(0), felt!(0), felt!(0), felt!(1)],
///     [felt!(1), felt!(0), felt!(0), felt!(0)],
/// );
/// output_note::add_asset(asset, note_idx);
/// ```
pub fn add_asset(asset: Asset, note_idx: NoteIdx) {
    raw::add_asset(asset.into(), note_idx.to_u16());
}

/// Seals the output note at `note_index`, so that its assets and attachments can no longer be
/// changed for the rest of the transaction.
///
/// Sealing an already sealed note has no effect.
///
/// # Panics
///
/// Panics if the active account is not the native account, or if `note_index` is out of bounds
/// for the transaction's output notes.
pub fn seal(note_index: NoteIdx) {
    raw::seal(note_index.to_u16())
}

/// Returns `true` if the output note at `note_index` is sealed against asset and attachment
/// changes.
///
/// # Panics
///
/// Panics if `note_index` is out of bounds for the transaction's output notes.
pub fn is_sealed(note_index: NoteIdx) -> bool {
    raw::is_sealed(note_index.to_u16())
}

/// Contains summary information about the assets of an output note.
pub struct OutputNoteAssetsInfo {
    pub commitment: Word,
    pub num_assets: u32,
}

/// Retrieves the assets commitment and asset count for the output note at `note_index`.
pub fn get_assets_info(note_index: NoteIdx) -> OutputNoteAssetsInfo {
    let (commitment, num_assets) = raw::get_assets_info(note_index.to_u16());
    OutputNoteAssetsInfo {
        commitment,
        num_assets: num_assets.into(),
    }
}

/// Returns the assets contained in the output note at `note_index`.
pub fn get_assets(note_index: NoteIdx) -> Vec<Asset> {
    const MAX_ASSETS: usize = 256;
    let mut assets: Vec<Asset> = Vec::with_capacity(MAX_ASSETS);
    let num_assets = unsafe {
        raw::get_assets(
            ElementPtr::from_ptr(assets.as_mut_ptr().cast::<raw_types::Asset>()),
            note_index.to_u16(),
        )
    };
    unsafe {
        assets.set_len(num_assets.into());
    }
    assets
}

/// Returns the commitment over all attachments of the output note at `note_index`.
pub fn get_attachments_commitment(note_index: NoteIdx) -> Word {
    raw::get_attachments_commitment(note_index.to_u16())
}

/// Returns the recipient of the output note at `note_index`.
pub fn get_recipient(note_index: NoteIdx) -> Recipient {
    raw::get_recipient(note_index.to_u16()).into()
}

/// Returns the metadata header of the output note at `note_index`.
pub fn get_metadata(note_index: NoteIdx) -> NoteMetadata {
    NoteMetadata::new(raw::get_metadata(note_index.to_u16()))
}

/// Searches the output note metadata for `attachment_scheme`.
///
/// # Panics
///
/// Panics if `note_index` is out of bounds for the transaction's output notes, or if
/// `attachment_scheme` does not fit in a `u16`: the protocol cannot store an attachment under
/// such a scheme, so asking for one is a caller bug.
pub fn find_attachment(note_index: NoteIdx, attachment_scheme: Felt) -> Option<u32> {
    let (note_index, attachment_scheme) = note_index_and_scheme_u16(note_index, attachment_scheme);
    let (found, index) = raw::find_attachment(attachment_scheme, note_index);
    found.then_some(index.into())
}

/// Returns the attachment commitments of the output note at `note_index`.
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

/// Returns the attachment at `attachment_idx` of the output note at `note_index` as protocol
/// words.
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

/// Computes the ID of the output note at `note_index`.
///
/// The ID is only final once the note has been fully constructed, that is, once all of its assets
/// and attachments have been added.
///
/// # Panics
///
/// Panics if `note_index` is out of bounds for the transaction's output notes.
pub fn compute_note_id(note_index: NoteIdx) -> NoteId {
    raw::compute_note_id(note_index.to_u16()).into()
}
