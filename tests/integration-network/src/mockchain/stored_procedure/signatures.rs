//! Stored-procedure signatures at the edges of what a dispatched call supports: a narrow scalar
//! (`bool`) argument, and an argument list filling the twelve-field-element budget.

use miden_client::{
    account::{
        AccountComponent,
        component::{BasicWallet, InitStorageData, StorageValueName},
    },
    note::NoteTag,
    transaction::RawOutputNote,
};
use miden_core::{Felt, Word};
use miden_protocol::{
    account::{AccountBuilder, AccountType, auth::AuthScheme},
    crypto::rand::RandomCoin,
};
use miden_standards::testing::note::NoteBuilder;
use miden_testing::{AccountState, Auth, MockChain};
use midenc_expect_test::expect;

use super::{
    super::support::{execute_tx_measurements, note_script_root, single_note_cycles},
    common::{
        DispatchProjectNames, assert_word_value_slots, build_dispatcher_package,
        build_note_package, build_target_package, lifted_export_root,
    },
};

/// Sum the dispatched `sum6` procedure returns: five small addends plus `2^40`, so both 32-bit
/// halves of the `u64` result are non-zero.
const EXPECTED_SUM: u64 = 15 + (1 << 40);

/// Deploys a target component exporting a procedure with a leading `bool` argument and one
/// taking six `u64`s (the full twelve-field-element argument budget), and a dispatcher whose two
/// stored-procedure slots hold their lifted export roots. A note drives the dispatcher, which
/// checks both results in-guest and records the sum's halves in a plain value slot the host
/// asserts after the transaction.
#[test]
fn dispatches_bool_and_twelve_felt_signatures() {
    let names = DispatchProjectNames::new("stored_procedure_signatures");
    let (_target_project, target_package) =
        build_target_package(&names, "signature-target", TARGET_SOURCE);
    let (dispatcher_project, dispatcher_package) =
        build_dispatcher_package(&names, DISPATCHER_SOURCE);
    let note_package = build_note_package(&names, "note", dispatcher_project.root(), NOTE_SOURCE);

    let scale_slot = names.dispatcher_slot("scale");
    let sum6_slot = names.dispatcher_slot("sum6");
    let last_sum_slot = names.dispatcher_slot("last_sum");
    assert_word_value_slots(&dispatcher_package, &[scale_slot.clone(), sum6_slot.clone()]);

    let target_component =
        AccountComponent::from_package(&target_package, &InitStorageData::default()).unwrap();
    let dispatcher_component = {
        let mut init_storage_data = InitStorageData::default();
        init_storage_data
            .insert_value(
                StorageValueName::from_slot_name(&scale_slot),
                lifted_export_root(&target_package, "scale"),
            )
            .unwrap();
        init_storage_data
            .insert_value(
                StorageValueName::from_slot_name(&sum6_slot),
                lifted_export_root(&target_package, "sum6"),
            )
            .unwrap();
        // Every value slot needs an initial value, including the one the dispatch writes.
        init_storage_data
            .insert_value(StorageValueName::from_slot_name(&last_sum_slot), Word::default())
            .unwrap();
        AccountComponent::from_package(&dispatcher_package, &init_storage_data).unwrap()
    };

    let mut builder = MockChain::builder();
    let account_builder = AccountBuilder::new([4_u8; 32])
        .account_type(AccountType::Public)
        .with_component(BasicWallet)
        .with_component(target_component)
        .with_component(dispatcher_component);
    let account = builder
        .add_account_from_builder(
            Auth::BasicAuth {
                auth_scheme: AuthScheme::Falcon512Poseidon2,
            },
            account_builder,
            AccountState::Exists,
        )
        .expect("failed to add the dispatch account to the mock chain builder");

    let rng = RandomCoin::new(note_script_root(note_package.as_ref()));
    let note = NoteBuilder::new(account.id(), rng)
        .package((*note_package).clone())
        .tag(NoteTag::with_account_target(account.id()).into())
        .build()
        .unwrap();
    builder.add_output_note(RawOutputNote::Full(note.clone()));

    let mut chain = builder.build().expect("failed to build mock chain");
    chain.prove_next_block().unwrap();
    chain.prove_next_block().unwrap();

    let mock_tx = chain
        .build_transaction(account.clone())
        .authenticated_input_note(note.id())
        .build()
        .unwrap();
    let tx_measurements = execute_tx_measurements(&mut chain, mock_tx);
    expect!["6436"].assert_eq(single_note_cycles(&tx_measurements));

    // The dispatcher recorded the low and high halves of the `u64` sum it received.
    let recorded = chain
        .committed_account(account.id())
        .unwrap()
        .storage()
        .get_item(&last_sum_slot)
        .expect("dispatcher should expose the recorded-sum slot");
    let expected = Word::new([
        Felt::new(EXPECTED_SUM & 0xffff_ffff).unwrap(),
        Felt::new(EXPECTED_SUM >> 32).unwrap(),
        Felt::ZERO,
        Felt::ZERO,
    ]);
    assert_eq!(recorded, expected, "recorded sum halves mismatch");
}

/// Target component exporting the two procedures dispatched through stored roots.
const TARGET_SOURCE: &str = r#"
#![no_std]
#![feature(alloc_error_handler)]

use miden::{component, component_storage, Felt};

/// Storage-less sibling account component: the procedures are pure.
#[component_storage]
struct SignatureTargetStorage;

/// Sibling account component exporting procedures with a narrow-scalar argument and with the
/// widest argument list a dispatched call supports.
#[component]
trait SignatureTarget {
    /// Returns `amount` doubled when `enabled`, and `amount` unchanged otherwise.
    #[account_procedure]
    fn scale(&self, enabled: bool, amount: Felt) -> Felt;
    /// Returns the sum of six 64-bit values.
    #[account_procedure]
    fn sum6(&self, a: u64, b: u64, c: u64, d: u64, e: u64, f: u64) -> u64;
}

#[component]
impl SignatureTarget for SignatureTargetStorage {
    fn scale(&self, enabled: bool, amount: Felt) -> Felt {
        if enabled { amount + amount } else { amount }
    }

    fn sum6(&self, a: u64, b: u64, c: u64, d: u64, e: u64, f: u64) -> u64 {
        a + b + c + d + e + f
    }
}
"#;

/// Dispatcher component: its call targets live in storage, so it depends on no other package.
const DISPATCHER_SOURCE: &str = r#"
#![no_std]
#![feature(alloc_error_handler)]

use miden::{assert_eq, component, component_storage, felt, Felt, StorageValue, StoredProcedure, Word};

/// Storage holding the roots of the dispatched procedures and the recorded sum.
#[component_storage]
struct DispatcherStorage {
    #[storage(description = "root of a procedure scaling a felt under a boolean flag")]
    scale: StorageValue<StoredProcedure<fn(enabled: bool, amount: Felt) -> Felt>>,
    #[storage(description = "root of a procedure summing six 64-bit values")]
    sum6: StorageValue<StoredProcedure<fn(a: u64, b: u64, c: u64, d: u64, e: u64, f: u64) -> u64>>,
    #[storage(description = "low and high 32-bit halves of the last dispatched sum")]
    last_sum: StorageValue<Word>,
}

/// Account component dispatching both stored procedures with fixed arguments.
#[component]
trait Dispatcher {
    /// Dispatches both stored procedures, checks their results, records the sum in storage, and
    /// returns the scaled value.
    #[account_procedure]
    fn dispatch(&mut self) -> Felt;
}

#[component]
impl Dispatcher for DispatcherStorage {
    fn dispatch(&mut self) -> Felt {
        // A `bool` argument is a narrow scalar: it occupies one field element on the wire, and
        // both of its values must survive the dispatch.
        let scale = self.scale.get();
        let scaled = scale.call(true, felt!(21));
        assert_eq(scaled, felt!(42));
        assert_eq(scale.call(false, felt!(21)), felt!(21));

        // Six `u64`s are twelve field elements: the whole argument budget of a dispatched call
        // whose result fits one flat value. `2^40` keeps both halves of the result non-zero.
        let sum = self.sum6.get().call(1, 2, 3, 4, 5, 1 << 40);
        let expected: u64 = 15 + (1 << 40);
        assert_eq(if sum == expected { felt!(1) } else { felt!(0) }, felt!(1));

        // Recorded for the host, which cannot observe the in-guest assertions above.
        self.last_sum.set(Word::new([
            Felt::from(sum as u32),
            Felt::from((sum >> 32) as u32),
            felt!(0),
            felt!(0),
        ]));

        scaled
    }
}
"#;

/// Note script triggering the dispatch on the active account.
const NOTE_SOURCE: &str = r#"
#![no_std]
#![feature(alloc_error_handler)]

use miden::*;

/// Native (active) account of the note: the dispatcher component account.
#[account(stored_procedure_signatures_dispatcher_account::Dispatcher)]
struct Account;

/// Input-less trigger note.
#[note]
struct DispatchNote;

#[note]
impl DispatchNote {
    /// Dispatches both stored procedures and checks the returned scaled value.
    #[note_script]
    pub fn run(self, _arg: Word, account: &mut Account) {
        assert_eq(account.dispatch(), felt!(42));
    }
}
"#;
