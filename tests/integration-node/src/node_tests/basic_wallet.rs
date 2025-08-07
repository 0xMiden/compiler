//! Basic wallet test module

use miden_client::{
    asset::{FungibleAsset, TokenSymbol},
    crypto::{FeltRng, RpoRandomCoin},
    note::{
        Note, NoteAssets, NoteExecutionHint, NoteInputs, NoteMetadata, NoteRecipient, NoteTag,
        NoteType,
    },
    transaction::{OutputNote, TransactionRequestBuilder, TransactionScript},
};
use miden_core::{utils::Serializable, Felt, FieldElement};
use miden_objects::{account::Account, asset::Asset};

use super::helpers::*;
use crate::local_node::ensure_shared_node;

/// Tests the basic-wallet contract deployment and p2id note consumption workflow on a local node.
#[test]
pub fn test_basic_wallet_p2id_local() {
    // Compile the contracts first (before creating any runtime)
    let wallet_package = compile_rust_package("../../examples/basic-wallet", true);
    let note_package = compile_rust_package("../../examples/p2id-note", true);
    let tx_script_package = compile_rust_package("../../examples/basic-wallet-tx-script", true);

    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        // Create temp directory and get node handle
        let temp_dir = temp_dir::TempDir::with_prefix("test_basic_wallet_p2id_local_")
            .expect("Failed to create temp directory");
        let node_handle = ensure_shared_node().await.expect("Failed to get shared node");

        // Initialize test infrastructure
        let TestSetup {
            mut client,
            keystore,
        } = setup_test_infrastructure(&temp_dir, &node_handle)
            .await
            .expect("Failed to setup test infrastructure");

        // Write wallet package to disk for potential future use
        let wallet_package_path = temp_dir.path().join("basic_wallet.masp");
        std::fs::write(&wallet_package_path, wallet_package.to_bytes())
            .expect("Failed to write wallet");

        // Create a fungible faucet account
        let token_symbol = TokenSymbol::new("TEST").unwrap();
        let decimals = 8u8;
        let max_supply = Felt::new(1_000_000_000); // 1 billion tokens

        let faucet_account = create_fungible_faucet_account(
            &mut client,
            keystore.clone(),
            token_symbol,
            decimals,
            max_supply,
        )
        .await
        .unwrap();

        eprintln!("Faucet account ID: {:?}", faucet_account.id().to_hex());

        // Create Alice's account with basic-wallet component
        let alice_config = AccountCreationConfig {
            with_basic_wallet: false,
            ..Default::default()
        };
        let alice_account = create_account_with_component(
            &mut client,
            keystore.clone(),
            wallet_package.clone(),
            alice_config,
        )
        .await
        .unwrap();
        eprintln!("Alice account ID: {:?}", alice_account.id().to_hex());

        eprintln!("\n=== Step 1: Minting tokens from faucet to Alice ===");

        let mint_amount = 100_000u64; // 100,000 tokens
        let fungible_asset = FungibleAsset::new(faucet_account.id(), mint_amount).unwrap();

        // Create the p2id note from faucet to Alice
        let p2id_note_mint = create_note_from_package(
            &mut client,
            note_package.clone(),
            faucet_account.id(),
            NoteCreationConfig {
                assets: NoteAssets::new(vec![fungible_asset.into()]).unwrap(),
                inputs: vec![alice_account.id().prefix().as_felt(), alice_account.id().suffix()],
                ..Default::default()
            },
        );
        eprintln!("P2ID mint note hash: {:?}", p2id_note_mint.id().to_hex());

        let mint_request = TransactionRequestBuilder::new()
            .own_output_notes(vec![OutputNote::Full(p2id_note_mint.clone())])
            .build()
            .unwrap();

        let mint_tx_result =
            client.new_transaction(faucet_account.id(), mint_request).await.unwrap();
        let mint_tx_id = mint_tx_result.executed_transaction().id();
        eprintln!("Created mint transaction. Tx ID: {mint_tx_id:?}");

        client.submit_transaction(mint_tx_result).await.unwrap();
        eprintln!("Submitted mint transaction. Tx ID: {mint_tx_id:?}");

        eprintln!("\n=== Step 2: Alice attempts to consume mint note ===");

        let consume_request = TransactionRequestBuilder::new()
            .unauthenticated_input_notes([(p2id_note_mint, None)])
            .build()
            .unwrap();

        let consume_tx = client
            .new_transaction(alice_account.id(), consume_request)
            .await
            .map_err(|e| format!("{e:?}"))
            .unwrap();

        client.submit_transaction(consume_tx).await.unwrap();

        eprintln!("\n=== Checking Alice's account has the minted asset ===");

        let alice_account_record = client
            .get_account(alice_account.id())
            .await
            .unwrap()
            .expect("Alice's account should exist");
        let alice_account_state: Account = alice_account_record.into();

        assert_eq!(
            alice_account_state.vault().assets().count(),
            1,
            "Alice should have exactly one asset after consuming the mint note"
        );

        let asset = alice_account_state.vault().assets().next().unwrap();

        if let Asset::Fungible(fungible_asset) = asset {
            assert_eq!(
                fungible_asset.faucet_id(),
                faucet_account.id(),
                "Asset should be from the faucet account"
            );
            assert_eq!(
                fungible_asset.amount(),
                mint_amount,
                "Alice should have the full minted amount of {mint_amount} tokens",
            );
        } else {
            panic!("Error: Alice's account should have a fungible asset, but found {asset:?}");
        }

        eprintln!("\n=== Step 3: Creating Bob's account ===");

        let bob_config = AccountCreationConfig {
            with_basic_wallet: false,
            ..Default::default()
        };
        let bob_account = create_account_with_component(
            &mut client,
            keystore.clone(),
            wallet_package,
            bob_config,
        )
        .await
        .unwrap();
        eprintln!("Bob account ID: {:?}", bob_account.id().to_hex());

        eprintln!("\n=== Step 4: Alice creates p2id note for Bob ===");

        let transfer_amount = 10_000u64; // 10,000 tokens
        let transfer_asset = FungibleAsset::new(faucet_account.id(), transfer_amount).unwrap();

        // Create the p2id note from Alice to Bob
        let p2id_note = create_note_from_package(
            &mut client,
            note_package,
            alice_account.id(),
            NoteCreationConfig {
                assets: NoteAssets::new(vec![transfer_asset.into()]).unwrap(),
                inputs: vec![bob_account.id().prefix().as_felt(), bob_account.id().suffix()],
                ..Default::default()
            },
        );
        eprintln!("P2ID note hash: {:?}", p2id_note.id().to_hex());

        let tx_script_program = tx_script_package.unwrap_program();
        let tx_script = TransactionScript::from_parts(
            tx_script_program.mast_forest().clone(),
            tx_script_program.entrypoint(),
        );

        let tag = NoteTag::for_local_use_case(0, 0).unwrap();
        let aux = Felt::ZERO;
        let note_type = NoteType::Public;
        let execution_hint = NoteExecutionHint::always();

        let program_hash = tx_script_program.hash();
        let serial_num = RpoRandomCoin::new(program_hash.into()).draw_word();
        let inputs =
            NoteInputs::new(vec![bob_account.id().prefix().as_felt(), bob_account.id().suffix()])
                .unwrap();
        let note_recipient = NoteRecipient::new(serial_num, p2id_note.script().clone(), inputs);
        let mut input: Vec<Felt> = vec![tag.into(), aux, note_type.into(), execution_hint.into()];
        let recipient: [Felt; 4] = note_recipient.digest().into();
        input.extend(recipient);

        let asset_arr: [Felt; 4] = transfer_asset.into();
        input.extend(asset_arr);

        let mut commitment: [Felt; 4] =
            miden_core::crypto::hash::Rpo256::hash_elements(&input).into();

        let mut advice_map = std::collections::BTreeMap::new();
        // NOTE: input must be word-sized
        advice_map.insert(commitment.into(), input.clone());

        let recipients = vec![note_recipient.clone()];

        // NOTE: passed on the stack reversed
        commitment.reverse();

        let alice_tx_request = TransactionRequestBuilder::new()
            .custom_script(tx_script)
            .script_arg(commitment)
            .expected_output_recipients(recipients)
            .extend_advice_map(advice_map)
            .build()
            .unwrap();

        let alice_tx = client.new_transaction(alice_account.id(), alice_tx_request).await.unwrap();

        let alice_tx_id = alice_tx.executed_transaction().id();
        eprintln!("Alice created p2id transaction. Tx ID: {alice_tx_id:?}");

        client.submit_transaction(alice_tx).await.unwrap();

        // Step 5: Bob attempts to consume the p2id note
        eprintln!("\n=== Step 5: Bob attempts to consume p2id note ===");

        let assets = NoteAssets::new(vec![transfer_asset.into()]).unwrap();
        let metadata =
            NoteMetadata::new(alice_account.id(), note_type, tag, execution_hint, aux).unwrap();
        let bob_note = Note::new(assets, metadata, note_recipient);
        let consume_request = TransactionRequestBuilder::new()
            .unauthenticated_input_notes([(bob_note, None)])
            .build()
            .unwrap();

        let consume_tx = client.new_transaction(bob_account.id(), consume_request).await.unwrap();
        let consume_tx_id = consume_tx.executed_transaction().id();
        eprintln!("Bob created consume transaction. Tx ID: {consume_tx_id:?}");

        client.submit_transaction(consume_tx).await.unwrap();

        eprintln!("\n=== Step 6: Checking Bob's account has the transferred asset ===");

        let bob_account_record = client
            .get_account(bob_account.id())
            .await
            .unwrap()
            .expect("Bob's account should exist");
        let bob_account_state: Account = bob_account_record.into();

        assert_eq!(
            bob_account_state.vault().assets().count(),
            1,
            "Bob should have exactly one asset after consuming the p2id note"
        );

        let asset = bob_account_state.vault().assets().next().unwrap();

        // Validate it's the asset transferred from Alice
        if let Asset::Fungible(fungible_asset) = asset {
            assert_eq!(
                fungible_asset.faucet_id(),
                faucet_account.id(),
                "Asset should be from the original faucet account"
            );
            assert_eq!(
                fungible_asset.amount(),
                transfer_amount,
                "Bob should have the transferred amount of {transfer_amount} tokens",
            );
        } else {
            panic!("Error: Bob's account should have a fungible asset, but found {asset:?}");
        }
    });
}
