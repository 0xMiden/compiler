//! Basic wallet test module

use std::{borrow::Borrow, collections::BTreeSet, sync::Arc};

use miden_client::{
    Word,
    account::component::BasicWallet,
    asset::FungibleAsset,
    crypto::{FeltRng, RpoRandomCoin},
    note::{
        Note, NoteAssets, NoteExecutionHint, NoteInputs, NoteMetadata, NoteRecipient, NoteScript,
        NoteTag, NoteType,
    },
    testing::{AccountState, Auth, MockChain, TransactionContextBuilder},
    transaction::OutputNote,
    utils::Deserializable,
};
use miden_core::{Felt, FieldElement, crypto::hash::Rpo256};
use miden_felt_repr_offchain::{AccountIdFeltRepr, ToFeltRepr};
use miden_lib::account::interface::AccountInterface;
use miden_mast_package::{Package, SectionId};
use miden_objects::account::{
    Account, AccountBuilder, AccountComponent, AccountComponentMetadata, AccountComponentTemplate,
    AccountId, AccountStorageMode, AccountType,
};

use super::helpers::compile_rust_package;

/// Configuration for creating a note.
#[derive(Debug, Clone)]
pub struct NoteCreationConfig {
    /// The note type (public/private).
    pub note_type: NoteType,
    /// The note tag.
    pub tag: NoteTag,
    /// Assets carried by the note.
    pub assets: NoteAssets,
    /// Note inputs (e.g. target account id, timelock/reclaim height, etc.).
    pub inputs: Vec<Felt>,
    /// Execution hint for the note script.
    pub execution_hint: NoteExecutionHint,
    /// Auxiliary note metadata field.
    pub aux: Felt,
}

impl Default for NoteCreationConfig {
    fn default() -> Self {
        Self {
            note_type: NoteType::Public,
            tag: NoteTag::for_local_use_case(0, 0).unwrap(),
            assets: Default::default(),
            inputs: Default::default(),
            execution_hint: NoteExecutionHint::always(),
            aux: Felt::ZERO,
        }
    }
}

/// Creates an account component from a compiled package's component metadata.
fn account_component_from_package(package: Arc<Package>) -> AccountComponent {
    let account_component_metadata = package.sections.iter().find_map(|section| {
        if section.id == SectionId::ACCOUNT_COMPONENT_METADATA {
            Some(section.data.borrow())
        } else {
            None
        }
    });

    match account_component_metadata {
        None => panic!("no account component metadata present"),
        Some(bytes) => {
            let metadata = AccountComponentMetadata::read_from_bytes(bytes).unwrap();
            let template =
                AccountComponentTemplate::new(metadata, package.unwrap_library().as_ref().clone());

            let supported_types = BTreeSet::from_iter([AccountType::RegularAccountUpdatableCode]);
            AccountComponent::new(template.library().clone(), vec![])
                .unwrap()
                .with_supported_types(supported_types)
        }
    }
}

/// Builds an account builder for an existing basic-wallet account based on the provided component
/// package.
fn build_basic_wallet_account_builder(
    wallet_package: Arc<Package>,
    with_std_basic_wallet: bool,
    seed: [u8; 32],
) -> AccountBuilder {
    let wallet_component = account_component_from_package(wallet_package);

    let mut builder = AccountBuilder::new(seed)
        .account_type(AccountType::RegularAccountUpdatableCode)
        .storage_mode(AccountStorageMode::Public)
        .with_component(wallet_component);

    if with_std_basic_wallet {
        builder = builder.with_component(BasicWallet);
    }

    builder
}

/// Creates a note from a compiled note package without requiring a `Client` RNG.
fn create_note_from_package(
    package: Arc<Package>,
    sender_id: AccountId,
    config: NoteCreationConfig,
    rng: &mut impl FeltRng,
) -> Note {
    let note_program = package.unwrap_program();
    let note_script =
        NoteScript::from_parts(note_program.mast_forest().clone(), note_program.entrypoint());

    let serial_num = rng.draw_word();
    let note_inputs = NoteInputs::new(config.inputs).unwrap();
    let recipient = NoteRecipient::new(serial_num, note_script, note_inputs);

    let metadata = NoteMetadata::new(
        sender_id,
        config.note_type,
        config.tag,
        config.execution_hint,
        config.aux,
    )
    .unwrap();

    Note::new(config.assets, metadata, recipient)
}

/// Builds a `send_notes` transaction script for accounts that support a standard note creation
/// interface (e.g. basic wallets and basic fungible faucets).
fn build_send_notes_script(
    account: &Account,
    notes: &[Note],
) -> miden_objects::transaction::TransactionScript {
    let partial_notes =
        notes.iter().map(miden_objects::note::PartialNote::from).collect::<Vec<_>>();

    AccountInterface::from(account)
        .build_send_notes_script(&partial_notes, None, false)
        .expect("failed to build send_notes transaction script")
}

/// Executes a transaction context against the chain and commits it in the next block.
async fn execute_tx(chain: &mut MockChain, tx_context_builder: TransactionContextBuilder) {
    let tx_context = tx_context_builder.build().unwrap();
    let executed_tx = tx_context.execute().await.unwrap();
    chain.add_pending_executed_transaction(&executed_tx).unwrap();
    chain.prove_next_block().unwrap();
}

/// Asserts that the account vault contains a fungible asset from the expected faucet with the
/// expected total amount.
fn assert_account_has_fungible_asset(
    account: &Account,
    expected_faucet_id: AccountId,
    expected_amount: u64,
) {
    let found_asset = account.vault().assets().find_map(|asset| match asset {
        miden_objects::asset::Asset::Fungible(fungible_asset)
            if fungible_asset.faucet_id() == expected_faucet_id =>
        {
            Some(fungible_asset)
        }
        _ => None,
    });

    match found_asset {
        Some(fungible_asset) => assert_eq!(
            fungible_asset.amount(),
            expected_amount,
            "Found asset from faucet {expected_faucet_id} but amount {} doesn't match expected \
             {expected_amount}",
            fungible_asset.amount()
        ),
        None => {
            panic!("Account does not contain a fungible asset from faucet {expected_faucet_id}")
        }
    }
}

/// Builds a transaction context which transfers an asset from `sender_id` to `recipient_id` using
/// the custom transaction script package.
///
/// This mirrors the `send_asset_to_account` helper logic (advice-map + script-arg commitment)
/// without requiring a local node `Client`.
fn build_asset_transfer_tx(
    chain: &MockChain,
    sender_id: AccountId,
    recipient_id: AccountId,
    asset: FungibleAsset,
    p2id_note_package: Arc<Package>,
    tx_script_package: Arc<Package>,
) -> (TransactionContextBuilder, Note) {
    let note_program = p2id_note_package.unwrap_program();
    let note_script =
        NoteScript::from_parts(note_program.mast_forest().clone(), note_program.entrypoint());

    let tx_script_program = tx_script_package.unwrap_program();
    let tx_script = miden_objects::transaction::TransactionScript::from_parts(
        tx_script_program.mast_forest().clone(),
        tx_script_program.entrypoint(),
    );

    let serial_num = RpoRandomCoin::new(tx_script_program.hash()).draw_word();
    let inputs = NoteInputs::new(AccountIdFeltRepr(&recipient_id).to_felt_repr()).unwrap();
    let note_recipient = NoteRecipient::new(serial_num, note_script, inputs);

    let config = NoteCreationConfig {
        assets: NoteAssets::new(vec![asset.into()]).unwrap(),
        ..Default::default()
    };
    let metadata = NoteMetadata::new(
        sender_id,
        config.note_type,
        config.tag,
        config.execution_hint,
        config.aux,
    )
    .unwrap();
    let output_note = Note::new(config.assets, metadata, note_recipient.clone());

    // Prepare commitment data
    let mut commitment_input: Vec<Felt> = vec![
        config.tag.into(),
        config.aux,
        Felt::from(config.note_type),
        Felt::from(config.execution_hint),
    ];
    let recipient_digest: [Felt; 4] = note_recipient.digest().into();
    commitment_input.extend(recipient_digest);

    let asset_arr: Word = asset.into();
    commitment_input.extend(asset_arr);

    let commitment_key: Word = Rpo256::hash_elements(&commitment_input);
    assert_eq!(commitment_input.len() % 4, 0, "commitment input needs to be word-aligned");

    // NOTE: passed on the stack reversed
    let mut commitment_arg = commitment_key;
    commitment_arg.reverse();

    let tx_context_builder = chain
        .build_tx_context(sender_id, &[], &[])
        .unwrap()
        .tx_script(tx_script)
        .tx_script_args(commitment_arg)
        .extend_advice_map([(commitment_key, commitment_input)])
        .extend_expected_output_notes(vec![OutputNote::Full(output_note.clone())]);

    (tx_context_builder, output_note)
}

/// Tests the basic-wallet contract deployment and p2id note consumption workflow on a mock chain.
#[test]
pub fn test_basic_wallet_p2id_mockchain() {
    // Compile the contracts first (before creating any runtime)
    let wallet_package = compile_rust_package("../../examples/basic-wallet", true);
    let note_package = compile_rust_package("../../examples/p2id-note", true);
    let tx_script_package = compile_rust_package("../../examples/basic-wallet-tx-script", true);

    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let mut builder = MockChain::builder();
        let max_supply = 1_000_000_000u64;
        let faucet_account = builder
            .add_existing_basic_faucet(Auth::BasicAuth, "TEST", max_supply, None)
            .unwrap();
        let faucet_id = faucet_account.id();

        let alice_account = builder
            .add_account_from_builder(
                Auth::BasicAuth,
                build_basic_wallet_account_builder(wallet_package.clone(), false, [1_u8; 32]),
                AccountState::Exists,
            )
            .unwrap();
        let alice_id = alice_account.id();

        let bob_account = builder
            .add_account_from_builder(
                Auth::BasicAuth,
                build_basic_wallet_account_builder(wallet_package, false, [2_u8; 32]),
                AccountState::Exists,
            )
            .unwrap();
        let bob_id = bob_account.id();

        let mut chain = builder.build().unwrap();
        chain.prove_next_block().unwrap();
        chain.prove_next_block().unwrap();

        eprintln!("\n=== Step 1: Minting tokens from faucet to Alice ===");
        let mint_amount = 100_000u64; // 100,000 tokens
        let mint_asset = FungibleAsset::new(faucet_id, mint_amount).unwrap();

        let mut note_rng = RpoRandomCoin::new(note_package.unwrap_program().hash());
        let p2id_note_mint = create_note_from_package(
            note_package.clone(),
            faucet_id,
            NoteCreationConfig {
                assets: NoteAssets::new(vec![mint_asset.into()]).unwrap(),
                inputs: AccountIdFeltRepr(&alice_id).to_felt_repr(),
                ..Default::default()
            },
            &mut note_rng,
        );

        let faucet_account = chain.committed_account(faucet_id).unwrap().clone();
        let mint_tx_script =
            build_send_notes_script(&faucet_account, std::slice::from_ref(&p2id_note_mint));
        let mint_tx_context_builder = chain
            .build_tx_context(faucet_id, &[], &[])
            .unwrap()
            .tx_script(mint_tx_script)
            .extend_expected_output_notes(vec![OutputNote::Full(p2id_note_mint.clone())]);
        execute_tx(&mut chain, mint_tx_context_builder).await;

        eprintln!("\n=== Step 2: Alice consumes mint note ===");
        let consume_tx_context_builder =
            chain.build_tx_context(alice_id, &[p2id_note_mint.id()], &[]).unwrap();
        execute_tx(&mut chain, consume_tx_context_builder).await;

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
        );
        execute_tx(&mut chain, alice_tx_context_builder).await;

        eprintln!("\n=== Step 4: Bob consumes p2id note ===");
        let consume_tx_context_builder =
            chain.build_tx_context(bob_id, &[bob_note.id()], &[]).unwrap();
        execute_tx(&mut chain, consume_tx_context_builder).await;

        eprintln!("\n=== Checking Bob's account has the transferred asset ===");
        let bob_account = chain.committed_account(bob_id).unwrap();
        assert_account_has_fungible_asset(bob_account, faucet_id, transfer_amount);

        eprintln!("\n=== Checking Alice's account reflects the new token amount ===");
        let alice_account = chain.committed_account(alice_id).unwrap();
        assert_account_has_fungible_asset(alice_account, faucet_id, mint_amount - transfer_amount);
    });
}

/// Tests the basic-wallet contract deployment and p2ide note consumption workflow on a mock chain.
///
/// Flow:
/// - Create fungible faucet and mint tokens to Alice
/// - Alice creates a p2ide note for Bob (with timelock=0, reclaim=0)
/// - Bob consumes the p2ide note and receives the assets
#[test]
pub fn test_basic_wallet_p2ide_mockchain() {
    // Compile the contracts first (before creating any runtime)
    let wallet_package = compile_rust_package("../../examples/basic-wallet", true);
    let p2id_note_package = compile_rust_package("../../examples/p2id-note", true);
    let p2ide_note_package = compile_rust_package("../../examples/p2ide-note", true);

    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let mut builder = MockChain::builder();
        let max_supply = 1_000_000_000u64;
        let faucet_account = builder
            .add_existing_basic_faucet(Auth::BasicAuth, "TEST", max_supply, None)
            .unwrap();
        let faucet_id = faucet_account.id();

        let alice_account = builder
            .add_account_from_builder(
                Auth::BasicAuth,
                build_basic_wallet_account_builder(wallet_package.clone(), true, [3_u8; 32]),
                AccountState::Exists,
            )
            .unwrap();
        let alice_id = alice_account.id();

        let bob_account = builder
            .add_account_from_builder(
                Auth::BasicAuth,
                build_basic_wallet_account_builder(wallet_package, false, [4_u8; 32]),
                AccountState::Exists,
            )
            .unwrap();
        let bob_id = bob_account.id();

        let mut chain = builder.build().unwrap();
        chain.prove_next_block().unwrap();
        chain.prove_next_block().unwrap();

        // Step 1: Mint assets from faucet to Alice using p2id note
        let mint_amount = 100_000u64;
        let mint_asset = FungibleAsset::new(faucet_id, mint_amount).unwrap();

        let mut p2id_rng = RpoRandomCoin::new(p2id_note_package.unwrap_program().hash());
        let p2id_note_mint = create_note_from_package(
            p2id_note_package.clone(),
            faucet_id,
            NoteCreationConfig {
                assets: NoteAssets::new(vec![mint_asset.into()]).unwrap(),
                inputs: AccountIdFeltRepr(&alice_id).to_felt_repr(),
                ..Default::default()
            },
            &mut p2id_rng,
        );

        let faucet_account = chain.committed_account(faucet_id).unwrap().clone();
        let mint_tx_script =
            build_send_notes_script(&faucet_account, std::slice::from_ref(&p2id_note_mint));
        let mint_tx_context_builder = chain
            .build_tx_context(faucet_id, &[], &[])
            .unwrap()
            .tx_script(mint_tx_script)
            .extend_expected_output_notes(vec![OutputNote::Full(p2id_note_mint.clone())]);
        execute_tx(&mut chain, mint_tx_context_builder).await;

        // Step 2: Alice consumes the p2id note
        let consume_tx_context_builder =
            chain.build_tx_context(alice_id, &[p2id_note_mint.id()], &[]).unwrap();
        execute_tx(&mut chain, consume_tx_context_builder).await;

        let alice_account = chain.committed_account(alice_id).unwrap();
        assert_account_has_fungible_asset(alice_account, faucet_id, mint_amount);

        // Step 3: Alice creates p2ide note for Bob
        let transfer_amount = 10_000u64;
        let transfer_asset = FungibleAsset::new(faucet_id, transfer_amount).unwrap();
        let timelock_height = Felt::new(0);
        let reclaim_height = Felt::new(0);

        let mut p2ide_rng = RpoRandomCoin::new(p2ide_note_package.unwrap_program().hash());
        let p2ide_note = create_note_from_package(
            p2ide_note_package,
            alice_id,
            NoteCreationConfig {
                assets: NoteAssets::new(vec![transfer_asset.into()]).unwrap(),
                inputs: {
                    let mut inputs = AccountIdFeltRepr(&bob_id).to_felt_repr();
                    inputs.extend([timelock_height, reclaim_height]);
                    inputs
                },
                ..Default::default()
            },
            &mut p2ide_rng,
        );

        let alice_account = chain.committed_account(alice_id).unwrap().clone();
        let transfer_tx_script =
            build_send_notes_script(&alice_account, std::slice::from_ref(&p2ide_note));
        let transfer_tx_context_builder = chain
            .build_tx_context(alice_id, &[], &[])
            .unwrap()
            .tx_script(transfer_tx_script)
            .extend_expected_output_notes(vec![OutputNote::Full(p2ide_note.clone())]);
        execute_tx(&mut chain, transfer_tx_context_builder).await;

        // Step 4: Bob consumes the p2ide note
        let consume_tx_context_builder =
            chain.build_tx_context(bob_id, &[p2ide_note.id()], &[]).unwrap();
        execute_tx(&mut chain, consume_tx_context_builder).await;

        // Step 5: verify balances
        let bob_account = chain.committed_account(bob_id).unwrap();
        assert_account_has_fungible_asset(bob_account, faucet_id, transfer_amount);

        let alice_account = chain.committed_account(alice_id).unwrap();
        assert_account_has_fungible_asset(alice_account, faucet_id, mint_amount - transfer_amount);
    });
}

/// Tests the p2ide note reclaim functionality.
///
/// Flow:
/// - Create fungible faucet and mint tokens to Alice
/// - Alice creates a p2ide note intended for Bob (with reclaim enabled)
/// - Alice reclaims the note herself (exercises the reclaim branch)
/// - Verify Alice has her original balance back
#[test]
pub fn test_basic_wallet_p2ide_reclaim_mockchain() {
    // Compile the contracts first (before creating any runtime)
    let wallet_package = compile_rust_package("../../examples/basic-wallet", true);
    let p2id_note_package = compile_rust_package("../../examples/p2id-note", true);
    let p2ide_note_package = compile_rust_package("../../examples/p2ide-note", true);

    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let mut builder = MockChain::builder();
        let max_supply = 1_000_000_000u64;
        let faucet_account = builder
            .add_existing_basic_faucet(Auth::BasicAuth, "TEST", max_supply, None)
            .unwrap();
        let faucet_id = faucet_account.id();

        let alice_account = builder
            .add_account_from_builder(
                Auth::BasicAuth,
                build_basic_wallet_account_builder(wallet_package.clone(), true, [5_u8; 32]),
                AccountState::Exists,
            )
            .unwrap();
        let alice_id = alice_account.id();

        let bob_account = builder
            .add_account_from_builder(
                Auth::BasicAuth,
                build_basic_wallet_account_builder(wallet_package, false, [6_u8; 32]),
                AccountState::Exists,
            )
            .unwrap();
        let bob_id = bob_account.id();

        let mut chain = builder.build().unwrap();
        chain.prove_next_block().unwrap();
        chain.prove_next_block().unwrap();

        // Step 1: Mint assets from faucet to Alice using p2id note
        let mint_amount = 100_000u64;
        let mint_asset = FungibleAsset::new(faucet_id, mint_amount).unwrap();

        let mut p2id_rng = RpoRandomCoin::new(p2id_note_package.unwrap_program().hash());
        let p2id_note_mint = create_note_from_package(
            p2id_note_package.clone(),
            faucet_id,
            NoteCreationConfig {
                assets: NoteAssets::new(vec![mint_asset.into()]).unwrap(),
                inputs: AccountIdFeltRepr(&alice_id).to_felt_repr(),
                ..Default::default()
            },
            &mut p2id_rng,
        );

        let faucet_account = chain.committed_account(faucet_id).unwrap().clone();
        let mint_tx_script =
            build_send_notes_script(&faucet_account, std::slice::from_ref(&p2id_note_mint));
        let mint_tx_context_builder = chain
            .build_tx_context(faucet_id, &[], &[])
            .unwrap()
            .tx_script(mint_tx_script)
            .extend_expected_output_notes(vec![OutputNote::Full(p2id_note_mint.clone())]);
        execute_tx(&mut chain, mint_tx_context_builder).await;

        // Step 2: Alice consumes the p2id note
        let consume_tx_context_builder =
            chain.build_tx_context(alice_id, &[p2id_note_mint.id()], &[]).unwrap();
        execute_tx(&mut chain, consume_tx_context_builder).await;

        let alice_account = chain.committed_account(alice_id).unwrap();
        assert_account_has_fungible_asset(alice_account, faucet_id, mint_amount);

        // Step 3: Alice creates p2ide note for Bob with reclaim enabled
        let transfer_amount = 10_000u64;
        let transfer_asset = FungibleAsset::new(faucet_id, transfer_amount).unwrap();
        let timelock_height = Felt::new(0);
        let reclaim_height = Felt::new(1000);

        let mut p2ide_rng = RpoRandomCoin::new(p2ide_note_package.unwrap_program().hash());
        let p2ide_note = create_note_from_package(
            p2ide_note_package,
            alice_id,
            NoteCreationConfig {
                assets: NoteAssets::new(vec![transfer_asset.into()]).unwrap(),
                inputs: {
                    let mut inputs = AccountIdFeltRepr(&bob_id).to_felt_repr();
                    inputs.extend([timelock_height, reclaim_height]);
                    inputs
                },
                ..Default::default()
            },
            &mut p2ide_rng,
        );

        let alice_account = chain.committed_account(alice_id).unwrap().clone();
        let transfer_tx_script =
            build_send_notes_script(&alice_account, std::slice::from_ref(&p2ide_note));
        let transfer_tx_context_builder = chain
            .build_tx_context(alice_id, &[], &[])
            .unwrap()
            .tx_script(transfer_tx_script)
            .extend_expected_output_notes(vec![OutputNote::Full(p2ide_note.clone())]);
        execute_tx(&mut chain, transfer_tx_context_builder).await;

        // Step 4: Alice reclaims the note (exercises the reclaim branch)
        let reclaim_tx_context_builder =
            chain.build_tx_context(alice_id, &[p2ide_note.id()], &[]).unwrap();
        execute_tx(&mut chain, reclaim_tx_context_builder).await;

        // Step 5: verify Alice has her original amount back
        let alice_account = chain.committed_account(alice_id).unwrap();
        assert_account_has_fungible_asset(alice_account, faucet_id, mint_amount);

        // Ensure Bob did not receive the asset.
        let bob_account = chain.committed_account(bob_id).unwrap();
        let bob_found = bob_account.vault().assets().find(|asset| {
            matches!(
                asset,
                miden_objects::asset::Asset::Fungible(fungible_asset)
                    if fungible_asset.faucet_id() == faucet_id
            )
        });
        assert!(bob_found.is_none(), "Bob unexpectedly received reclaimed assets");
    });
}
