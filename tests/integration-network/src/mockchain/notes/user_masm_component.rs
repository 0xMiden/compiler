//! A user-written Miden Assembly account component driven from Rust on the mock chain.
//!
//! The account component is the MASM project `tests/fixtures/components/acme-counter-masm`
//! (package `counter`, namespace `acme::counter::counter`), built with `midenc` in the test and
//! instantiated on the host with storage metadata the test writes, since a MASM package carries
//! none. Its count lives in one value slot and it provides the account's authentication
//! procedure. The Rust consumers depend on the MASM project by path with no WIT; the SDK macros
//! synthesize its interface from the package manifest. The tests prove that, through that
//! interface:
//! - a Rust note's calls (one with a record argument) update the count;
//! - a Rust transaction script reads a felt and a word result in the order the component returns
//!   them;
//! - a Rust account component calls the MASM component as its sibling in the same account;
//! - a Rust note on another account reads the count through foreign procedure invocation.

use std::{path::PathBuf, sync::Arc};

use miden_client::{
    account::component::InitStorageData, note::NoteTag, transaction::RawOutputNote,
};
use miden_core::{Felt, Word};
use miden_field_repr::{FromFeltRepr, ToFeltRepr};
use miden_mast_package::Package;
use miden_protocol::{
    account::{
        Account, AccountBuilder, AccountComponent, AccountId, AccountType, StorageSlot,
        StorageSlotName,
        auth::AuthScheme,
        component::{
            AccountComponentCode, AccountComponentMetadata, FeltSchema, SchemaType, StorageSchema,
            StorageSlotSchema, WordSchema,
        },
    },
    crypto::rand::RandomCoin,
    note::Note,
};
use miden_standards::{account::wallets::BasicWallet, testing::note::NoteBuilder};
use miden_testing::{AccountState, Auth, MockChain, MockChainBuilder};
use midenc_expect_test::expect;
use midenc_integration_test_support::{compile_masm_project, project, workspace_root};

use super::super::{
    sibling::build_masm_sibling_test_packages,
    support::{
        apply_script_args, compile_rust_package, execute_tx_measurements,
        note_cargo_toml_for_dependency, note_miden_project_toml_for_dependency, note_script_root,
        single_note_cycles, sysroot_with_standard_components, to_core_felts, to_field_felt,
        to_field_word, transaction_script_from_package, tx_script_processing_cycles,
    },
};

/// The name of the counter's package, which is also the name its consumers depend on it by.
const COUNTER_PACKAGE: &str = "counter";

/// The name of the value slot holding the count (`COUNT_SLOT` in the component's source).
const COUNT_SLOT: &str = "acme::counter::count";

/// The count every counter account starts with.
const INITIAL_COUNT: u64 = 10;

/// Host-side mirror of `TxScriptArgs` in `tests/fixtures/components/acme-counter-tx-script`.
#[derive(FromFeltRepr, ToFeltRepr)]
struct CounterArgs {
    /// The expected `get_count` result.
    count: miden_field::Felt,
    /// The expected `get_count_word` result.
    count_word: miden_field::Word,
}

/// The root of the counter's MASM project.
fn counter_project_root() -> PathBuf {
    workspace_root().join("tests/fixtures/components/acme-counter-masm")
}

/// The slot holding the count.
fn count_slot() -> StorageSlotName {
    StorageSlotName::new(COUNT_SLOT).expect("the count slot name is valid")
}

/// Builds the counter's MASM project and instantiates it as an account component whose count
/// starts at [`INITIAL_COUNT`].
///
/// The metadata names the component and describes its one value slot, `[count, 0, 0, 0]`.
fn counter_component() -> AccountComponent {
    let package: Arc<Package> =
        compile_masm_project(&counter_project_root(), &sysroot_with_standard_components());
    let count = WordSchema::new_value([
        FeltSchema::new_typed(SchemaType::native_felt(), "count"),
        FeltSchema::new_void(),
        FeltSchema::new_void(),
        FeltSchema::new_void(),
    ]);
    let schema =
        StorageSchema::new(vec![(count_slot(), StorageSlotSchema::value("the count", count))])
            .expect("the counter storage schema is valid");
    let metadata = AccountComponentMetadata::new("acme::counter")
        .with_description("A counter kept in one value slot")
        .with_storage_schema(schema);
    let slots = vec![StorageSlot::with_value(
        count_slot(),
        Word::from([Felt::new_unchecked(INITIAL_COUNT), Felt::ZERO, Felt::ZERO, Felt::ZERO]),
    )];
    AccountComponent::new(AccountComponentCode::from((*package).clone()), slots, metadata)
        .expect("the counter is a valid account component")
}

/// Adds an existing account made of the counter, which authenticates it, and the `extra`
/// components to the mock chain.
fn add_counter_account(
    builder: &mut MockChainBuilder,
    extra: impl IntoIterator<Item = AccountComponent>,
) -> Account {
    let account = AccountBuilder::new([7_u8; 32])
        .account_type(AccountType::Public)
        .with_component(counter_component())
        .with_components(extra)
        .build_existing()
        .expect("failed to build the counter account");
    builder.add_account(account.clone()).expect("failed to add the counter account");
    account
}

/// Adds a note running `note_package` with `note_storage`, sent to `target`, to the mock chain.
fn add_note(
    builder: &mut MockChainBuilder,
    target: AccountId,
    note_package: &Package,
    note_storage: Vec<Felt>,
) -> Note {
    let rng = RandomCoin::new(note_script_root(note_package));
    let note = NoteBuilder::new(target, rng)
        .package(note_package.clone())
        .note_storage(note_storage)
        .unwrap()
        .tag(NoteTag::with_account_target(target).into())
        .build()
        .unwrap();
    builder.add_output_note(RawOutputNote::Full(note.clone()));
    note
}

/// Builds the mock chain and proves two blocks, so the accounts and notes are committed.
fn build_chain(builder: MockChainBuilder) -> MockChain {
    let mut chain = builder.build().expect("failed to build the mock chain");
    chain.prove_next_block().unwrap();
    chain.prove_next_block().unwrap();
    chain
}

/// Consumes `note` with `account`, with `foreign` as the foreign accounts, and returns the
/// note's execution cycles.
fn consume_note(
    chain: &mut MockChain,
    account: &Account,
    note: &Note,
    foreign: &[AccountId],
) -> String {
    let foreign_inputs: Vec<_> = foreign
        .iter()
        .map(|id| chain.get_foreign_account_inputs(*id).unwrap())
        .collect();
    let mock_tx = chain
        .build_transaction(account.id())
        .authenticated_input_note(note.id())
        .foreign_accounts(foreign_inputs)
        .build()
        .unwrap();
    single_note_cycles(&execute_tx_measurements(chain, mock_tx)).to_string()
}

/// Asserts the committed count of `account`.
fn assert_count(chain: &MockChain, account: AccountId, expected: u64) {
    let word = chain
        .committed_account(account)
        .unwrap()
        .storage()
        .get_item(&count_slot())
        .expect("the counter account has the count slot");
    assert_eq!(word[0].as_canonical_u64(), expected, "count of {word:?}");
}

/// A Rust note calls `increment` and `add_to_count(Delta { add: 5, mul: 2 })` on the MASM
/// counter of its consuming account: the count goes from 10 to (10 + 1) * 2 + 5. Swapped record
/// fields would give (10 + 1) * 5 + 2, so the result pins the order of the record's fields.
#[test]
fn user_masm_counter_is_updated_by_a_rust_note() {
    let note_package = compile_rust_package("../fixtures/components/acme-counter-note", true);
    let mut builder = MockChain::builder();
    let account = add_counter_account(&mut builder, []);
    let note = add_note(&mut builder, account.id(), &note_package, Vec::new());
    let mut chain = build_chain(builder);
    assert_count(&chain, account.id(), INITIAL_COUNT);

    let cycles = consume_note(&mut chain, &account, &note, &[]);
    expect!["2233"].assert_eq(&cycles);
    assert_count(&chain, account.id(), (INITIAL_COUNT + 1) * 2 + 5);
}

/// A Rust transaction script reads `get_count` and the word `get_count_word` of the MASM counter
/// and checks them against the values the host passes in its arguments, so every element of the
/// word must come back where the component puts it.
#[test]
fn user_masm_counter_getters_are_read_by_a_rust_tx_script() {
    let tx_script_package =
        compile_rust_package("../fixtures/components/acme-counter-tx-script", true);
    let mut builder = MockChain::builder();
    let account = add_counter_account(&mut builder, []);
    let mut chain = build_chain(builder);

    let felt = |value: u64| Felt::new_unchecked(value);
    let args = CounterArgs {
        count: to_field_felt(felt(INITIAL_COUNT)),
        count_word: to_field_word([0, 1, 2, 3].map(|offset| felt(INITIAL_COUNT + offset))),
    };
    let mock_tx_builder = chain
        .build_transaction(account.id())
        .tx_script(transaction_script_from_package(&tx_script_package));
    let mock_tx = apply_script_args(mock_tx_builder, &args).build().unwrap();
    let measurements = execute_tx_measurements(&mut chain, mock_tx);
    expect!["3413"].assert_eq(tx_script_processing_cycles(&measurements));
    assert_count(&chain, account.id(), INITIAL_COUNT);
}

/// A Rust account component calls `increment` twice on its sibling MASM counter in the same
/// account, then returns `get_count`; a note drives it and checks the result, and the committed
/// count is 10 + 2.
#[test]
fn user_masm_counter_is_a_sibling_of_a_rust_component() {
    let counter_root = counter_project_root();
    let (caller_package, note_package) = build_masm_sibling_test_packages(
        "user_masm_sibling",
        &[(COUNTER_PACKAGE, &counter_root)],
        SIBLING_CALLER_SOURCE,
        SIBLING_NOTE_SOURCE,
    );
    let caller =
        AccountComponent::from_package((*caller_package).clone(), &InitStorageData::default())
            .unwrap();

    let mut builder = MockChain::builder();
    let account = add_counter_account(&mut builder, [caller]);
    let note = add_note(&mut builder, account.id(), &note_package, Vec::new());
    let mut chain = build_chain(builder);

    let cycles = consume_note(&mut chain, &account, &note, &[]);
    expect!["3307"].assert_eq(&cycles);
    assert_count(&chain, account.id(), INITIAL_COUNT + 2);
}

/// A Rust note consumed by a wallet reads `get_count` of the MASM counter account through
/// foreign procedure invocation and checks it against the count carried in its storage.
#[test]
fn user_masm_counter_is_read_through_fpi_by_a_rust_note() {
    let note_name = "user-masm-fpi-note";
    let note_package_name = format!("miden:{note_name}");
    let counter_root = counter_project_root();
    let note_project = project(note_name)
        .file(
            "miden-project.toml",
            &note_miden_project_toml_for_dependency(
                note_name,
                &note_package_name,
                COUNTER_PACKAGE,
                &counter_root,
            ),
        )
        .file(
            "Cargo.toml",
            &note_cargo_toml_for_dependency(note_name, COUNTER_PACKAGE, &counter_root),
        )
        .file("src/lib.rs", FPI_NOTE_SOURCE)
        .build();
    let note_package = compile_rust_package(note_project.root(), true);

    let mut builder = MockChain::builder();
    let counter = add_counter_account(&mut builder, []);
    let wallet = builder
        .add_account_from_builder(
            Auth::BasicAuth {
                auth_scheme: AuthScheme::Falcon512Poseidon2,
            },
            AccountBuilder::new([8_u8; 32])
                .account_type(AccountType::Public)
                .with_component(BasicWallet),
            AccountState::Exists,
        )
        .expect("failed to add the wallet account");
    let mut note_storage = to_core_felts(&counter.id());
    note_storage.push(Felt::new_unchecked(INITIAL_COUNT));
    let note = add_note(&mut builder, wallet.id(), &note_package, note_storage);
    let mut chain = build_chain(builder);

    let cycles = consume_note(&mut chain, &wallet, &note, &[counter.id()]);
    expect!["3264"].assert_eq(&cycles);
    assert_count(&chain, counter.id(), INITIAL_COUNT);
}

/// Rust account component calling its sibling MASM counter.
const SIBLING_CALLER_SOURCE: &str = r#"
#![no_std]
#![feature(alloc_error_handler)]

use miden::{component, component_storage, native_account::NativeAccount, Felt};

/// Storage-less component which forwards calls to its sibling MASM counter.
#[component_storage]
struct CallerAccountStorage;

/// Account component which calls the sibling MASM counter of the same account.
#[component(counter::Counter)]
trait CallerAccount: NativeAccount + Counter {
    /// Increments the sibling counter twice and returns its count.
    #[account_procedure]
    fn bump_twice(&mut self) -> Felt;
}

#[component]
impl CallerAccount for CallerAccountStorage {
    fn bump_twice(&mut self) -> Felt {
        self.increment();
        self.increment();
        self.get_count()
    }
}
"#;

/// Note making the caller component bump its sibling counter.
const SIBLING_NOTE_SOURCE: &str = r#"
#![no_std]
#![feature(alloc_error_handler)]

use miden::*;

/// Native (active) account of the note: the caller component account.
#[account(user_masm_sibling_caller_account::CallerAccount)]
struct Account;

/// Storage-less note driving the caller component.
#[note]
struct SiblingNote;

#[note]
impl SiblingNote {
    /// Bumps the sibling counter twice and checks the count it reports.
    #[note_script]
    pub fn run(self, _arg: Word, account: &mut Account) {
        assert_eq(account.bump_twice(), felt!(12));
    }
}
"#;

/// Note reading the count of a foreign MASM counter account.
const FPI_NOTE_SOURCE: &str = r#"
#![no_std]
#![feature(alloc_error_handler)]

use miden::*;

/// Foreign account of the note: exposes the procedures of the MASM `counter` component.
#[account(counter::Counter)]
struct CounterAccount;

/// Note carrying the counter account to read and the count it must report.
#[note]
struct CounterReader {
    /// The counter account.
    counter_account_id: AccountId,
    /// The count the counter account must report.
    expected_count: Felt,
}

#[note]
impl CounterReader {
    /// Reads the foreign count and checks it.
    #[note_script]
    pub fn run(self, _arg: Word) {
        let counter = CounterAccount::new(self.counter_account_id);
        assert_eq(counter.get_count(), self.expected_count);
    }
}
"#;
