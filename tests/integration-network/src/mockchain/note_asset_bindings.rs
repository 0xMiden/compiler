//! Mock-chain test for the note asset and note identity bindings of the Miden SDK.
//!
//! The compile-only tests for these bindings pin their Rust signatures, but not their runtime
//! behaviour: a swapped argument, a wrong return layout or a reversed felt order would still
//! compile. This test executes them inside a real transaction against a note whose contents the
//! host controls, so every binding is checked against a known-good value while the transaction
//! kernel is live.
//!
//! Covered bindings: `active_note::{get_note_id, get_initial_assets, get_initial_assets_info,
//! get_initial_num_assets, get_asset, remove_asset}`, `input_note::{find_note, get_note_id,
//! get_initial_num_assets, get_asset}`, `asset::{id_into_faucet_id, id_into_asset_class,
//! id_into_composition}` and `tx::{get_reference_block_number, get_reference_block_commitment,
//! get_block_commitment}`.

use std::{path::Path, sync::Arc};

use miden_client::{
    account::{AccountComponent, component::InitStorageData},
    asset::{Asset, FungibleAsset},
    transaction::RawOutputNote,
};
use miden_mast_package::Package;
use miden_protocol::{account::auth::AuthScheme, crypto::rand::RandomCoin};
use miden_standards::testing::note::NoteBuilder;
use miden_testing::{Auth, MockChain};
use midenc_integration_test_support::project;

use super::support::{
    assert_account_has_fungible_asset, build_send_notes_script, compile_rust_package, execute_tx,
    note_cargo_toml_for_dependency, note_miden_project_toml_for_dependency, note_script_root,
    to_core_felts,
};

/// Project name of the generated note.
const NOTE_NAME: &str = "note-asset-bindings-note";
/// Miden package name of the generated note.
const NOTE_PACKAGE: &str = "miden:note-asset-bindings-note";
/// Path of the basic-wallet example account component, relative to this crate.
const BASIC_WALLET_PROJECT: &str = "../../examples/basic-wallet";
/// Miden package name of the basic-wallet example account component.
const BASIC_WALLET_PACKAGE: &str = "miden:basic-wallet";
/// Amounts of the fungible assets carried by the two notes, one asset per note.
///
/// The transaction consumes both notes, so the note at input index 1 exercises the note-index
/// arguments of the `input_note` bindings with a non-zero index, and the differing amounts make a
/// read of the wrong note observable. The note script mirrors the per-note asset *count* (one) in
/// its `EXPECTED_NUM_ASSETS` constant.
const NOTE_ASSET_AMOUNTS: [u64; 2] = [100_000, 25_000];

/// Note script asserting the note asset and identity bindings against the note the host built.
///
/// Everything the script compares against is either fixed by the host (the issuing faucet, passed
/// through note storage, and the asset count) or read through a second, independent binding, so a
/// binding that returns the wrong value aborts the transaction.
const NOTE_ASSET_BINDINGS_NOTE_SOURCE: &str = r#"
#![no_std]
#![feature(alloc_error_handler)]

use miden::*;

/// Native account of the note: exposes the `basic-wallet` component methods (e.g.
/// `receive_asset`) gathered from the `basic_wallet` package.
#[account(basic_wallet::BasicWallet)]
pub struct Wallet;

/// Number of assets the host puts into the note.
const EXPECTED_NUM_ASSETS: u32 = 1;

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
"#;

/// Generates and compiles the bindings note project against the basic-wallet example.
fn compile_note_package(wallet_root: &Path) -> Arc<Package> {
    let note_project = project(NOTE_NAME)
        .file(
            "miden-project.toml",
            &note_miden_project_toml_for_dependency(
                NOTE_NAME,
                NOTE_PACKAGE,
                BASIC_WALLET_PACKAGE,
                wallet_root,
            ),
        )
        .file(
            "Cargo.toml",
            &note_cargo_toml_for_dependency(NOTE_NAME, BASIC_WALLET_PACKAGE, wallet_root),
        )
        .file("src/lib.rs", NOTE_ASSET_BINDINGS_NOTE_SOURCE)
        .build();

    compile_rust_package(note_project.root(), true)
}

/// Executes the note asset and identity bindings against a live transaction kernel.
///
/// Flow:
/// - The faucet emits two notes, each carrying one fungible asset and the faucet id in note
///   storage
/// - A basic-wallet account consumes both notes in one transaction, so each note script checks
///   every binding against the note the host built and against the other bindings reading the
///   same note, once at input index 0 and once at input index 1
/// - Each note script removes its asset from the note and passes it on, so the committed wallet
///   vault must hold their sum afterwards
#[test]
fn note_asset_bindings_match_the_host_note() {
    // Compile the contracts first (before creating any runtime). The wallet is compiled first so
    // that its package artifacts are available to the note project which depends on it.
    let wallet_root = Path::new(BASIC_WALLET_PROJECT)
        .canonicalize()
        .expect("the basic-wallet example project must exist");
    let wallet_package = compile_rust_package(&wallet_root, true);
    let note_package = compile_note_package(&wallet_root);

    let wallet_component = AccountComponent::from_package(
        wallet_package.as_ref().clone(),
        &InitStorageData::default(),
    )
    .unwrap();

    let mut builder = MockChain::builder();
    let max_supply = 1_000_000_000u64;
    let faucet_account = builder
        .add_existing_basic_faucet(
            Auth::BasicAuth {
                auth_scheme: AuthScheme::Falcon512Poseidon2,
            },
            "TEST",
            max_supply,
            None,
        )
        .unwrap();
    let faucet_id = faucet_account.id();

    let alice_account = builder
        .add_existing_account_from_components(
            Auth::BasicAuth {
                auth_scheme: AuthScheme::Falcon512Poseidon2,
            },
            [wallet_component],
        )
        .unwrap();
    let alice_id = alice_account.id();

    let mut chain = builder.build().unwrap();
    chain.prove_next_block().unwrap();
    chain.prove_next_block().unwrap();

    eprintln!("\n=== Step 1: Minting two bindings notes from the faucet ===");
    let mut note_rng = RandomCoin::new(note_script_root(note_package.as_ref()));
    let notes = NOTE_ASSET_AMOUNTS.map(|amount| {
        let mint_asset = FungibleAsset::new(faucet_id, amount).unwrap();
        NoteBuilder::new(faucet_id, &mut note_rng)
            .package((*note_package).clone())
            .add_assets([Asset::from(mint_asset)])
            .note_storage(to_core_felts(&faucet_id))
            .unwrap()
            .build()
            .unwrap()
    });

    let faucet_account = chain.committed_account(faucet_id).unwrap().clone();
    let mint_tx_script = build_send_notes_script(&faucet_account, &notes);
    let mint_tx = chain
        .build_transaction(faucet_id)
        .send_notes_script(&mint_tx_script)
        .expected_output_notes(notes.iter().cloned().map(RawOutputNote::Full).collect::<Vec<_>>())
        .build()
        .unwrap();
    execute_tx(&mut chain, mint_tx);

    eprintln!("\n=== Step 2: Alice consumes both notes; the scripts assert the bindings ===");
    let faucet_inputs = chain.get_foreign_account_inputs(faucet_id).unwrap();
    let consume_tx = chain
        .build_transaction(alice_id)
        .authenticated_input_notes([notes[0].id(), notes[1].id()])
        .foreign_accounts(vec![faucet_inputs])
        .build()
        .unwrap();
    execute_tx(&mut chain, consume_tx);

    eprintln!("\n=== Step 3: Checking the removed assets reached Alice's committed vault ===");
    let alice_account = chain.committed_account(alice_id).unwrap();
    assert_account_has_fungible_asset(
        alice_account,
        faucet_id,
        NOTE_ASSET_AMOUNTS.iter().sum::<u64>(),
    );
}
