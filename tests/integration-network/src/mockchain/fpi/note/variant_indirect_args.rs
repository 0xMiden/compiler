//! Foreign procedure invocation tests for variant arguments in the indirect call shape.
//!
//! The record fields plus the variant lanes push the flattened parameter count past the
//! canonical ABI's sixteen-value threshold, so the caller passes one argument tuple pointer
//! and the FPI wrapper reloads each variant — an option and a mixed-width (`u64` + `felt`)
//! payload variant — with a discriminant switch.

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

/// Variant whose cases join a double-word integer lane with a felt lane.
#[export_type]
pub enum MixedPayload {
    /// Carries a double-word integer value.
    Wide(u64),
    /// Carries a single felt value.
    Scalar(Felt),
}

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
    /// Adds the record's field sum to the payload of either mixed-width variant case.
    fn fold_mixed(&self, input: TenU32Record, payload: MixedPayload) -> MixedPayload;
}

#[component]
impl CounterContract for CounterContractStorage {
    /// Adds the record's field sum to the optional value, preserving `None`.
    fn pick_sum(&self, input: TenU32Record, choice: Option<u32>) -> Option<u32> {
        let _ = &self.count_map;
        choice.map(|value| value + field_sum(&input))
    }

    /// Adds the record's field sum to the payload of either mixed-width variant case.
    fn fold_mixed(&self, input: TenU32Record, payload: MixedPayload) -> MixedPayload {
        let _ = &self.count_map;
        let sum = field_sum(&input);
        match payload {
            MixedPayload::Wide(value) => MixedPayload::Wide(value + sum as u64),
            MixedPayload::Scalar(value) => MixedPayload::Scalar(value + Felt::from_u32(sum)),
        }
    }
}

/// Returns the sum of all record fields.
fn field_sum(input: &TenU32Record) -> u32 {
    input.f0
        + input.f1
        + input.f2
        + input.f3
        + input.f4
        + input.f5
        + input.f6
        + input.f7
        + input.f8
        + input.f9
}
"#;

/// Minimal note script source which exercises a tupled variant argument over FPI.
const COUNTER_CALLER_SOURCE: &str = r#"
#![no_std]
#![feature(alloc_error_handler)]

use miden::*;

use crate::bindings::miden::variant_indirect_args_account::counter_contract::{
    MixedPayload, TenU32Record,
};
#[account(variant_indirect_args_account::CounterContract)]
struct Counter;

/// Sum of the record fields sent by every call.
const FIELD_SUM: u32 = 55;
/// Double-word value whose limbs both stay non-zero across the fold.
const WIDE_VALUE: u64 = 0x0000_0003_0000_0007;

/// Note script input containing the foreign counter account id.
#[note]
struct CounterCaller {
    /// Account id of the counter contract to invoke through FPI.
    counter_account_id: AccountId,
}

#[note]
impl CounterCaller {
    /// Checks that variant arguments loaded from the argument tuple cross the FPI boundary.
    #[note_script]
    pub fn run(self, _arg: Word) {
        let count_acc = Counter::new(self.counter_account_id);

        match count_acc.pick_sum(test_record(), Some(100)) {
            Some(value) => assert_eq(Felt::from_u32(value), Felt::from_u32(100 + FIELD_SUM)),
            None => assert_eq(felt!(0), felt!(1)),
        }
        match count_acc.pick_sum(test_record(), None) {
            None => (),
            Some(_) => assert_eq(felt!(0), felt!(1)),
        }

        match count_acc.fold_mixed(test_record(), MixedPayload::Wide(WIDE_VALUE)) {
            MixedPayload::Wide(value) => assert_u64_eq(value, WIDE_VALUE + FIELD_SUM as u64),
            MixedPayload::Scalar(_) => assert_eq(felt!(0), felt!(1)),
        }
        match count_acc.fold_mixed(test_record(), MixedPayload::Scalar(felt!(9))) {
            MixedPayload::Scalar(value) => assert_eq(value, Felt::from_u32(9 + FIELD_SUM)),
            MixedPayload::Wide(_) => assert_eq(felt!(0), felt!(1)),
        }
    }
}

/// Returns the record whose fields sum to `FIELD_SUM`.
fn test_record() -> TenU32Record {
    TenU32Record {
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
    }
}

/// Asserts that two u64 values contain the same two u32 limbs.
fn assert_u64_eq(actual: u64, expected: u64) {
    assert_eq(
        Felt::from_u32((actual & 0xffff_ffff) as u32),
        Felt::from_u32((expected & 0xffff_ffff) as u32),
    );
    assert_eq(
        Felt::from_u32((actual >> 32) as u32),
        Felt::from_u32((expected >> 32) as u32),
    );
}
"#;
