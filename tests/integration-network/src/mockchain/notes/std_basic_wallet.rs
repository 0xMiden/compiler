//! Rust notes and transaction scripts driving accounts made of the standard (MASM) components.

use miden_client::{
    asset::{Asset, FungibleAsset},
    transaction::RawOutputNote,
};
use miden_protocol::{account::auth::AuthScheme, crypto::rand::RandomCoin};
use miden_standards::testing::note::NoteBuilder;
use miden_testing::{Auth, MockChain};
use midenc_expect_test::expect;

use super::super::support::{
    assert_account_has_fungible_asset, build_asset_transfer_tx, build_send_notes_script,
    compile_rust_package, execute_tx, execute_tx_measurements, note_script_root, prologue_cycles,
    single_note_cycles, to_core_felts, tx_script_processing_cycles,
};

/// Transfers an asset between two accounts made only of the standard components, with a Rust
/// note and a Rust transaction script that call the standard basic wallet.
///
/// Alice and Bob are standard basic wallets (`miden_standards::account::wallets::BasicWallet`)
/// authenticated by `Auth::BasicAuth`, which in `miden-testing` is the standard `AuthSingleSig`
/// component. The Rust code links the wallet through its package alone
/// (`examples/std-wallet-p2id-note` and `examples/std-wallet-tx-script`).
///
/// Flow:
/// - The faucet mints to Alice through the Rust P2ID note; Alice consumes it
/// - Alice sends part of it to Bob through the Rust tx script; Bob consumes the P2ID note
/// - The vault balances after each consumption check the asset crossing each wallet call
#[test]
pub fn std_basic_wallet_p2id_transfers_asset_with_rust_note_and_tx_script() {
    // Compile the contracts first (before creating any runtime)
    let note_package = compile_rust_package("../../examples/std-wallet-p2id-note", true);
    let tx_script_package = compile_rust_package("../../examples/std-wallet-tx-script", true);

    let auth = || Auth::BasicAuth {
        auth_scheme: AuthScheme::Falcon512Poseidon2,
    };
    let mut builder = MockChain::builder();
    let max_supply = 1_000_000_000u64;
    let faucet_account =
        builder.add_existing_basic_faucet(auth(), "TEST", max_supply, None).unwrap();
    let faucet_id = faucet_account.id();
    let alice_id = builder.add_existing_wallet(auth()).unwrap().id();
    let bob_id = builder.add_existing_wallet(auth()).unwrap().id();

    let mut chain = builder.build().unwrap();
    chain.prove_next_block().unwrap();
    chain.prove_next_block().unwrap();

    eprintln!("\n=== Step 1: Minting tokens from faucet to Alice ===");
    let mint_amount = 100_000u64;
    let mint_asset = FungibleAsset::new(faucet_id, mint_amount).unwrap();

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
    let mock_tx = chain
        .build_transaction(faucet_id)
        .send_notes_script(&mint_tx_script)
        .expected_output_notes(vec![RawOutputNote::Full(p2id_note_mint.clone())])
        .build()
        .unwrap();
    execute_tx(&mut chain, mock_tx);

    eprintln!("\n=== Step 2: Alice consumes mint note ===");
    let faucet_inputs = chain.get_foreign_account_inputs(faucet_id).unwrap();
    let mock_tx = chain
        .build_transaction(alice_id)
        .authenticated_input_note(p2id_note_mint.id())
        .foreign_accounts(vec![faucet_inputs])
        .build()
        .unwrap();
    let tx_measurements = execute_tx_measurements(&mut chain, mock_tx);
    expect!["3882"].assert_eq(prologue_cycles(&tx_measurements));
    expect!["3936"].assert_eq(single_note_cycles(&tx_measurements));

    let alice_account = chain.committed_account(alice_id).unwrap();
    assert_account_has_fungible_asset(alice_account, faucet_id, mint_amount);

    eprintln!("\n=== Step 3: Alice creates p2id note for Bob (Rust tx script) ===");
    let transfer_amount = 10_000u64;
    let transfer_asset = FungibleAsset::new(faucet_id, transfer_amount).unwrap();

    let (mock_tx, bob_note) = build_asset_transfer_tx(
        &chain,
        alice_id,
        bob_id,
        transfer_asset,
        note_package,
        tx_script_package,
        &mut note_rng,
    );
    let tx_measurements = execute_tx_measurements(&mut chain, mock_tx);
    expect!["4796"].assert_eq(tx_script_processing_cycles(&tx_measurements));

    eprintln!("\n=== Step 4: Bob consumes p2id note ===");
    let faucet_inputs = chain.get_foreign_account_inputs(faucet_id).unwrap();
    let mock_tx = chain
        .build_transaction(bob_id)
        .authenticated_input_note(bob_note.id())
        .foreign_accounts(vec![faucet_inputs])
        .build()
        .unwrap();
    let tx_measurements = execute_tx_measurements(&mut chain, mock_tx);
    expect!["3936"].assert_eq(single_note_cycles(&tx_measurements));

    let bob_account = chain.committed_account(bob_id).unwrap();
    assert_account_has_fungible_asset(bob_account, faucet_id, transfer_amount);

    let alice_account = chain.committed_account(alice_id).unwrap();
    assert_account_has_fungible_asset(alice_account, faucet_id, mint_amount - transfer_amount);
}
