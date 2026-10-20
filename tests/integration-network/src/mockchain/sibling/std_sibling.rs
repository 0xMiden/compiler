//! A Rust account component calling standard (MASM) account components of the same account.
//!
//! The caller component declares `miden-standards` packages from the sysroot as registry
//! dependencies and reaches them through the sibling traits `#[component(pkg::Interface, ...)]`
//! generates from the packages alone, with no hand-written WIT. The tests prove a sibling call
//! from a Rust component into a MASM component moves an asset into the account vault, and that a
//! multi-felt result (an account id) comes back across a sibling call in the right order.

use miden_client::{
    account::component::InitStorageData,
    asset::{Asset, FungibleAsset},
    note::Note,
    transaction::RawOutputNote,
};
use miden_core::Felt;
use miden_mast_package::Package;
use miden_protocol::{
    account::{Account, AccountBuilder, AccountComponent, AccountId, AccountType},
    crypto::rand::RandomCoin,
};
use miden_standards::{
    account::{access::Ownable2Step, wallets::BasicWallet},
    testing::note::NoteBuilder,
};
use miden_testing::{AccountState, MockChain, MockChainBuilder};
use midenc_expect_test::expect;

use super::{
    super::support::{
        assert_account_has_fungible_asset, assert_counter_storage_at_key, basic_auth,
        build_send_notes_script, execute_tx, execute_tx_measurements, note_script_root,
        single_note_cycles, to_core_felts,
    },
    common::build_std_sibling_test_packages,
    counter_storage_key,
};

/// Registry dependency on the standard basic wallet package.
const STD_BASIC_WALLET: (&str, &str) = ("miden-standards-wallets-basic-wallet", "0.17.0");

/// Registry dependency on the standard two-step ownership package.
const STD_OWNABLE2STEP: (&str, &str) = ("miden-standards-access-ownable2step", "0.17.0");

/// Amount of the fungible asset the faucet mints into the trigger note.
const MINT_AMOUNT: u64 = 100_000;

/// Deploys one account made of the standard basic wallet and a Rust caller component, then
/// consumes a minted note whose script makes the caller move the note's asset into the vault
/// through the sibling wallet's `receive_asset`. The vault balance and the caller's counter prove
/// the Rust-to-MASM sibling call ran and both components' effects committed.
#[test]
fn std_wallet_sibling() {
    let (caller_package, note_package, counter_slot) = build_std_sibling_test_packages(
        "std_wallet_sibling",
        &[STD_BASIC_WALLET],
        WALLET_CALLER_SOURCE,
        WALLET_NOTE_SOURCE,
    );

    let mut builder = MockChain::builder();
    let faucet_id = builder
        .add_existing_basic_faucet(basic_auth(), "TEST", 1_000_000_000, None)
        .unwrap()
        .id();
    let account = add_caller_account(&mut builder, &caller_package, []);
    let (mut chain, note) = mint_note(builder, faucet_id, &note_package, Vec::new());

    let cycles = consume_note(&mut chain, &account, faucet_id, &note);
    expect!["6813"].assert_eq(&cycles);

    let committed = chain.committed_account(account.id()).unwrap();
    assert_account_has_fungible_asset(committed, faucet_id, MINT_AMOUNT);
    assert_counter_storage_at_key(committed.storage(), &counter_slot, counter_storage_key(), 1);
}

/// Deploys one account made of the standard basic wallet, the standard `ownable2step` component
/// configured with an owner, and a Rust caller component depending on both, then consumes a
/// minted note carrying the expected owner id. The caller reads the owner through the sibling
/// `get_owner` and checks both felts of the account id before receiving the asset, so the
/// transaction succeeds only if the two-felt result crosses the sibling call in order.
#[test]
fn std_multi_sibling() {
    let (caller_package, note_package, counter_slot) = build_std_sibling_test_packages(
        "std_multi_sibling",
        &[STD_BASIC_WALLET, STD_OWNABLE2STEP],
        OWNER_CALLER_SOURCE,
        OWNER_NOTE_SOURCE,
    );

    let mut builder = MockChain::builder();
    let faucet_id = builder
        .add_existing_basic_faucet(basic_auth(), "TEST", 1_000_000_000, None)
        .unwrap()
        .id();
    let owner_id = builder.add_existing_wallet(basic_auth()).unwrap().id();
    let account =
        add_caller_account(&mut builder, &caller_package, [Ownable2Step::new(owner_id).into()]);
    let (mut chain, note) = mint_note(builder, faucet_id, &note_package, to_core_felts(&owner_id));

    let cycles = consume_note(&mut chain, &account, faucet_id, &note);
    expect!["8083"].assert_eq(&cycles);

    let committed = chain.committed_account(account.id()).unwrap();
    assert_account_has_fungible_asset(committed, faucet_id, MINT_AMOUNT);
    assert_counter_storage_at_key(committed.storage(), &counter_slot, counter_storage_key(), 1);
}

/// Adds an account made of the standard basic wallet, the `extra` standard components, and the
/// caller component to the mock chain.
fn add_caller_account(
    builder: &mut MockChainBuilder,
    caller_package: &Package,
    extra: impl IntoIterator<Item = AccountComponent>,
) -> Account {
    let caller_component =
        AccountComponent::from_package(caller_package.clone(), &InitStorageData::default())
            .unwrap();
    let mut account_builder = AccountBuilder::new([1_u8; 32])
        .account_type(AccountType::Public)
        .with_component(BasicWallet);
    for component in extra {
        account_builder = account_builder.with_component(component);
    }
    let account_builder = account_builder.with_component(caller_component);
    builder
        .add_account_from_builder(basic_auth(), account_builder, AccountState::Exists)
        .expect("failed to add the standard-sibling account to the mock chain builder")
}

/// Builds the mock chain and has the faucet mint [`MINT_AMOUNT`] into a note running the note
/// package with the given note storage; returns the chain and the minted note.
fn mint_note(
    builder: MockChainBuilder,
    faucet_id: AccountId,
    note_package: &Package,
    note_storage: Vec<Felt>,
) -> (MockChain, Note) {
    let mut chain = builder.build().unwrap();
    chain.prove_next_block().unwrap();
    chain.prove_next_block().unwrap();

    let asset = FungibleAsset::new(faucet_id, MINT_AMOUNT).unwrap();
    let mut rng = RandomCoin::new(note_script_root(note_package));
    let note = NoteBuilder::new(faucet_id, &mut rng)
        .package(note_package.clone())
        .add_assets([Asset::from(asset)])
        .note_storage(note_storage)
        .unwrap()
        .build()
        .unwrap();

    let faucet_account = chain.committed_account(faucet_id).unwrap().clone();
    let mint_script = build_send_notes_script(&faucet_account, std::slice::from_ref(&note));
    let mock_tx = chain
        .build_transaction(faucet_id)
        .send_notes_script(&mint_script)
        .expected_output_notes(vec![RawOutputNote::Full(note.clone())])
        .build()
        .unwrap();
    execute_tx(&mut chain, mock_tx);
    (chain, note)
}

/// Consumes `note` with `account` and returns the note's execution cycles.
fn consume_note(
    chain: &mut MockChain,
    account: &Account,
    faucet_id: AccountId,
    note: &Note,
) -> String {
    let faucet_inputs = chain.get_foreign_account_inputs(faucet_id).unwrap();
    let mock_tx = chain
        .build_transaction(account.id())
        .authenticated_input_note(note.id())
        .foreign_accounts(vec![faucet_inputs])
        .build()
        .unwrap();
    let measurements = execute_tx_measurements(chain, mock_tx);
    single_note_cycles(&measurements).to_string()
}

/// Caller component receiving assets through its sibling standard basic wallet.
const WALLET_CALLER_SOURCE: &str = r#"
#![no_std]
#![feature(alloc_error_handler)]

use miden::{component, component_storage, felt, native_account::NativeAccount, Asset, Felt, StorageMap, Word};

/// Storage of the caller component: how many assets it received.
#[component_storage]
struct CallerAccountStorage {
    /// Storage map holding the received-asset counter.
    #[storage(description = "received-asset counter storage map")]
    count_map: StorageMap<Word, Felt>,
}

/// Account component which receives assets through the sibling standard basic wallet.
#[component(miden_standards_wallets_basic_wallet::BasicWallet)]
trait CallerAccount: NativeAccount + BasicWallet {
    /// Receives the asset through the sibling standard wallet and returns the updated count of
    /// received assets.
    #[account_procedure]
    fn receive_and_count(&mut self, asset: Asset) -> Felt;
}

#[component]
impl CallerAccount for CallerAccountStorage {
    fn receive_and_count(&mut self, asset: Asset) -> Felt {
        self.receive_asset(asset);
        let key = Word::new([felt!(13), felt!(21), felt!(34), felt!(55)]);
        let count = self.count_map.get(key) + felt!(1);
        self.count_map.set(key, count);
        count
    }
}
"#;

/// Note script handing every note asset to the wallet caller component.
const WALLET_NOTE_SOURCE: &str = r#"
#![no_std]
#![feature(alloc_error_handler)]

use miden::*;

/// Native (active) account of the note: the caller component account.
#[account(std_wallet_sibling_caller_account::CallerAccount)]
struct Account;

/// Storage-less note moving its assets into the account through the caller component.
#[note]
struct StdWalletSiblingNote;

#[note]
impl StdWalletSiblingNote {
    /// Receives every note asset through the caller component.
    #[note_script]
    pub fn run(self, _arg: Word, account: &mut Account) {
        let mut expected = felt!(0);
        for asset in self.get_initial_assets() {
            expected = expected + felt!(1);
            assert_eq(account.receive_and_count(asset), expected);
        }
    }
}
"#;

/// Caller component receiving assets only when the sibling `ownable2step` owner matches.
const OWNER_CALLER_SOURCE: &str = r#"
#![no_std]
#![feature(alloc_error_handler)]

use miden::{assert_eq, component, component_storage, felt, native_account::NativeAccount, AccountId, Asset, Felt, StorageMap, Word};

/// Storage of the caller component: how many assets it received.
#[component_storage]
struct CallerAccountStorage {
    /// Storage map holding the received-asset counter.
    #[storage(description = "received-asset counter storage map")]
    count_map: StorageMap<Word, Felt>,
}

/// Account component which checks the sibling owner, then receives through the sibling wallet.
#[component(miden_standards_wallets_basic_wallet::BasicWallet, miden_standards_access_ownable2step::Ownable2step)]
trait CallerAccount: NativeAccount + BasicWallet + Ownable2step {
    /// Asserts the sibling `ownable2step` owner is `expected`, receives the asset through the
    /// sibling standard wallet, and returns the updated count of received assets.
    #[account_procedure]
    fn receive_if_owner(&mut self, asset: Asset, expected: AccountId) -> Felt;
}

#[component]
impl CallerAccount for CallerAccountStorage {
    fn receive_if_owner(&mut self, asset: Asset, expected: AccountId) -> Felt {
        let owner = self.get_owner();
        assert_eq(owner.suffix, expected.suffix);
        assert_eq(owner.prefix, expected.prefix);
        self.receive_asset(asset);
        let key = Word::new([felt!(13), felt!(21), felt!(34), felt!(55)]);
        let count = self.count_map.get(key) + felt!(1);
        self.count_map.set(key, count);
        count
    }
}
"#;

/// Note script handing every note asset to the owner-checking caller component.
const OWNER_NOTE_SOURCE: &str = r#"
#![no_std]
#![feature(alloc_error_handler)]

use miden::*;

/// Native (active) account of the note: the caller component account.
#[account(std_multi_sibling_caller_account::CallerAccount)]
struct Account;

/// Note carrying the owner the account's `ownable2step` component must report.
#[note]
struct StdMultiSiblingNote {
    /// The expected owner of the consuming account.
    expected_owner: AccountId,
}

#[note]
impl StdMultiSiblingNote {
    /// Receives every note asset through the caller component, which checks the owner first.
    #[note_script]
    pub fn run(self, _arg: Word, account: &mut Account) {
        let mut expected = felt!(0);
        for asset in self.get_initial_assets() {
            expected = expected + felt!(1);
            assert_eq(account.receive_if_owner(asset, self.expected_owner), expected);
        }
    }
}
"#;
