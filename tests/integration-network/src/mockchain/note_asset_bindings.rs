//! Mock-chain test for the note asset and note identity bindings of the Miden SDK.
//!
//! This test exercises the `tests/fixtures/components/note-asset-bindings-note` note script. The
//! compile-only tests for these bindings pin their Rust signatures, but not their runtime
//! behaviour: a swapped argument, a wrong return layout or a reversed felt order would still
//! compile. This test executes them inside a real transaction against a note whose contents the
//! host controls, so every binding is checked against a known-good value while the transaction
//! kernel is live.
//!
//! Covered bindings: `active_note::{get_note_id, get_initial_assets, get_initial_assets_info,
//! get_initial_num_assets, get_asset, remove_asset, get_storage_info}`, `input_note::{find_note,
//! get_note_id, get_initial_num_assets, get_asset}`, `asset::{id_into_faucet_id,
//! id_into_asset_class, id_into_composition}` and `tx::{get_reference_block_number,
//! get_reference_block_commitment, get_block_commitment}`.
//!
//! Not executed here, and so still covered by compile-only tests alone: `tx::{compute_fee,
//! get_fee_asset_id}`, `output_note::compute_note_id`, `input_note::remove_asset`,
//! `native_account::{has_state_changed, has_initial_asset}` and
//! `active_account::has_storage_slot`.

use miden_client::{
    account::{AccountComponent, component::InitStorageData},
    asset::{Asset, FungibleAsset},
    transaction::RawOutputNote,
};
use miden_protocol::{account::auth::AuthScheme, crypto::rand::RandomCoin};
use miden_standards::testing::note::NoteBuilder;
use miden_testing::{Auth, MockChain};

use super::support::{
    assert_account_has_fungible_asset, build_send_notes_script, compile_rust_package, execute_tx,
    note_script_root, to_core_felts,
};

/// Path of the basic-wallet example account component, relative to this crate.
const BASIC_WALLET_PROJECT: &str = "../../examples/basic-wallet";
/// Path of the bindings note script fixture, relative to this crate.
const NOTE_ASSET_BINDINGS_NOTE_PROJECT: &str = "../fixtures/components/note-asset-bindings-note";
/// Amounts of the fungible assets carried by the two notes, one asset per note.
///
/// The transaction consumes both notes, so the note at input index 1 exercises the note-index
/// arguments of the `input_note` bindings with a non-zero index, and the differing amounts make a
/// read of the wrong note observable. The note script mirrors the per-note asset *count* (one) in
/// the `EXPECTED_NUM_ASSETS` constant of the fixture.
const NOTE_ASSET_AMOUNTS: [u64; 2] = [100_000, 25_000];

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
    let wallet_package = compile_rust_package(BASIC_WALLET_PROJECT, true);
    let note_package = compile_rust_package(NOTE_ASSET_BINDINGS_NOTE_PROJECT, true);

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
