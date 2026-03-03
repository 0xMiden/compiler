//! Basic wallet test module

use miden_client::{
    account::component::BasicWallet,
    asset::FungibleAsset,
    crypto::RpoRandomCoin,
    testing::{Auth, MockChain, NoteBuilder},
    transaction::OutputNote,
};
use miden_core::Felt;
use miden_protocol::{account::AccountComponent, asset::Asset};

use super::helpers::{
    assert_account_has_fungible_asset, build_asset_transfer_tx, build_send_notes_script,
    execute_tx, to_core_felts,
};
use crate::mockchain::helpers::{CustomComponentBuilder, PackageFromProject};

/// Tests the basic-wallet contract deployment and p2id note consumption workflow on a mock chain.
#[test]
pub fn test_basic_wallet_p2id() {
    // Compile the contracts first (before creating any runtime)
    let wallet_package =
        CustomComponentBuilder::with_package("../../examples/basic-wallet").build();
    let note_package = NoteBuilder::build_project("../../examples/p2id-note");
    let tx_script_package = NoteBuilder::build_project("../../examples/basic-wallet-tx-script");

    let mut builder = MockChain::builder();
    let max_supply = 1_000_000_000u64;
    let faucet_account = builder
        .add_existing_basic_faucet(Auth::BasicAuth, "TEST", max_supply, None)
        .unwrap();
    let faucet_id = faucet_account.id();

    let alice_account = builder
        .add_existing_account_from_components(Auth::BasicAuth, [wallet_package.clone().into()])
        .unwrap();
    let alice_id = alice_account.id();

    let bob_account = builder
        .add_existing_account_from_components(Auth::BasicAuth, [wallet_package.into()])
        .unwrap();
    let bob_id = bob_account.id();

    let mut chain = builder.build().unwrap();
    chain.prove_next_block().unwrap();
    chain.prove_next_block().unwrap();

    eprintln!("\n=== Step 1: Minting tokens from faucet to Alice ===");
    let mint_amount = 100_000u64; // 100,000 tokens
    let mint_asset = FungibleAsset::new(faucet_id, mint_amount).unwrap();

    let mut note_rng = RpoRandomCoin::new(note_package.unwrap_program().hash());
    let p2id_note_mint = NoteBuilder::new(faucet_id, &mut note_rng)
        .package((*note_package).clone())
        .add_assets([Asset::from(mint_asset)])
        .note_inputs(to_core_felts(&alice_id))
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
        .extend_expected_output_notes(vec![OutputNote::Full(p2id_note_mint.clone())]);
    execute_tx(&mut chain, mint_tx_context_builder);

    eprintln!("\n=== Step 2: Alice consumes mint note ===");
    let consume_tx_context_builder =
        chain.build_tx_context(alice_id, &[p2id_note_mint.id()], &[]).unwrap();
    execute_tx(&mut chain, consume_tx_context_builder);

    eprintln!("\n=== Checking Alice's account has the minted asset ===");
    let alice_account = chain.committed_account(alice_id).unwrap();
    assert_account_has_fungible_asset(alice_account, faucet_id, mint_amount);

    eprintln!("\n=== Step 3: Alice creates p2id note for Bob (custom tx script) ===");
    let transfer_amount = 10_000u64; // 10,000 tokens
    let transfer_asset = FungibleAsset::new(faucet_id, transfer_amount).unwrap();

    let (alice_tx_context_builder, bob_note) = build_asset_transfer_tx(
        &chain,
        alice_id,
        bob_id,
        transfer_asset,
        note_package,
        tx_script_package,
        &mut note_rng,
    );
    execute_tx(&mut chain, alice_tx_context_builder);

    eprintln!("\n=== Step 4: Bob consumes p2id note ===");
    let consume_tx_context_builder = chain.build_tx_context(bob_id, &[bob_note.id()], &[]).unwrap();
    execute_tx(&mut chain, consume_tx_context_builder);

    eprintln!("\n=== Checking Bob's account has the transferred asset ===");
    let bob_account = chain.committed_account(bob_id).unwrap();
    assert_account_has_fungible_asset(bob_account, faucet_id, transfer_amount);

    eprintln!("\n=== Checking Alice's account reflects the new token amount ===");
    let alice_account = chain.committed_account(alice_id).unwrap();
    assert_account_has_fungible_asset(alice_account, faucet_id, mint_amount - transfer_amount);
}

/// Tests the basic-wallet contract deployment and p2ide note consumption workflow on a mock chain.
///
/// Flow:
/// - Create fungible faucet and mint tokens to Alice
/// - Alice creates a p2ide note for Bob (with timelock=0, reclaim=0)
/// - Bob consumes the p2ide note and receives the assets
#[test]
pub fn test_basic_wallet_p2ide() {
    // Compile the contracts first (before creating any runtime)
    let wallet_package =
        CustomComponentBuilder::with_package("../../examples/basic-wallet").build();
    let p2id_note_package = NoteBuilder::build_project("../../examples/p2id-note");
    let p2ide_note_package = NoteBuilder::build_project("../../examples/p2ide-note");

    let mut builder = MockChain::builder();
    let max_supply = 1_000_000_000u64;
    let faucet_account = builder
        .add_existing_basic_faucet(Auth::BasicAuth, "TEST", max_supply, None)
        .unwrap();
    let faucet_id = faucet_account.id();

    let alice_account = builder
        .add_existing_account_from_components(
            Auth::BasicAuth,
            [wallet_package.clone().into(), BasicWallet.into()],
        )
        .unwrap();
    let alice_id = alice_account.id();

    let bob_account = builder
        .add_existing_account_from_components(
            Auth::BasicAuth,
            [AccountComponent::from(wallet_package)],
        )
        .unwrap();
    let bob_id = bob_account.id();

    let mut chain = builder.build().unwrap();
    chain.prove_next_block().unwrap();
    chain.prove_next_block().unwrap();

    // Step 1: Mint assets from faucet to Alice using p2id note
    let mint_amount = 100_000u64;
    let mint_asset = FungibleAsset::new(faucet_id, mint_amount).unwrap();

    let p2id_rng = RpoRandomCoin::new(p2id_note_package.unwrap_program().hash());
    let p2id_note_mint = NoteBuilder::new(faucet_id, p2id_rng)
        .package((*p2id_note_package).clone())
        .add_assets([Asset::from(mint_asset)])
        .note_inputs(to_core_felts(&alice_id))
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
        .extend_expected_output_notes(vec![OutputNote::Full(p2id_note_mint.clone())]);
    execute_tx(&mut chain, mint_tx_context_builder);

    // Step 2: Alice consumes the p2id note
    let consume_tx_context_builder =
        chain.build_tx_context(alice_id, &[p2id_note_mint.id()], &[]).unwrap();
    execute_tx(&mut chain, consume_tx_context_builder);

    let alice_account = chain.committed_account(alice_id).unwrap();
    assert_account_has_fungible_asset(alice_account, faucet_id, mint_amount);

    // Step 3: Alice creates p2ide note for Bob
    let transfer_amount = 10_000u64;
    let transfer_asset = FungibleAsset::new(faucet_id, transfer_amount).unwrap();
    let timelock_height = Felt::new(0);
    let reclaim_height = Felt::new(0);

    let p2ide_rng = RpoRandomCoin::new(p2ide_note_package.unwrap_program().hash());
    let p2ide_note = NoteBuilder::new(alice_id, p2ide_rng)
        .package((*p2ide_note_package).clone())
        .add_assets([Asset::from(transfer_asset)])
        .note_inputs({
            let mut inputs = to_core_felts(&bob_id);
            inputs.extend([timelock_height, reclaim_height]);
            inputs
        })
        .unwrap()
        .build()
        .unwrap();

    let alice_account = chain.committed_account(alice_id).unwrap().clone();
    let transfer_tx_script =
        build_send_notes_script(&alice_account, std::slice::from_ref(&p2ide_note));
    let transfer_tx_context_builder = chain
        .build_tx_context(alice_id, &[], &[])
        .unwrap()
        .tx_script(transfer_tx_script)
        .extend_expected_output_notes(vec![OutputNote::Full(p2ide_note.clone())]);
    execute_tx(&mut chain, transfer_tx_context_builder);

    // Step 4: Bob consumes the p2ide note
    let consume_tx_context_builder =
        chain.build_tx_context(bob_id, &[p2ide_note.id()], &[]).unwrap();
    execute_tx(&mut chain, consume_tx_context_builder);

    // Step 5: verify balances
    let bob_account = chain.committed_account(bob_id).unwrap();
    assert_account_has_fungible_asset(bob_account, faucet_id, transfer_amount);

    let alice_account = chain.committed_account(alice_id).unwrap();
    assert_account_has_fungible_asset(alice_account, faucet_id, mint_amount - transfer_amount);
}

/// Tests the p2ide note reclaim functionality.
///
/// Flow:
/// - Create fungible faucet and mint tokens to Alice
/// - Alice creates a p2ide note intended for Bob (with reclaim enabled)
/// - Alice reclaims the note herself (exercises the reclaim branch)
/// - Verify Alice has her original balance back
#[test]
pub fn test_basic_wallet_p2ide_reclaim() {
    // Compile the contracts first (before creating any runtime)
    let wallet_package =
        CustomComponentBuilder::with_package("../../examples/basic-wallet").build();
    let p2id_note_package = NoteBuilder::build_project("../../examples/p2id-note");
    let p2ide_note_package = NoteBuilder::build_project("../../examples/p2ide-note");

    let mut builder = MockChain::builder();
    let max_supply = 1_000_000_000u64;
    let faucet_account = builder
        .add_existing_basic_faucet(Auth::BasicAuth, "TEST", max_supply, None)
        .unwrap();
    let faucet_id = faucet_account.id();

    let alice_account = builder
        .add_existing_account_from_components(
            Auth::BasicAuth,
            [wallet_package.clone().into(), BasicWallet.into()],
        )
        .unwrap();
    let alice_id = alice_account.id();

    let bob_account = builder
        .add_existing_account_from_components(Auth::BasicAuth, [wallet_package.into()])
        .unwrap();
    let bob_id = bob_account.id();

    let mut chain = builder.build().unwrap();
    chain.prove_next_block().unwrap();
    chain.prove_next_block().unwrap();

    // Step 1: Mint assets from faucet to Alice using p2id note
    let mint_amount = 100_000u64;
    let mint_asset = FungibleAsset::new(faucet_id, mint_amount).unwrap();

    let p2id_rng = RpoRandomCoin::new(p2id_note_package.unwrap_program().hash());
    let p2id_note_mint = NoteBuilder::new(faucet_id, p2id_rng)
        .package((*p2id_note_package).clone())
        .add_assets([Asset::from(mint_asset)])
        .note_inputs(to_core_felts(&alice_id))
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
        .extend_expected_output_notes(vec![OutputNote::Full(p2id_note_mint.clone())]);
    execute_tx(&mut chain, mint_tx_context_builder);

    // Step 2: Alice consumes the p2id note
    let consume_tx_context_builder =
        chain.build_tx_context(alice_id, &[p2id_note_mint.id()], &[]).unwrap();
    execute_tx(&mut chain, consume_tx_context_builder);

    let alice_account = chain.committed_account(alice_id).unwrap();
    assert_account_has_fungible_asset(alice_account, faucet_id, mint_amount);

    // Step 3: Alice creates p2ide note for Bob with reclaim enabled
    let transfer_amount = 10_000u64;
    let transfer_asset = FungibleAsset::new(faucet_id, transfer_amount).unwrap();
    let timelock_height = Felt::new(0);
    let reclaim_height = Felt::new(1000);

    let p2ide_rng = RpoRandomCoin::new(p2ide_note_package.unwrap_program().hash());
    let p2ide_note = NoteBuilder::new(alice_id, p2ide_rng)
        .package((*p2ide_note_package).clone())
        .add_assets([Asset::from(transfer_asset)])
        .note_inputs({
            let mut inputs = to_core_felts(&bob_id);
            inputs.extend([timelock_height, reclaim_height]);
            inputs
        })
        .unwrap()
        .build()
        .unwrap();

    let alice_account = chain.committed_account(alice_id).unwrap().clone();
    let transfer_tx_script =
        build_send_notes_script(&alice_account, std::slice::from_ref(&p2ide_note));
    let transfer_tx_context_builder = chain
        .build_tx_context(alice_id, &[], &[])
        .unwrap()
        .tx_script(transfer_tx_script)
        .extend_expected_output_notes(vec![OutputNote::Full(p2ide_note.clone())]);
    execute_tx(&mut chain, transfer_tx_context_builder);

    // Step 4: Alice reclaims the note (exercises the reclaim branch)
    let reclaim_tx_context_builder =
        chain.build_tx_context(alice_id, &[p2ide_note.id()], &[]).unwrap();
    execute_tx(&mut chain, reclaim_tx_context_builder);

    // Step 5: verify Alice has her original amount back
    let alice_account = chain.committed_account(alice_id).unwrap();
    assert_account_has_fungible_asset(alice_account, faucet_id, mint_amount);

    // Ensure Bob did not receive the asset.
    let bob_account = chain.committed_account(bob_id).unwrap();
    let bob_found = bob_account.vault().assets().find(|asset| {
        matches!(
            asset,
            miden_protocol::asset::Asset::Fungible(fungible_asset)
                if fungible_asset.faucet_id() == faucet_id
        )
    });
    assert!(bob_found.is_none(), "Bob unexpectedly received reclaimed assets");
}
