//! Note-constructor test module.
//!
//! Exercises the flow where a transaction script creates an output note by calling the note
//! package's exported constructor: the note recipient — including the note script root obtained
//! via `note::get_entrypoint_root()` — is computed inside the note package, not supplied by the
//! host.

use miden_client::{
    account::{AccountComponent, component::InitStorageData},
    asset::{Asset, AssetCallbackFlag, FungibleAsset},
    crypto::FeltRng,
    transaction::RawOutputNote,
};
use miden_core::Felt;
use miden_protocol::{account::auth::AuthScheme, crypto::rand::RandomCoin, note::NoteType};
use miden_standards::testing::note::NoteBuilder;
use miden_testing::{Auth, MockChain};
use midenc_expect_test::expect;

use super::super::support::{
    assert_account_has_fungible_asset, build_send_notes_script, compile_rust_package, execute_tx,
    note_script_root, single_note_cycles, to_core_felts, transaction_script_from_package_with_deps,
    tx_script_processing_cycles,
};

/// Tests that a transaction script can create a P2ID note through the note package's exported
/// constructor, and that the created note is consumable with the note script of the standalone
/// note package.
///
/// This proves that the note script root the constructor commits to (computed in-VM via
/// `note::get_entrypoint_root()`) equals the root of the `@note_script` procedure of the
/// compiled note package.
#[test]
pub fn tx_script_creates_p2id_note_via_note_constructor() {
    // Compile the contracts first (before creating any runtime)
    let wallet_package = compile_rust_package("../../examples/basic-wallet", true);
    let note_package = compile_rust_package("../../examples/p2id-note", true);
    let tx_script_package = compile_rust_package("../../examples/p2id-tx-script", true);

    let wallet_component =
        AccountComponent::from_package(&wallet_package, &InitStorageData::default()).unwrap();

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
            [wallet_component.clone()],
        )
        .unwrap();
    let alice_id = alice_account.id();

    let bob_account = builder
        .add_existing_account_from_components(
            Auth::BasicAuth {
                auth_scheme: AuthScheme::Falcon512Poseidon2,
            },
            [wallet_component],
        )
        .unwrap();
    let bob_id = bob_account.id();

    let mut chain = builder.build().unwrap();
    chain.prove_next_block().unwrap();
    chain.prove_next_block().unwrap();

    eprintln!("\n=== Step 1: Minting tokens from faucet to Alice ===");
    let mint_amount = 100_000u64;
    let mint_asset = FungibleAsset::new(faucet_id, mint_amount)
        .unwrap()
        .with_callbacks(AssetCallbackFlag::Enabled);

    let mut note_rng = RandomCoin::new(note_script_root(note_package.as_ref()));
    let p2id_note_mint = NoteBuilder::new(faucet_id, &mut note_rng)
        .package((*note_package).clone())
        .add_assets([Asset::from(mint_asset)])
        .note_storage(to_core_felts(&alice_id))
        .unwrap()
        .build()
        .unwrap();

    let faucet_account = chain.committed_account(faucet_id).unwrap().clone();
    let mint_tx_script =
        build_send_notes_script(&faucet_account, std::slice::from_ref(&p2id_note_mint));
    let mint_tx_context_builder = chain
        .build_tx_context(faucet_id, &[], &[])
        .unwrap()
        .tx_script(mint_tx_script)
        .extend_expected_output_notes(vec![RawOutputNote::Full(p2id_note_mint.clone())]);
    execute_tx(&mut chain, mint_tx_context_builder);

    eprintln!("\n=== Step 2: Alice consumes mint note ===");
    let faucet_inputs = chain.get_foreign_account_inputs(faucet_id).unwrap();
    let consume_tx_context_builder = chain
        .build_tx_context(alice_id, &[p2id_note_mint.id()], &[])
        .unwrap()
        .foreign_accounts(vec![faucet_inputs]);
    execute_tx(&mut chain, consume_tx_context_builder);

    let alice_account = chain.committed_account(alice_id).unwrap();
    assert_account_has_fungible_asset(alice_account, faucet_id, mint_amount);

    eprintln!("\n=== Step 3: Alice creates p2id note for Bob via the note constructor ===");
    let transfer_amount = 10_000u64;
    let transfer_asset = FungibleAsset::new(faucet_id, transfer_amount)
        .unwrap()
        .with_callbacks(AssetCallbackFlag::Enabled);

    let serial_num = note_rng.draw_word();

    // The expected output note, built host-side from the standalone note package. The recipient
    // computed by the constructor inside the VM must commit to the same note script root for the
    // created note to match this expectation.
    let transfer_asset_core: Asset = transfer_asset.into();
    let bob_note = NoteBuilder::new(alice_id, &mut note_rng)
        .serial_number(serial_num)
        .package((*note_package).clone())
        .note_storage(to_core_felts(&bob_id))
        .unwrap()
        .add_assets([transfer_asset_core])
        .tag(0)
        .build()
        .unwrap();

    // The transaction script calls into the note package, whose procedures are linked as
    // external MAST references; merge the note package's forest into the script.
    let tx_script =
        transaction_script_from_package_with_deps(&tx_script_package, &[note_package.as_ref()]);

    // Prepare commitment data.
    // This must match the input layout expected by `examples/p2id-tx-script`.
    let mut commitment_input: Vec<Felt> = vec![
        // The output note tag
        Felt::ZERO,
        // The output note type
        Felt::from(NoteType::Public),
        // The target account id
        bob_id.prefix().as_felt(),
        bob_id.suffix(),
    ];
    let serial_num_felts: [Felt; 4] = serial_num.into();
    commitment_input.extend(serial_num_felts);
    commitment_input.extend(transfer_asset_core.as_elements());
    assert_eq!(commitment_input.len() % 4, 0, "commitment input needs to be word-aligned");

    let commitment_key: miden_client::Word =
        miden_core::crypto::hash::Poseidon2::hash_elements(&commitment_input);

    let faucet_inputs = chain.get_foreign_account_inputs(faucet_id).unwrap();
    let create_tx_context_builder = chain
        .build_tx_context(alice_id, &[], &[])
        .unwrap()
        .foreign_accounts(vec![faucet_inputs])
        .tx_script(tx_script)
        .tx_script_args(commitment_key)
        .extend_advice_map([(commitment_key, commitment_input)])
        .extend_expected_output_notes(vec![RawOutputNote::Full(bob_note.clone())]);
    let tx_measurements = execute_tx(&mut chain, create_tx_context_builder);
    expect!["12252"].assert_eq(tx_script_processing_cycles(&tx_measurements));

    eprintln!("\n=== Step 4: Bob consumes the note created by the constructor ===");
    let faucet_inputs = chain.get_foreign_account_inputs(faucet_id).unwrap();
    let consume_tx_context_builder = chain
        .build_tx_context(bob_id, &[bob_note.id()], &[])
        .unwrap()
        .foreign_accounts(vec![faucet_inputs]);
    let tx_measurements = execute_tx(&mut chain, consume_tx_context_builder);
    expect!["9311"].assert_eq(single_note_cycles(&tx_measurements));

    eprintln!("\n=== Checking Bob's account has the transferred asset ===");
    let bob_account = chain.committed_account(bob_id).unwrap();
    assert_account_has_fungible_asset(bob_account, faucet_id, transfer_amount);

    eprintln!("\n=== Checking Alice's account reflects the new token amount ===");
    let alice_account = chain.committed_account(alice_id).unwrap();
    assert_account_has_fungible_asset(alice_account, faucet_id, mint_amount - transfer_amount);
}
