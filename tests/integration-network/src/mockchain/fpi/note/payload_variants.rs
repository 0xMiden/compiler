//! Foreign procedure invocation tests for payload-carrying variants in direct call shapes.
//!
//! Covers WIT `option`, `result`, and a custom payload variant with mixed-width lanes
//! (`u64` + `felt`) crossing the FPI boundary in both directions.

use miden_client::Word;
use miden_core::Felt;

use super::super::common::{build_fpi_test_packages, execute_counter_caller_note};

/// Deploys a counter contract and consumes a note which passes payload variants through FPI.
#[test]
pub fn payload_variants() {
    let (counter_package, caller_note_package, counter_storage_slot) =
        build_fpi_test_packages("payload_variants", COUNTER_CONTRACT_SOURCE, COUNTER_CALLER_SOURCE);

    execute_counter_caller_note(
        counter_package,
        caller_note_package,
        counter_storage_slot,
        payload_variants_storage_key(),
        100,
    );
}

/// Returns the non-zero storage key used by the payload variant FPI test.
fn payload_variants_storage_key() -> Word {
    Word::new([
        Felt::new(61).unwrap(),
        Felt::new(62).unwrap(),
        Felt::new(63).unwrap(),
        Felt::new(64).unwrap(),
    ])
}

/// Minimal counter account component source used by the payload variant FPI test.
const COUNTER_CONTRACT_SOURCE: &str = r#"
#![no_std]
#![feature(alloc_error_handler)]

use miden::{component, component_storage, export_type, felt, Felt, StorageMap, Word};

/// Variant whose cases join a double-word integer lane with a felt lane.
#[export_type]
pub enum MixedPayload {
    /// Carries a double-word integer value.
    Wide(u64),
    /// Carries a single felt value.
    Scalar(Felt),
}

/// Account component whose FPI methods transform payload variants.
#[component_storage]
struct CounterContractStorage {
    /// Storage map included so the shared FPI mock-chain helper can initialize the account.
    #[storage(description = "counter contract storage map")]
    count_map: StorageMap<Word, Felt>,
}

/// Account component whose FPI methods transform payload variants.
#[component]
trait CounterContract {
    /// Increments the payload of an optional felt value.
    fn bump_option(&self, input: Option<Felt>) -> Option<Felt>;
    /// Increments the payload of either result case.
    fn bump_result(&self, input: Result<Felt, u64>) -> Result<Felt, u64>;
    /// Increments the payload of either mixed-width variant case.
    fn bump_mixed(&self, input: MixedPayload) -> MixedPayload;
}

#[component]
impl CounterContract for CounterContractStorage {
    /// Increments the payload of an optional felt value.
    fn bump_option(&self, input: Option<Felt>) -> Option<Felt> {
        let _ = &self.count_map;
        input.map(|value| value + felt!(1))
    }

    /// Increments the payload of either result case.
    fn bump_result(&self, input: Result<Felt, u64>) -> Result<Felt, u64> {
        match input {
            Ok(value) => Ok(value + felt!(1)),
            Err(value) => Err(value + 1),
        }
    }

    /// Increments the payload of either mixed-width variant case.
    fn bump_mixed(&self, input: MixedPayload) -> MixedPayload {
        match input {
            MixedPayload::Wide(value) => MixedPayload::Wide(value + 1),
            MixedPayload::Scalar(value) => MixedPayload::Scalar(value + felt!(1)),
        }
    }
}
"#;

/// Minimal note script source which exercises payload variants in both FPI directions.
const COUNTER_CALLER_SOURCE: &str = r#"
#![no_std]
#![feature(alloc_error_handler)]

use miden::*;

use crate::bindings::miden::payload_variants_account::counter_contract::MixedPayload;
#[account(payload_variants_account::CounterContract)]
struct Counter;

/// Double-word value whose limbs both stay non-zero across the increment.
const WIDE_VALUE: u64 = 0x0000_0002_0000_0005;

/// Note script input containing the foreign counter account id.
#[note]
struct CounterCaller {
    /// Account id of the counter contract to invoke through FPI.
    counter_account_id: AccountId,
}

#[note]
impl CounterCaller {
    /// Checks that option, result, and mixed-width variant payloads cross the FPI boundary.
    #[note_script]
    pub fn run(self, _arg: Word) {
        let count_acc = Counter::new(self.counter_account_id);

        match count_acc.bump_option(Some(felt!(41))) {
            Some(value) => assert_eq(value, felt!(42)),
            None => assert_eq(felt!(0), felt!(1)),
        }
        match count_acc.bump_option(None) {
            None => (),
            Some(_) => assert_eq(felt!(0), felt!(1)),
        }

        match count_acc.bump_result(Ok(felt!(7))) {
            Ok(value) => assert_eq(value, felt!(8)),
            Err(_) => assert_eq(felt!(0), felt!(1)),
        }
        match count_acc.bump_result(Err(WIDE_VALUE)) {
            Err(value) => assert_u64_eq(value, WIDE_VALUE + 1),
            Ok(_) => assert_eq(felt!(0), felt!(1)),
        }

        match count_acc.bump_mixed(MixedPayload::Wide(WIDE_VALUE)) {
            MixedPayload::Wide(value) => assert_u64_eq(value, WIDE_VALUE + 1),
            MixedPayload::Scalar(_) => assert_eq(felt!(0), felt!(1)),
        }
        match count_acc.bump_mixed(MixedPayload::Scalar(felt!(9))) {
            MixedPayload::Scalar(value) => assert_eq(value, felt!(10)),
            MixedPayload::Wide(_) => assert_eq(felt!(0), felt!(1)),
        }
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
