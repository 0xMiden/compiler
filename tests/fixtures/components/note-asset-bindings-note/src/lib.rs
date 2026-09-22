//! Note script asserting the note asset and identity bindings against the note the host built.
//!
//! Everything the script compares against is either fixed by the host (the issuing faucet, passed
//! through note storage, and the asset count) or read through a second, independent binding, so a
//! binding that returns the wrong value aborts the transaction.

// Do not link against libstd (i.e. anything defined in `std::`)
#![no_std]
#![feature(alloc_error_handler)]

use miden::*;

/// Native account of the note: exposes the `basic-wallet` component methods (e.g.
/// `receive_asset`) gathered from the `basic_wallet` package.
#[account(basic_wallet::BasicWallet)]
pub struct Wallet;

/// Number of assets the host puts into the note.
const EXPECTED_NUM_ASSETS: u32 = 1;

/// Number of note storage items the host puts into the note: the two felts of the faucet id.
const EXPECTED_NUM_STORAGE_ITEMS: u32 = 2;

/// Note storage of the bindings note: the id of the faucet that issued the note's asset.
#[note]
struct NoteAssetBindingsNote {
    faucet_id: AccountId,
}

#[note]
impl NoteAssetBindingsNote {
    /// Verifies the note asset and identity bindings against the note under execution, then
    /// hands the note's asset to the consuming account.
    #[note_script]
    pub fn run(mut self, _arg: Word, account: &mut Wallet) {
        // The active note is one of the transaction's input notes, and the index it is found
        // under addresses the same note.
        let id = self.get_note_id();
        let idx = input_note::find_note(id).expect("the active note must be an input note");
        assert!(
            input_note::get_note_id(idx) == id,
            "the input note at the found index must be the active note"
        );

        // All asset counts describe the same note, whichever binding reports them.
        let assets = self.get_initial_assets();
        let num_assets = self.get_initial_num_assets();
        assert!(num_assets == EXPECTED_NUM_ASSETS, "unexpected number of note assets");
        assert!(
            self.get_initial_assets_info().num_assets == num_assets,
            "the assets info count must match the asset count"
        );
        assert!(
            assets.len() as u32 == num_assets,
            "the bulk asset read must return as many assets as the asset count"
        );
        assert!(
            input_note::get_initial_num_assets(idx) == num_assets,
            "the input note asset count must match the active note asset count"
        );

        // Indexed access returns the same asset as the bulk read, through either note view.
        let asset = self.get_asset(0);
        assert!(asset == assets[0], "the indexed asset must match the bulk read");
        assert!(
            input_note::get_asset(idx, 0) == asset,
            "the indexed input note asset must match the active note asset"
        );

        // The asset id decodes into the faucet the host minted from, a fungible composition and
        // an empty asset class (fungible assets carry no class).
        let asset_id = asset.id();
        assert!(asset_id.faucet_id() == self.faucet_id, "unexpected issuing faucet");
        assert!(
            asset_id.composition() == AssetComposition::Fungible,
            "a fungible asset must have a fungible composition"
        );
        let asset_class = asset_id.asset_class();
        assert!(
            asset_class.prefix == Felt::ZERO && asset_class.suffix == Felt::ZERO,
            "a fungible asset must have an empty asset class"
        );

        // The storage summary counts the items the host stored in the note.
        assert!(
            self.get_storage_info().num_storage_items == EXPECTED_NUM_STORAGE_ITEMS,
            "unexpected number of note storage items"
        );

        // Looking up the reference block by number yields the reference block commitment.
        assert!(
            tx::get_block_commitment(tx::get_reference_block_number())
                == tx::get_reference_block_commitment(),
            "the reference block commitment must be the commitment of the reference block"
        );

        // Removing the whole asset leaves nothing behind and clears the note's asset slot.
        let left = self.remove_asset(asset);
        assert!(left.is_empty(), "removing the whole asset must leave nothing in the note");
        let removed = self.get_asset(0);
        assert!(
            removed.key.is_empty() && removed.value.is_empty(),
            "a fully removed asset must read back as the empty asset"
        );

        // Move the removed asset into the consuming account so the transaction balances.
        account.receive_asset(asset);
    }
}
