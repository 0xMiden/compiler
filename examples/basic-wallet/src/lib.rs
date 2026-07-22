// Do not link against libstd (i.e. anything defined in `std::`)
#![no_std]
#![feature(alloc_error_handler)]

// However, we could still use some standard library types while
// remaining no-std compatible, if we uncommented the following lines:
//
// extern crate alloc;

use miden::{Asset, NoteIdx, NoteType, Recipient, Tag, component, component_storage, output_note};

#[component_storage]
struct BasicWalletStorage;

/// API of the basic wallet account component.
#[component]
trait BasicWallet {
    /// Adds an asset to the account.
    ///
    /// This function adds the specified asset to the account's asset list.
    ///
    /// # Arguments
    /// * `asset` - The asset to be added to the account
    fn receive_asset(&mut self, asset: Asset);

    /// Moves an asset from the account to a note.
    ///
    /// This function removes the specified asset from the account and adds it to
    /// the note identified by the given index.
    ///
    /// # Arguments
    /// * `asset` - The asset to move from the account to the note
    /// * `note_idx` - The index of the note to receive the asset
    fn move_asset_to_note(&mut self, asset: Asset, note_idx: NoteIdx);

    /// Creates an output note and returns its index.
    ///
    /// `output_note::create` may only be called from an account component
    /// context, so transaction scripts must create notes through this method.
    ///
    /// # Arguments
    /// * `tag` - The note tag
    /// * `note_type` - The note type (public/private/encrypted)
    /// * `recipient` - The note recipient digest
    fn create_note(&mut self, tag: Tag, note_type: NoteType, recipient: Recipient) -> NoteIdx;
}

#[component]
impl BasicWallet for BasicWalletStorage {
    fn receive_asset(&mut self, asset: Asset) {
        self.add_asset(asset);
    }

    fn move_asset_to_note(&mut self, asset: Asset, note_idx: NoteIdx) {
        self.remove_asset(asset);
        output_note::add_asset(asset, note_idx);
    }

    fn create_note(&mut self, tag: Tag, note_type: NoteType, recipient: Recipient) -> NoteIdx {
        output_note::create(tag, note_type, recipient)
    }
}
