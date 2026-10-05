//! Basic wallet test module

use miden_client::{
    account::{
        AccountComponent, AccountId,
        component::{BasicWallet, InitStorageData},
    },
    asset::{Asset, FungibleAsset},
    transaction::RawOutputNote,
};
use miden_core::Felt;
use miden_protocol::{account::auth::AuthScheme, crypto::rand::RandomCoin};
use miden_standards::testing::note::NoteBuilder;
use miden_testing::{Auth, MockChain};
use midenc_expect_test::expect;

use super::super::support::{
    assert_account_has_fungible_asset, build_asset_transfer_tx, build_send_notes_script,
    compile_rust_package, execute_tx, execute_tx_measurements, note_script_root, prologue_cycles,
    single_note_cycles, to_core_felts, tx_script_processing_cycles,
};
/// Converts the P2IDE note payload into protocol storage order for the basic-wallet tests.
fn to_p2ide_storage_felts(
    target: &AccountId,
    reclaim_height: Felt,
    timelock_height: Felt,
) -> Vec<Felt> {
    vec![target.suffix(), target.prefix().as_felt(), reclaim_height, timelock_height]
}

/// Tests the basic-wallet contract deployment and p2id note consumption workflow on a mock chain.
#[test]
pub fn basic_wallet_p2id_transfers_asset_with_custom_tx_script() {
    // Compile the contracts first (before creating any runtime)
    let wallet_package = compile_rust_package("../../examples/basic-wallet", true);
    let note_package = compile_rust_package("../../examples/p2id-note", true);
    let tx_script_package = compile_rust_package("../../examples/basic-wallet-tx-script", true);

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
    let mint_amount = 100_000u64; // 100,000 tokens
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
    // 5018 before codegen lowered a cast that changes only a value's type by renaming its
    // operand where it stands (14 cycles fewer) and a peephole deleted adjacent stack operations
    // that undo each other, such as `swap.1 swap.1` (50 fewer). 24 fewer (4954 before) with no
    // `u32assert` on a constant-address 32-bit store: one fewer per 32-bit global initializer, in
    // the `init` that every call into a component runs. The P2ID note's `init` stores four such
    // globals (14 cycles: 12 for the assertions, 2 for padding `noop`s), and the wallet's, run
    // once for `receive_asset`, three (10 cycles: 9, and 1 for an op batch its stores no longer
    // fill).
    expect!["4930"].assert_eq(single_note_cycles(&tx_measurements));

    eprintln!("\n=== Checking Alice's account has the minted asset ===");
    let alice_account = chain.committed_account(alice_id).unwrap();
    assert_account_has_fungible_asset(alice_account, faucet_id, mint_amount);

    eprintln!("\n=== Step 3: Alice creates p2id note for Bob (custom tx script) ===");
    let transfer_amount = 10_000u64; // 10,000 tokens
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
    // 6439 before the SDK's bindings were generated, 6441 after `miden-stdlib-sys` was (Tasks 6
    // and 6b of the generator plan; this suite was not re-run then). 189 cycles more when
    // `miden-base-sys` began calling the protocol through its generated bindings, which take the
    // manifest's integer types, and 2 more (6630 before) since `mem::pipe_preimage_to_memory`,
    // which the tx script reaches through `adv_load_preimage`, resolves from the core manifest
    // instead of the deleted transitional table: the casts that give the stub's `i32` carriers
    // the manifest's element-space pointer type cost the stub a redundant `swap.1 swap.1`.
    //
    // 130 cycles more (6632 before) since `Tag`, `NoteIdx` and `NoteType` wrap the manifest's
    // integers. The wallet's felt range checks went: its `create_note` keeps only the check that
    // the `NoteType` is private or public, its `move_asset_to_note` has none, the frontend masks
    // the narrow values to their declared types as before, and the component now masks the `u8`
    // and `u16` it lifts from its arguments (new with the integer newtypes). But the tx script
    // now checks the `Tag` and the `NoteType` it used to read unchecked (see
    // `basic_wallet_and_p2id` in the protocol tests): each is split to its canonical `u64`,
    // compared with the bound through the core library's `u64::gt` and kept in a 64-bit local
    // until the call, which costs more than the felt comparisons the wallet made.
    //
    // 77 cycles fewer (6762 before) since codegen lowers a cast that changes only a value's type
    // by renaming its operand where it stands (14 cycles, against the 2 the stub's redundant
    // `swap.1 swap.1` above cost), and a peephole deletes adjacent stack operations that undo
    // each other (63 cycles).
    //
    // 25 cycles fewer (6685 before) with no `u32assert` on a constant-address 32-bit store: one
    // fewer per 32-bit global initializer. The tx script's `init` stores two such globals (5
    // cycles: 6 for the assertions, less a padding `noop` its shorter block needs), and the
    // wallet's `init`, which runs on each of the script's two calls into it, three (10 cycles
    // each; see Step 2).
    expect!["6660"].assert_eq(tx_script_processing_cycles(&tx_measurements));

    eprintln!("\n=== Step 4: Bob consumes p2id note ===");
    let faucet_inputs = chain.get_foreign_account_inputs(faucet_id).unwrap();
    let mock_tx = chain
        .build_transaction(bob_id)
        .authenticated_input_note(bob_note.id())
        .foreign_accounts(vec![faucet_inputs])
        .build()
        .unwrap();
    let tx_measurements = execute_tx_measurements(&mut chain, mock_tx);
    // 5018 before casts that change only a type became renames and the stack peephole was added,
    // 4954 before the `u32assert` on a constant-address 32-bit store went (see Step 2).
    expect!["4930"].assert_eq(single_note_cycles(&tx_measurements));

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
pub fn basic_wallet_p2ide_allows_recipient_claim() {
    // Compile the contracts first (before creating any runtime)
    let wallet_package = compile_rust_package("../../examples/basic-wallet", true);
    let p2id_note_package = compile_rust_package("../../examples/p2id-note", true);
    let p2ide_note_package = compile_rust_package("../../examples/p2ide-note", true);

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
            [wallet_component.clone(), BasicWallet.into()],
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

    // Step 1: Mint assets from faucet to Alice using p2id note
    let mint_amount = 100_000u64;
    let mint_asset = FungibleAsset::new(faucet_id, mint_amount).unwrap();

    let p2id_rng = RandomCoin::new(note_script_root(p2id_note_package.as_ref()));
    let p2id_note_mint = NoteBuilder::new(faucet_id, p2id_rng)
        .package((*p2id_note_package).clone())
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

    // Step 2: Alice consumes the p2id note
    let faucet_inputs = chain.get_foreign_account_inputs(faucet_id).unwrap();
    let mock_tx = chain
        .build_transaction(alice_id)
        .authenticated_input_note(p2id_note_mint.id())
        .foreign_accounts(vec![faucet_inputs])
        .build()
        .unwrap();
    execute_tx(&mut chain, mock_tx);

    let alice_account = chain.committed_account(alice_id).unwrap();
    assert_account_has_fungible_asset(alice_account, faucet_id, mint_amount);

    // Step 3: Alice creates p2ide note for Bob
    let transfer_amount = 10_000u64;
    let transfer_asset = FungibleAsset::new(faucet_id, transfer_amount).unwrap();
    let timelock_height = Felt::ZERO;
    let reclaim_height = Felt::ZERO;

    let p2ide_rng = RandomCoin::new(note_script_root(p2ide_note_package.as_ref()));
    let p2ide_note = NoteBuilder::new(alice_id, p2ide_rng)
        .package((*p2ide_note_package).clone())
        .add_assets([Asset::from(transfer_asset)])
        .note_storage(to_p2ide_storage_felts(&bob_id, reclaim_height, timelock_height))
        .unwrap()
        .build()
        .unwrap();

    let alice_account = chain.committed_account(alice_id).unwrap().clone();
    let faucet_inputs = chain.get_foreign_account_inputs(faucet_id).unwrap();
    let transfer_tx_script =
        build_send_notes_script(&alice_account, std::slice::from_ref(&p2ide_note));
    let mock_tx = chain
        .build_transaction(alice_id)
        .foreign_accounts(vec![faucet_inputs])
        .send_notes_script(&transfer_tx_script)
        .expected_output_notes(vec![RawOutputNote::Full(p2ide_note.clone())])
        .build()
        .unwrap();
    execute_tx(&mut chain, mock_tx);

    // Step 4: Bob consumes the p2ide note
    let faucet_inputs = chain.get_foreign_account_inputs(faucet_id).unwrap();
    let mock_tx = chain
        .build_transaction(bob_id)
        .authenticated_input_note(p2ide_note.id())
        .foreign_accounts(vec![faucet_inputs])
        .build()
        .unwrap();
    let tx_measurements = execute_tx_measurements(&mut chain, mock_tx);
    // 5438 before codegen lowered a cast that changes only a value's type by renaming its
    // operand where it stands (16 cycles fewer) and a peephole deleted adjacent stack operations
    // that undo each other, such as `swap.1 swap.1` (53 fewer). 24 fewer (5369 before) with no
    // `u32assert` on a constant-address 32-bit store: one fewer per 32-bit global initializer.
    // The P2IDE note's `init` stores four such globals and the wallet's, run once for
    // `receive_asset`, three: 14 and 10 cycles, as for the P2ID note in
    // `basic_wallet_p2id_transfers_asset_with_custom_tx_script`.
    expect!["5345"].assert_eq(single_note_cycles(&tx_measurements));

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
pub fn basic_wallet_p2ide_allows_sender_reclaim() {
    // Compile the contracts first (before creating any runtime)
    let wallet_package = compile_rust_package("../../examples/basic-wallet", true);
    let p2id_note_package = compile_rust_package("../../examples/p2id-note", true);
    let p2ide_note_package = compile_rust_package("../../examples/p2ide-note", true);

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

    let wallet_component = AccountComponent::from_package(
        wallet_package.as_ref().clone(),
        &InitStorageData::default(),
    )
    .unwrap();

    let alice_account = builder
        .add_existing_account_from_components(
            Auth::BasicAuth {
                auth_scheme: AuthScheme::Falcon512Poseidon2,
            },
            [wallet_component.clone(), BasicWallet.into()],
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

    // Step 1: Mint assets from faucet to Alice using p2id note
    let mint_amount = 100_000u64;
    let mint_asset = FungibleAsset::new(faucet_id, mint_amount).unwrap();

    let p2id_rng = RandomCoin::new(note_script_root(p2id_note_package.as_ref()));
    let p2id_note_mint = NoteBuilder::new(faucet_id, p2id_rng)
        .package((*p2id_note_package).clone())
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

    // Step 2: Alice consumes the p2id note
    let faucet_inputs = chain.get_foreign_account_inputs(faucet_id).unwrap();
    let mock_tx = chain
        .build_transaction(alice_id)
        .authenticated_input_note(p2id_note_mint.id())
        .foreign_accounts(vec![faucet_inputs])
        .build()
        .unwrap();
    execute_tx(&mut chain, mock_tx);

    let alice_account = chain.committed_account(alice_id).unwrap();
    assert_account_has_fungible_asset(alice_account, faucet_id, mint_amount);

    // Step 3: Alice creates p2ide note for Bob with reclaim enabled
    let transfer_amount = 10_000u64;
    let transfer_asset = FungibleAsset::new(faucet_id, transfer_amount).unwrap();
    let timelock_height = Felt::ZERO;
    let reclaim_height = Felt::ONE;

    let p2ide_rng = RandomCoin::new(note_script_root(p2ide_note_package.as_ref()));
    let p2ide_note = NoteBuilder::new(alice_id, p2ide_rng)
        .package((*p2ide_note_package).clone())
        .add_assets([Asset::from(transfer_asset)])
        .note_storage(to_p2ide_storage_felts(&bob_id, reclaim_height, timelock_height))
        .unwrap()
        .build()
        .unwrap();

    let alice_account = chain.committed_account(alice_id).unwrap().clone();
    let faucet_inputs = chain.get_foreign_account_inputs(faucet_id).unwrap();
    let transfer_tx_script =
        build_send_notes_script(&alice_account, std::slice::from_ref(&p2ide_note));
    let mock_tx = chain
        .build_transaction(alice_id)
        .foreign_accounts(vec![faucet_inputs])
        .send_notes_script(&transfer_tx_script)
        .expected_output_notes(vec![RawOutputNote::Full(p2ide_note.clone())])
        .build()
        .unwrap();
    execute_tx(&mut chain, mock_tx);

    // Step 4: Alice reclaims the note (exercises the reclaim branch)
    let faucet_inputs = chain.get_foreign_account_inputs(faucet_id).unwrap();
    let mock_tx = chain
        .build_transaction(alice_id)
        .authenticated_input_note(p2ide_note.id())
        .foreign_accounts(vec![faucet_inputs])
        .build()
        .unwrap();
    let tx_measurements = execute_tx_measurements(&mut chain, mock_tx);
    // 6002 before codegen lowered a cast that changes only a value's type by renaming its
    // operand where it stands (20 cycles fewer) and a peephole deleted adjacent stack operations
    // that undo each other, such as `swap.1 swap.1` (57 fewer). 24 fewer (5925 before) with no
    // `u32assert` on a constant-address 32-bit store, as in
    // `basic_wallet_p2ide_allows_recipient_claim`.
    expect!["5901"].assert_eq(single_note_cycles(&tx_measurements));

    // Step 5: verify Alice has her original amount back
    let alice_account = chain.committed_account(alice_id).unwrap();
    assert_account_has_fungible_asset(alice_account, faucet_id, mint_amount);

    // Ensure Bob did not receive the asset.
    let bob_account = chain.committed_account(bob_id).unwrap();
    let bob_found = bob_account
        .vault()
        .assets()
        .find(|asset| asset.is_fungible() && asset.faucet_id() == faucet_id);
    assert!(bob_found.is_none(), "Bob unexpectedly received reclaimed assets");
}
