//! Foreign procedure invocation tests for variant arguments in the indirect call shape.
//!
//! The record fields plus the option lanes push the flattened parameter count past the
//! canonical ABI's sixteen-value threshold, so the caller passes one argument tuple pointer
//! and the FPI wrapper reloads the variant with a discriminant switch.

use miden_client::Word;
use miden_core::Felt;

use super::super::common::{build_fpi_test_packages, execute_counter_caller_note};

/// Deploys a counter contract and consumes a note which passes a tupled variant through FPI.
#[test]
pub fn variant_indirect_args() {
    let (counter_package, caller_note_package, counter_storage_slot) = build_fpi_test_packages(
        "variant_indirect_args",
        COUNTER_CONTRACT_SOURCE,
        COUNTER_CALLER_SOURCE,
    );

    execute_counter_caller_note(
        counter_package,
        caller_note_package,
        counter_storage_slot,
        variant_indirect_args_storage_key(),
        100,
    );
}

/// Returns the non-zero storage key used by the indirect variant FPI test.
fn variant_indirect_args_storage_key() -> Word {
    Word::new([
        Felt::new(71).unwrap(),
        Felt::new(72).unwrap(),
        Felt::new(73).unwrap(),
        Felt::new(74).unwrap(),
    ])
}

/// Minimal counter account component source used by the indirect variant FPI test.
const COUNTER_CONTRACT_SOURCE: &str = r#"
#![no_std]
#![feature(alloc_error_handler)]

use miden::{component, component_storage, export_type, Felt, StorageMap, Word};

/// Record whose ten fields fill most of the flattened parameter budget.
#[export_type]
pub struct TenU32Record {
    /// First single-word integer field.
    pub f0: u32,
    /// Second single-word integer field.
    pub f1: u32,
    /// Third single-word integer field.
    pub f2: u32,
    /// Fourth single-word integer field.
    pub f3: u32,
    /// Fifth single-word integer field.
    pub f4: u32,
    /// Sixth single-word integer field.
    pub f5: u32,
    /// Seventh single-word integer field.
    pub f6: u32,
    /// Eighth single-word integer field.
    pub f7: u32,
    /// Ninth single-word integer field.
    pub f8: u32,
    /// Tenth single-word integer field.
    pub f9: u32,
}

/// Account component whose FPI method folds a record into an optional value.
#[component_storage]
struct CounterContractStorage {
    /// Storage map included so the shared FPI mock-chain helper can initialize the account.
    #[storage(description = "counter contract storage map")]
    count_map: StorageMap<Word, Felt>,
}

/// Account component whose FPI method folds a record into an optional value.
#[component]
trait CounterContract {
    /// Adds the record's field sum to the optional value, preserving `None`.
    fn pick_sum(&self, input: TenU32Record, choice: Option<u32>) -> Option<u32>;
}

#[component]
impl CounterContract for CounterContractStorage {
    /// Adds the record's field sum to the optional value, preserving `None`.
    fn pick_sum(&self, input: TenU32Record, choice: Option<u32>) -> Option<u32> {
        let _ = &self.count_map;
        let sum = input.f0
            + input.f1
            + input.f2
            + input.f3
            + input.f4
            + input.f5
            + input.f6
            + input.f7
            + input.f8
            + input.f9;
        choice.map(|value| value + sum)
    }
}
"#;

/// Minimal note script source which exercises a tupled variant argument over FPI.
const COUNTER_CALLER_SOURCE: &str = r#"
#![no_std]
#![feature(alloc_error_handler)]

use miden::*;

use crate::bindings::miden::variant_indirect_args_account::counter_contract::TenU32Record;
#[account(variant_indirect_args_account::CounterContract)]
struct Counter;

/// Note script input containing the foreign counter account id.
#[note]
struct CounterCaller {
    /// Account id of the counter contract to invoke through FPI.
    counter_account_id: AccountId,
}

#[note]
impl CounterCaller {
    /// Checks that a variant argument loaded from the argument tuple crosses the FPI boundary.
    #[note_script]
    pub fn run(self, _arg: Word) {
        let count_acc = Counter::new(self.counter_account_id);
        let record = TenU32Record {
            f0: 1,
            f1: 2,
            f2: 3,
            f3: 4,
            f4: 5,
            f5: 6,
            f6: 7,
            f7: 8,
            f8: 9,
            f9: 10,
        };

        match count_acc.pick_sum(record, Some(100)) {
            Some(value) => assert_eq(Felt::from_u32(value), Felt::from_u32(155)),
            None => assert_eq(felt!(0), felt!(1)),
        }

        let record = TenU32Record {
            f0: 1,
            f1: 2,
            f2: 3,
            f3: 4,
            f4: 5,
            f5: 6,
            f6: 7,
            f7: 8,
            f8: 9,
            f9: 10,
        };
        match count_acc.pick_sum(record, None) {
            None => (),
            Some(_) => assert_eq(felt!(0), felt!(1)),
        }
    }
}
"#;
