//! SWAPP (partially-fillable swap) note tests.
//!
//! These tests exercise the `tests/fixtures/components/swapp-note` note script, a port of the
//! SWAPP note from <https://github.com/inicio-labs/miden-swapp>. A SWAPP note locks an offered
//! asset and asks for a requested asset in return; consumers can fill it fully or partially,
//! with the requested asset routed back to the creator through a P2ID note and the unfilled
//! portion re-offered through a remainder SWAPP note.

use std::{collections::BTreeMap, sync::Arc};

use miden_client::{
    account::component::{BasicWallet, InitStorageData},
    transaction::RawOutputNote,
};
use miden_core::{Felt, crypto::hash::Poseidon2};
use miden_mast_package::Package;
use miden_protocol::{
    Word,
    account::{
        Account, AccountBuilder, AccountComponent, AccountId, AccountType, auth::AuthScheme,
    },
    asset::{Asset, FungibleAsset},
    crypto::rand::RandomCoin,
    note::{
        Note, NoteAssets, NoteAttachment, NoteAttachmentScheme, NoteAttachments, NoteId,
        NoteRecipient, NoteScript, NoteStorage, NoteTag, NoteType, PartialNoteMetadata,
    },
    transaction::ExecutedTransaction,
};
use miden_standards::{
    note::{P2idNote, P2idNoteStorage},
    testing::note::NoteBuilder,
};
use miden_testing::{AccountState, Auth, MockChain, MockChainBuilder, TransactionContextBuilder};
use midenc_expect_test::expect;
use midenc_integration_test_support::testing::stripped_mast_size_str;

use super::support::{
    assert_account_has_fungible_asset, block_on, compile_rust_package, note_script_root,
    single_note_cycles, to_core_felts,
};

/// Tag used for the SWAPP notes themselves.
const SWAPP_NOTE_TAG: u32 = 0;

/// Compiled packages used by the SWAPP tests.
struct SwappPackages {
    wallet: Arc<Package>,
    swapp: Arc<Package>,
}

/// Compiles the basic-wallet account component and the swapp-note script.
///
/// The basic wallet is compiled first so that its package artifacts are available to the note
/// project which depends on it.
fn compile_swapp_packages() -> SwappPackages {
    let wallet = compile_rust_package("../../examples/basic-wallet", true);
    let swapp = compile_rust_package("../fixtures/components/swapp-note", true);
    SwappPackages { wallet, swapp }
}

/// Terms of a swap offer, encoded into the SWAPP note storage.
struct SwapTerms {
    /// The asset (and total amount) requested by the swap creator.
    requested_asset: FungibleAsset,
    /// The account that created the swap and receives the requested asset.
    creator: AccountId,
    /// Note type of the notes created by the SWAPP script.
    note_type: NoteType,
    /// Tag routing the P2ID payback note to the creator.
    p2id_tag: NoteTag,
    /// Script root of the P2ID note script used for the payback note.
    p2id_script_root: Word,
}

impl SwapTerms {
    fn new(requested_asset: FungibleAsset, creator: AccountId) -> Self {
        Self {
            requested_asset,
            creator,
            note_type: NoteType::Public,
            p2id_tag: NoteTag::with_account_target(creator),
            p2id_script_root: P2idNote::script_root().into(),
        }
    }

    /// Sets the note type used for the swap note and the notes created by the SWAPP script.
    fn with_note_type(mut self, note_type: NoteType) -> Self {
        self.note_type = note_type;
        self
    }

    /// Encodes the swap terms into the SWAPP note storage layout expected by the note script:
    /// `[requested_asset_key (4), requested_total, creator_prefix, creator_suffix, note_type,
    /// p2id_tag, p2id_script_root (4)]`.
    fn to_storage_felts(&self) -> Vec<Felt> {
        let requested_key: [Felt; 4] = self.requested_asset.to_key_word().into();
        let p2id_root: [Felt; 4] = self.p2id_script_root.into();

        let mut storage = requested_key.to_vec();
        storage.push(Felt::from(self.requested_asset.amount()));
        storage.extend(to_core_felts(&self.creator));
        storage.push(Felt::from(self.note_type));
        storage.push(Felt::from(self.p2id_tag));
        storage.extend(p2id_root);
        storage
    }
}

/// Returns the auth method used for all accounts in the SWAPP tests.
fn basic_auth() -> Auth {
    Auth::BasicAuth {
        auth_scheme: AuthScheme::Falcon512Poseidon2,
    }
}

/// Adds an existing public account with the Rust basic-wallet component and the provided
/// initial assets to the chain builder.
///
/// The standard [`BasicWallet`] component is attached as well: the protocol P2ID payback notes
/// created by the SWAPP script call its `receive_asset` procedure when the creator claims them.
fn add_wallet_account(
    builder: &mut MockChainBuilder,
    wallet_component: AccountComponent,
    assets: impl IntoIterator<Item = Asset>,
    seed: [u8; 32],
) -> Account {
    let account_builder = AccountBuilder::new(seed)
        .account_type(AccountType::Public)
        .with_component(wallet_component)
        .with_component(BasicWallet)
        .with_assets(assets);

    builder
        .add_account_from_builder(basic_auth(), account_builder, AccountState::Exists)
        .expect("failed to add wallet account to the mock chain")
}

/// Builds a SWAPP note offering `offered_asset` under the provided swap terms.
fn build_swapp_note(
    swapp_package: &Package,
    sender: AccountId,
    offered_asset: FungibleAsset,
    terms: &SwapTerms,
    rng: &mut RandomCoin,
) -> Note {
    NoteBuilder::new(sender, rng)
        .package(swapp_package.clone())
        .add_assets([Asset::from(offered_asset)])
        .note_storage(terms.to_storage_felts())
        .unwrap()
        .note_type(terms.note_type)
        .tag(SWAPP_NOTE_TAG)
        .build()
        .unwrap()
}

/// Builds the note args word consumed by the SWAPP note script:
/// `[input_amount, inflight_amount, 0, 0]`.
fn swapp_note_args(input_amount: u64, inflight_amount: u64) -> Word {
    Word::new([
        Felt::new(input_amount).unwrap(),
        Felt::new(inflight_amount).unwrap(),
        Felt::ZERO,
        Felt::ZERO,
    ])
}

/// Builds the aux word attachment the SWAPP script attaches to its created notes.
fn aux_attachments(aux: u64) -> NoteAttachments {
    let aux_word = Word::new([Felt::new(aux).unwrap(), Felt::ZERO, Felt::ZERO, Felt::ZERO]);
    let attachment = NoteAttachment::with_word(NoteAttachmentScheme::none(), aux_word);
    NoteAttachments::new(vec![attachment]).expect("aux attachment should be valid")
}

/// Predicts the protocol P2ID routing note the SWAPP script creates for the swap creator.
///
/// Mirrors the note script: serial is the swap note serial plus one in every element, storage
/// targets the creator, and the fill amount is carried both as the note asset and as the aux
/// attachment.
fn predict_routing_p2id_note(
    swap_note: &Note,
    consumer: AccountId,
    terms: &SwapTerms,
    fill_amount: u64,
) -> Note {
    let swap_serial = swap_note.recipient().serial_num();
    let serial = Word::new([
        swap_serial[0] + Felt::ONE,
        swap_serial[1] + Felt::ONE,
        swap_serial[2] + Felt::ONE,
        swap_serial[3] + Felt::ONE,
    ]);

    let recipient = P2idNoteStorage::new(terms.creator).into_recipient(serial);

    let fill_asset = FungibleAsset::new(terms.requested_asset.faucet_id(), fill_amount).unwrap();
    let assets = NoteAssets::new(vec![fill_asset.into()]).unwrap();

    let metadata = PartialNoteMetadata::new(consumer, terms.note_type).with_tag(terms.p2id_tag);
    Note::with_attachments(assets, metadata, recipient, aux_attachments(fill_amount))
}

/// Predicts the remainder SWAPP note created on a partial fill.
///
/// Mirrors the note script: serial is the hash of the swap note serial, the storage carries
/// the reduced swap terms, the tag is inherited from the consumed swap note, and the offered
/// amount paid out for this fill is carried as the aux attachment.
fn predict_remainder_note(
    swap_note: &Note,
    swapp_package: &Package,
    consumer: AccountId,
    remainder_terms: &SwapTerms,
    remaining_offered: FungibleAsset,
    offered_out_aux: u64,
) -> Note {
    let swap_serial: [Felt; 4] = swap_note.recipient().serial_num().into();
    let serial = Poseidon2::hash_elements(&swap_serial);

    let script = NoteScript::from_package(swapp_package).expect("swapp package is a note script");
    let storage = NoteStorage::new(remainder_terms.to_storage_felts()).unwrap();
    let recipient = NoteRecipient::new(serial, script, storage);

    let assets = NoteAssets::new(vec![remaining_offered.into()]).unwrap();

    let metadata = PartialNoteMetadata::new(consumer, remainder_terms.note_type)
        .with_tag(swap_note.metadata().tag());
    Note::with_attachments(assets, metadata, recipient, aux_attachments(offered_out_aux))
}

/// Executes a transaction context, commits it in the next block, and returns the executed
/// transaction for inspection.
fn execute_and_commit(
    chain: &mut MockChain,
    tx_context_builder: TransactionContextBuilder,
) -> ExecutedTransaction {
    let tx_context = tx_context_builder.build().unwrap();
    let executed_tx = block_on(tx_context.execute()).unwrap_or_else(|err| panic!("{err}"));

    chain.add_pending_executed_transaction(&executed_tx).unwrap();
    chain.prove_next_block().unwrap();

    executed_tx
}

/// Returns the ids of the output notes of an executed transaction.
fn output_note_ids(executed_tx: &ExecutedTransaction) -> Vec<NoteId> {
    let output_notes = executed_tx.output_notes();
    (0..output_notes.num_notes())
        .map(|idx| output_notes.get_note(idx).id())
        .collect()
}

/// Asserts that the account vault holds no fungible asset from the given faucet.
fn assert_no_fungible_asset(account: &Account, faucet_id: AccountId) {
    let found = account.vault().assets().find(|asset| {
        matches!(
            asset,
            Asset::Fungible(fungible_asset) if fungible_asset.faucet_id() == faucet_id
        )
    });
    assert!(
        found.is_none(),
        "account {} unexpectedly holds an asset from faucet {faucet_id}",
        account.id()
    );
}

/// Pins the artifact size of the compiled SWAPP note package (debug info stripped).
#[test]
fn swapp_note_package_size() {
    let packages = compile_swapp_packages();
    expect!["42456"].assert_eq(stripped_mast_size_str(packages.swapp.as_ref()));
}

/// Tests a full fill of a SWAPP note.
///
/// Alice offers 50 USDC for 25 ETH. Bob fills the swap completely: he receives the 50 USDC and
/// a P2ID routing note carrying his 25 ETH is created for Alice, who then consumes it.
#[test]
fn swapp_note_full_fill_transfers_assets() {
    let packages = compile_swapp_packages();
    let wallet_component =
        AccountComponent::from_package(&packages.wallet, &InitStorageData::default()).unwrap();

    let mut builder = MockChain::builder();
    let usdc_faucet = builder
        .add_existing_basic_faucet(basic_auth(), "USDC", 1_000_000, None)
        .unwrap();
    let eth_faucet =
        builder.add_existing_basic_faucet(basic_auth(), "ETH", 1_000_000, None).unwrap();

    let alice = add_wallet_account(&mut builder, wallet_component.clone(), [], [1u8; 32]);
    let bob = add_wallet_account(
        &mut builder,
        wallet_component,
        [FungibleAsset::new(eth_faucet.id(), 25).unwrap().into()],
        [2u8; 32],
    );

    // Alice offers 50 USDC for 25 ETH.
    let offered_asset = FungibleAsset::new(usdc_faucet.id(), 50).unwrap();
    let requested_asset = FungibleAsset::new(eth_faucet.id(), 25).unwrap();
    let terms = SwapTerms::new(requested_asset, alice.id());

    let mut rng = RandomCoin::new(note_script_root(packages.swapp.as_ref()));
    let swap_note =
        build_swapp_note(packages.swapp.as_ref(), alice.id(), offered_asset, &terms, &mut rng);
    builder.add_output_note(RawOutputNote::Full(swap_note.clone()));

    let mut chain = builder.build().unwrap();

    // Bob fills the swap completely with 25 ETH.
    let p2id_note = predict_routing_p2id_note(&swap_note, bob.id(), &terms, 25);
    let consume_builder = chain
        .build_tx_context(bob.id(), &[swap_note.id()], &[])
        .unwrap()
        .extend_note_args(BTreeMap::from([(swap_note.id(), swapp_note_args(25, 0))]))
        .extend_expected_output_notes(vec![RawOutputNote::Full(p2id_note.clone())]);
    let executed_tx = execute_and_commit(&mut chain, consume_builder);

    assert_eq!(
        output_note_ids(&executed_tx),
        vec![p2id_note.id()],
        "full fill must create exactly the P2ID routing note"
    );
    expect!["12161"].assert_eq(single_note_cycles(executed_tx.measurements()));

    let bob_account = chain.committed_account(bob.id()).unwrap();
    assert_account_has_fungible_asset(bob_account, usdc_faucet.id(), 50);
    assert_no_fungible_asset(bob_account, eth_faucet.id());

    // Alice consumes the routing P2ID note and receives the requested 25 ETH.
    let claim_builder = chain.build_tx_context(alice.id(), &[p2id_note.id()], &[]).unwrap();
    execute_and_commit(&mut chain, claim_builder);

    let alice_account = chain.committed_account(alice.id()).unwrap();
    assert_account_has_fungible_asset(alice_account, eth_faucet.id(), 25);
}

/// Tests a partial fill of a SWAPP note followed by a fill of the remainder note.
///
/// Alice offers 10 USDC for 3 ETH. Bob first fills 1 ETH: he receives 3 USDC, a P2ID note
/// carries 1 ETH to Alice, and a remainder SWAPP note re-offers 7 USDC for 2 ETH. Bob then
/// fills the remainder completely, and Alice consumes both P2ID notes.
#[test]
fn swapp_note_partial_fill_creates_remainder_and_chains() {
    let packages = compile_swapp_packages();
    let wallet_component =
        AccountComponent::from_package(&packages.wallet, &InitStorageData::default()).unwrap();

    let mut builder = MockChain::builder();
    let usdc_faucet = builder
        .add_existing_basic_faucet(basic_auth(), "USDC", 1_000_000, None)
        .unwrap();
    let eth_faucet =
        builder.add_existing_basic_faucet(basic_auth(), "ETH", 1_000_000, None).unwrap();

    let alice = add_wallet_account(&mut builder, wallet_component.clone(), [], [1u8; 32]);
    let bob = add_wallet_account(
        &mut builder,
        wallet_component,
        [FungibleAsset::new(eth_faucet.id(), 3).unwrap().into()],
        [2u8; 32],
    );

    // Alice offers 10 USDC for 3 ETH.
    let offered_asset = FungibleAsset::new(usdc_faucet.id(), 10).unwrap();
    let requested_asset = FungibleAsset::new(eth_faucet.id(), 3).unwrap();
    let terms = SwapTerms::new(requested_asset, alice.id());

    let mut rng = RandomCoin::new(note_script_root(packages.swapp.as_ref()));
    let swap_note =
        build_swapp_note(packages.swapp.as_ref(), alice.id(), offered_asset, &terms, &mut rng);
    builder.add_output_note(RawOutputNote::Full(swap_note.clone()));

    let mut chain = builder.build().unwrap();

    // Bob fills 1 of the 3 requested ETH: proportional payout is (10 / 3) * 1 = 3 USDC, so the
    // remainder note re-offers 7 USDC for the remaining 2 ETH.
    let first_p2id_note = predict_routing_p2id_note(&swap_note, bob.id(), &terms, 1);
    let remainder_terms =
        SwapTerms::new(FungibleAsset::new(eth_faucet.id(), 2).unwrap(), alice.id());
    let remainder_note = predict_remainder_note(
        &swap_note,
        packages.swapp.as_ref(),
        bob.id(),
        &remainder_terms,
        FungibleAsset::new(usdc_faucet.id(), 7).unwrap(),
        3,
    );

    let consume_builder = chain
        .build_tx_context(bob.id(), &[swap_note.id()], &[])
        .unwrap()
        .extend_note_args(BTreeMap::from([(swap_note.id(), swapp_note_args(1, 0))]))
        .extend_expected_output_notes(vec![
            RawOutputNote::Full(first_p2id_note.clone()),
            RawOutputNote::Full(remainder_note.clone()),
        ]);
    let executed_tx = execute_and_commit(&mut chain, consume_builder);

    assert_eq!(
        output_note_ids(&executed_tx),
        vec![first_p2id_note.id(), remainder_note.id()],
        "partial fill must create the P2ID routing note and the remainder note"
    );
    expect!["15982"].assert_eq(single_note_cycles(executed_tx.measurements()));

    let bob_account = chain.committed_account(bob.id()).unwrap();
    assert_account_has_fungible_asset(bob_account, usdc_faucet.id(), 3);
    assert_account_has_fungible_asset(bob_account, eth_faucet.id(), 2);

    // Bob fills the remainder note completely with the remaining 2 ETH.
    let second_p2id_note =
        predict_routing_p2id_note(&remainder_note, bob.id(), &remainder_terms, 2);
    let consume_builder = chain
        .build_tx_context(bob.id(), &[remainder_note.id()], &[])
        .unwrap()
        .extend_note_args(BTreeMap::from([(remainder_note.id(), swapp_note_args(2, 0))]))
        .extend_expected_output_notes(vec![RawOutputNote::Full(second_p2id_note.clone())]);
    let executed_tx = execute_and_commit(&mut chain, consume_builder);

    assert_eq!(
        output_note_ids(&executed_tx),
        vec![second_p2id_note.id()],
        "filling the remainder completely must create only the P2ID routing note"
    );

    let bob_account = chain.committed_account(bob.id()).unwrap();
    assert_account_has_fungible_asset(bob_account, usdc_faucet.id(), 10);
    assert_no_fungible_asset(bob_account, eth_faucet.id());

    // Alice consumes both P2ID routing notes and receives the requested 3 ETH in total.
    let claim_builder = chain
        .build_tx_context(alice.id(), &[first_p2id_note.id(), second_p2id_note.id()], &[])
        .unwrap();
    execute_and_commit(&mut chain, claim_builder);

    let alice_account = chain.committed_account(alice.id()).unwrap();
    assert_account_has_fungible_asset(alice_account, eth_faucet.id(), 3);
}

/// Tests that the SWAPP note creator can reclaim the offered asset by consuming their own
/// note: no routing or remainder notes are created.
#[test]
fn swapp_note_creator_reclaims_offered_asset() {
    let packages = compile_swapp_packages();
    let wallet_component =
        AccountComponent::from_package(&packages.wallet, &InitStorageData::default()).unwrap();

    let mut builder = MockChain::builder();
    let usdc_faucet = builder
        .add_existing_basic_faucet(basic_auth(), "USDC", 1_000_000, None)
        .unwrap();
    let eth_faucet =
        builder.add_existing_basic_faucet(basic_auth(), "ETH", 1_000_000, None).unwrap();

    let alice = add_wallet_account(&mut builder, wallet_component, [], [1u8; 32]);

    // Alice offers 50 USDC for 25 ETH.
    let offered_asset = FungibleAsset::new(usdc_faucet.id(), 50).unwrap();
    let requested_asset = FungibleAsset::new(eth_faucet.id(), 25).unwrap();
    let terms = SwapTerms::new(requested_asset, alice.id());

    let mut rng = RandomCoin::new(note_script_root(packages.swapp.as_ref()));
    let swap_note =
        build_swapp_note(packages.swapp.as_ref(), alice.id(), offered_asset, &terms, &mut rng);
    builder.add_output_note(RawOutputNote::Full(swap_note.clone()));

    let mut chain = builder.build().unwrap();

    // Alice reclaims her own swap note; no note args are needed.
    let reclaim_builder = chain.build_tx_context(alice.id(), &[swap_note.id()], &[]).unwrap();
    let executed_tx = execute_and_commit(&mut chain, reclaim_builder);

    assert!(
        output_note_ids(&executed_tx).is_empty(),
        "reclaiming the swap note must not create any output notes"
    );
    expect!["4896"].assert_eq(single_note_cycles(executed_tx.measurements()));

    let alice_account = chain.committed_account(alice.id()).unwrap();
    assert_account_has_fungible_asset(alice_account, usdc_faucet.id(), 50);
}

/// Tests an inflight cross swap: a matcher without capital crosses two opposite swap notes in
/// a single transaction.
///
/// Alice offers 25 ETH for 50 USDC and Charlie offers 50 USDC for 25 ETH. Bob (zero assets)
/// consumes both notes, declaring the assets released by each note as the inflight fill of the
/// other. Both creators are paid through P2ID routing notes and Bob's vault stays empty.
#[test]
fn swapp_note_inflight_cross_swap_without_capital() {
    let packages = compile_swapp_packages();
    let wallet_component =
        AccountComponent::from_package(&packages.wallet, &InitStorageData::default()).unwrap();

    let mut builder = MockChain::builder();
    let usdc_faucet = builder
        .add_existing_basic_faucet(basic_auth(), "USDC", 1_000_000, None)
        .unwrap();
    let eth_faucet =
        builder.add_existing_basic_faucet(basic_auth(), "ETH", 1_000_000, None).unwrap();

    let alice = add_wallet_account(&mut builder, wallet_component.clone(), [], [1u8; 32]);
    let charlie = add_wallet_account(&mut builder, wallet_component.clone(), [], [2u8; 32]);
    let bob = add_wallet_account(&mut builder, wallet_component, [], [3u8; 32]);

    let mut rng = RandomCoin::new(note_script_root(packages.swapp.as_ref()));

    // Alice offers 25 ETH for 50 USDC.
    let alice_terms = SwapTerms::new(FungibleAsset::new(usdc_faucet.id(), 50).unwrap(), alice.id());
    let alice_swap_note = build_swapp_note(
        packages.swapp.as_ref(),
        alice.id(),
        FungibleAsset::new(eth_faucet.id(), 25).unwrap(),
        &alice_terms,
        &mut rng,
    );
    builder.add_output_note(RawOutputNote::Full(alice_swap_note.clone()));

    // Charlie offers 50 USDC for 25 ETH.
    let charlie_terms =
        SwapTerms::new(FungibleAsset::new(eth_faucet.id(), 25).unwrap(), charlie.id());
    let charlie_swap_note = build_swapp_note(
        packages.swapp.as_ref(),
        charlie.id(),
        FungibleAsset::new(usdc_faucet.id(), 50).unwrap(),
        &charlie_terms,
        &mut rng,
    );
    builder.add_output_note(RawOutputNote::Full(charlie_swap_note.clone()));

    let mut chain = builder.build().unwrap();

    // Bob consumes both notes in one transaction with inflight-only fills: the 50 USDC
    // released by Charlie's note fill Alice's request and the 25 ETH released by Alice's note
    // fill Charlie's request.
    let alice_p2id_note = predict_routing_p2id_note(&alice_swap_note, bob.id(), &alice_terms, 50);
    let charlie_p2id_note =
        predict_routing_p2id_note(&charlie_swap_note, bob.id(), &charlie_terms, 25);

    let consume_builder = chain
        .build_tx_context(bob.id(), &[alice_swap_note.id(), charlie_swap_note.id()], &[])
        .unwrap()
        .extend_note_args(BTreeMap::from([
            (alice_swap_note.id(), swapp_note_args(0, 50)),
            (charlie_swap_note.id(), swapp_note_args(0, 25)),
        ]))
        .extend_expected_output_notes(vec![
            RawOutputNote::Full(alice_p2id_note.clone()),
            RawOutputNote::Full(charlie_p2id_note.clone()),
        ]);
    let executed_tx = execute_and_commit(&mut chain, consume_builder);

    assert_eq!(
        output_note_ids(&executed_tx),
        vec![alice_p2id_note.id(), charlie_p2id_note.id()],
        "the cross swap must create exactly the two P2ID routing notes"
    );

    // Bob acted purely as a matcher and keeps nothing.
    let bob_account = chain.committed_account(bob.id()).unwrap();
    assert_no_fungible_asset(bob_account, usdc_faucet.id());
    assert_no_fungible_asset(bob_account, eth_faucet.id());

    // Both creators consume their P2ID routing notes.
    let claim_builder = chain.build_tx_context(alice.id(), &[alice_p2id_note.id()], &[]).unwrap();
    execute_and_commit(&mut chain, claim_builder);
    let claim_builder =
        chain.build_tx_context(charlie.id(), &[charlie_p2id_note.id()], &[]).unwrap();
    execute_and_commit(&mut chain, claim_builder);

    let alice_account = chain.committed_account(alice.id()).unwrap();
    assert_account_has_fungible_asset(alice_account, usdc_faucet.id(), 50);
    let charlie_account = chain.committed_account(charlie.id()).unwrap();
    assert_account_has_fungible_asset(charlie_account, eth_faucet.id(), 25);
}

/// Builds a chain with a single SWAPP note (offering `offered_amount` USDC for
/// `requested_amount` ETH) and asserts that a non-creator consuming it with the provided note
/// args fails.
fn assert_swapp_fill_fails(
    packages: &SwappPackages,
    offered_amount: u64,
    requested_amount: u64,
    consumer_eth_amount: u64,
    note_args: Word,
    context: &str,
) {
    let wallet_component =
        AccountComponent::from_package(&packages.wallet, &InitStorageData::default()).unwrap();

    let mut builder = MockChain::builder();
    let usdc_faucet = builder
        .add_existing_basic_faucet(basic_auth(), "USDC", 1_000_000, None)
        .unwrap();
    let eth_faucet =
        builder.add_existing_basic_faucet(basic_auth(), "ETH", 1_000_000, None).unwrap();

    let alice = add_wallet_account(&mut builder, wallet_component.clone(), [], [1u8; 32]);
    let bob_assets = (consumer_eth_amount > 0)
        .then(|| FungibleAsset::new(eth_faucet.id(), consumer_eth_amount).unwrap().into());
    let bob = add_wallet_account(&mut builder, wallet_component, bob_assets, [2u8; 32]);

    let offered_asset = FungibleAsset::new(usdc_faucet.id(), offered_amount).unwrap();
    let requested_asset = FungibleAsset::new(eth_faucet.id(), requested_amount).unwrap();
    let terms = SwapTerms::new(requested_asset, alice.id());

    let mut rng = RandomCoin::new(note_script_root(packages.swapp.as_ref()));
    let swap_note =
        build_swapp_note(packages.swapp.as_ref(), alice.id(), offered_asset, &terms, &mut rng);
    builder.add_output_note(RawOutputNote::Full(swap_note.clone()));

    let chain = builder.build().unwrap();

    let tx_context = chain
        .build_tx_context(bob.id(), &[swap_note.id()], &[])
        .unwrap()
        .extend_note_args(BTreeMap::from([(swap_note.id(), note_args)]))
        .build()
        .unwrap();

    let result = block_on(tx_context.execute());
    assert!(result.is_err(), "{context}");
}

/// Tests that filling a SWAPP note with more than the requested amount fails via the fill
/// guard (here the proportional payout of 60 USDC would also exceed the 50 USDC locked in the
/// note).
#[test]
fn swapp_note_overfill_fails() {
    let packages = compile_swapp_packages();
    assert_swapp_fill_fails(
        &packages,
        50,
        25,
        30,
        swapp_note_args(30, 0),
        "overfilling the swap must fail",
    );
}

/// Tests that an overfill is rejected even when the rounded payout still fits the locked
/// offered asset: 1 offered for 2 requested, filled with 3, pays out only 1, so without the
/// fill guard asset conservation would hold and the overfill would succeed.
#[test]
fn swapp_note_overfill_fails_when_payout_fits() {
    let packages = compile_swapp_packages();
    assert_swapp_fill_fails(
        &packages,
        1,
        2,
        3,
        swapp_note_args(3, 0),
        "an overfill must fail even when the payout fits the locked asset",
    );
}

/// Tests that a zero fill by a non-creator fails: it would recreate the order under a new
/// note id for free (griefing).
#[test]
fn swapp_note_zero_fill_fails() {
    let packages = compile_swapp_packages();
    assert_swapp_fill_fails(&packages, 50, 25, 0, swapp_note_args(0, 0), "a zero fill must fail");
}

/// Pins an inherited limitation of the SWAPP ratio math: a full fill of a non-divisible offer
/// fails asset conservation. 10 offered for 3 requested, filled with 3, pays out
/// `floor(3 * floor(10 * 100000 / 3) / 100000) = 9` and creates no remainder note, leaving
/// 1 offered unit unaccounted for.
#[test]
fn swapp_note_non_divisible_full_fill_fails() {
    let packages = compile_swapp_packages();
    assert_swapp_fill_fails(
        &packages,
        10,
        3,
        3,
        swapp_note_args(3, 0),
        "a non-divisible full fill is expected to fail asset conservation",
    );
}

/// Tests a partial fill of a private SWAPP note.
///
/// The note type stored in the swap terms must be applied to both notes created by the SWAPP
/// script: the P2ID payback note and the remainder SWAPP note are private, and the payback
/// note is consumable by the creator.
#[test]
fn swapp_note_private_partial_fill_creates_private_notes() {
    let packages = compile_swapp_packages();
    let wallet_component =
        AccountComponent::from_package(&packages.wallet, &InitStorageData::default()).unwrap();

    let mut builder = MockChain::builder();
    let usdc_faucet = builder
        .add_existing_basic_faucet(basic_auth(), "USDC", 1_000_000, None)
        .unwrap();
    let eth_faucet =
        builder.add_existing_basic_faucet(basic_auth(), "ETH", 1_000_000, None).unwrap();

    let alice = add_wallet_account(&mut builder, wallet_component.clone(), [], [1u8; 32]);
    let bob = add_wallet_account(
        &mut builder,
        wallet_component,
        [FungibleAsset::new(eth_faucet.id(), 1).unwrap().into()],
        [2u8; 32],
    );

    // Alice privately offers 10 USDC for 3 ETH.
    let offered_asset = FungibleAsset::new(usdc_faucet.id(), 10).unwrap();
    let requested_asset = FungibleAsset::new(eth_faucet.id(), 3).unwrap();
    let terms = SwapTerms::new(requested_asset, alice.id()).with_note_type(NoteType::Private);

    let mut rng = RandomCoin::new(note_script_root(packages.swapp.as_ref()));
    let swap_note =
        build_swapp_note(packages.swapp.as_ref(), alice.id(), offered_asset, &terms, &mut rng);
    builder.add_output_note(RawOutputNote::Full(swap_note.clone()));

    let mut chain = builder.build().unwrap();

    // Bob fills 1 of the 3 requested ETH.
    let p2id_note = predict_routing_p2id_note(&swap_note, bob.id(), &terms, 1);
    let remainder_terms =
        SwapTerms::new(FungibleAsset::new(eth_faucet.id(), 2).unwrap(), alice.id())
            .with_note_type(NoteType::Private);
    let remainder_note = predict_remainder_note(
        &swap_note,
        packages.swapp.as_ref(),
        bob.id(),
        &remainder_terms,
        FungibleAsset::new(usdc_faucet.id(), 7).unwrap(),
        3,
    );

    let consume_builder = chain
        .build_tx_context(bob.id(), &[swap_note.id()], &[])
        .unwrap()
        .extend_note_args(BTreeMap::from([(swap_note.id(), swapp_note_args(1, 0))]))
        .extend_expected_output_notes(vec![
            RawOutputNote::Full(p2id_note.clone()),
            RawOutputNote::Full(remainder_note.clone()),
        ]);
    let executed_tx = execute_and_commit(&mut chain, consume_builder);

    assert_eq!(
        output_note_ids(&executed_tx),
        vec![p2id_note.id(), remainder_note.id()],
        "the private partial fill must create the P2ID routing note and the remainder note"
    );
    let output_notes = executed_tx.output_notes();
    for idx in 0..output_notes.num_notes() {
        assert_eq!(
            output_notes.get_note(idx).metadata().note_type(),
            NoteType::Private,
            "notes created by a private swap must be private"
        );
    }

    let bob_account = chain.committed_account(bob.id()).unwrap();
    assert_account_has_fungible_asset(bob_account, usdc_faucet.id(), 3);

    // Alice claims the private P2ID payback note. The mock chain only tracks the private
    // note's header, so the full note details are provided as an unauthenticated input note.
    let claim_builder = chain
        .build_tx_context(alice.id(), &[], std::slice::from_ref(&p2id_note))
        .unwrap();
    execute_and_commit(&mut chain, claim_builder);

    let alice_account = chain.committed_account(alice.id()).unwrap();
    assert_account_has_fungible_asset(alice_account, eth_faucet.id(), 1);
}
