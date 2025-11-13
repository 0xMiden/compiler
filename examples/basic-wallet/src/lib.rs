// Do not link against libstd (i.e. anything defined in `std::`)
#![no_std]

// However, we could still use some standard library types while
// remaining no-std compatible, if we uncommented the following lines:
//
// extern crate alloc;

use miden::{component, native_account, output_note, Asset, NoteIdx};

#[component]
struct MyAccount;

#[component]
impl MyAccount {
    /// Adds an asset to the account.
    ///
    /// This function adds the specified asset to the account's asset list.
    ///
    /// # Arguments
    /// * `asset` - The asset to be added to the account
    pub fn receive_asset(&self, asset: Asset) {
        native_account::add_asset(asset);
    }

    /// Moves an asset from the account to a note.
    ///
    /// This function removes the specified asset from the account and adds it to
    /// the note identified by the given index.
    ///
    /// # Arguments
    /// * `asset` - The asset to move from the account to the note
    /// * `note_idx` - The index of the note to receive the asset
    pub fn move_asset_to_note(&self, asset: Asset, note_idx: NoteIdx) {
        let asset = native_account::remove_asset(asset);
        output_note::add_asset(asset, note_idx);
    }
}
