//! Foreign procedure invocation tests for records containing mixed scalar types.

use miden_client::Word;
use miden_core::Felt;

use super::super::common::{build_fpi_test_packages, execute_counter_caller_note};

/// Deploys a counter contract and consumes a note which passes mixed scalar records through FPI.
#[test]
pub fn mixed_scalar_struct() {
    let (counter_package, caller_note_package, counter_storage_slot) = build_fpi_test_packages(
        "mixed_scalar_struct",
        COUNTER_CONTRACT_SOURCE,
        COUNTER_CALLER_SOURCE,
    );

    execute_counter_caller_note(
        counter_package,
        caller_note_package,
        counter_storage_slot,
        mixed_scalar_struct_storage_key(),
        100,
    );
}

/// Returns the non-zero storage key used by the mixed scalar record FPI test.
fn mixed_scalar_struct_storage_key() -> Word {
    Word::new([Felt::new(41), Felt::new(42), Felt::new(43), Felt::new(44)])
}

/// Minimal counter account component source used by the mixed scalar record FPI test.
const COUNTER_CONTRACT_SOURCE: &str = r#"
#![no_std]
#![feature(alloc_error_handler)]

use miden::{component, export_type, Felt, StorageMap, Word};

/// Record with mixed scalar fields passed through the FPI boundary.
#[export_type]
pub struct MixedScalarRecord {
    /// First double-word integer field.
    pub first_u64: u64,
    /// Second double-word integer field.
    pub second_u64: u64,
    /// Single felt field.
    pub felt_value: Felt,
    /// Single-word integer field.
    pub u32_value: u32,
    /// Byte-sized integer field.
    pub u8_value: u8,
}

/// Account component whose FPI method echoes a mixed scalar record.
#[component]
struct CounterContract {
    /// Storage map included so the shared FPI mock-chain helper can initialize the account.
    #[storage(description = "counter contract storage map")]
    count_map: StorageMap<Word, Felt>,
}

#[component]
impl CounterContract {
    /// Returns the mixed scalar record received from the caller.
    pub fn echo_mixed_scalar_record(&self, input: MixedScalarRecord) -> MixedScalarRecord {
        let _ = &self.count_map;
        input
    }
}
"#;

/// Minimal note script source which exercises a mixed scalar record argument and result over FPI.
const COUNTER_CALLER_SOURCE: &str = r#"
#![no_std]
#![feature(alloc_error_handler)]

use miden::*;

use crate::bindings::miden::mixed_scalar_struct_account::mixed_scalar_struct_account::MixedScalarRecord;
use crate::bindings::MixedScalarStructAccount as CounterContract;

/// First double-word value used by the mixed scalar record FPI test.
const FIRST_U64: u64 = 0x0000_0001_0000_0002;
/// Second double-word value used by the mixed scalar record FPI test.
const SECOND_U64: u64 = 0x0000_0003_0000_0004;
/// Single-word value used by the mixed scalar record FPI test.
const U32_VALUE: u32 = 0xdead_beef;
/// Byte-sized value used by the mixed scalar record FPI test.
const U8_VALUE: u8 = 0xab;

/// Note script input containing the foreign counter account id.
#[note]
struct CounterCaller {
    /// Account id of the counter contract to invoke through FPI.
    counter_account_id: AccountId,
}

#[note]
impl CounterCaller {
    /// Checks that a mixed scalar record crosses the FPI boundary in both directions.
    #[note_script]
    pub fn run(self, _arg: Word) {
        let count_acc = CounterContract::from_account(self.counter_account_id);
        let result = count_acc.echo_mixed_scalar_record(MixedScalarRecord {
            first_u64: FIRST_U64,
            second_u64: SECOND_U64,
            felt_value: felt!(77),
            u32_value: U32_VALUE,
            u8_value: U8_VALUE,
        });

        assert_u64_eq(result.first_u64, FIRST_U64);
        assert_u64_eq(result.second_u64, SECOND_U64);
        assert_eq(result.felt_value, felt!(77));
        assert_eq(Felt::from_u32(result.u32_value), Felt::from_u32(U32_VALUE));
        assert_eq(Felt::from_u32(result.u8_value as u32), Felt::from_u32(U8_VALUE as u32));
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
