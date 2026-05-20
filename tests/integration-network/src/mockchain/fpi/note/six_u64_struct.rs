//! Foreign procedure invocation rejection tests for records whose `u64` fields expand too wide.

use super::super::common::build_fpi_test_packages;

/// Rejects FPI signatures whose direct canonical form expands past the direct stack window.
#[test]
#[should_panic(expected = "direct FPI lowering supports at most 16")]
pub fn six_u64_struct_rejects_direct_width() {
    let _ =
        build_fpi_test_packages("six_u64_struct", COUNTER_CONTRACT_SOURCE, COUNTER_CALLER_SOURCE);
}

/// Minimal counter account component source used by the six-`u64` record FPI test.
const COUNTER_CONTRACT_SOURCE: &str = r#"
#![no_std]
#![feature(alloc_error_handler)]

use miden::{component, export_type, Felt, StorageMap, Word};

/// Record whose six `u64` fields expand past the direct FPI stack window.
#[export_type]
pub struct SixU64Record {
    /// First double-word integer field.
    pub first: u64,
    /// Second double-word integer field.
    pub second: u64,
    /// Third double-word integer field.
    pub third: u64,
    /// Fourth double-word integer field.
    pub fourth: u64,
    /// Fifth double-word integer field.
    pub fifth: u64,
    /// Sixth double-word integer field.
    pub sixth: u64,
    /// Felt field included to keep the generated WIT interface using Miden core types.
    pub felt_value: Felt,
}

/// Account component whose FPI method echoes a six-`u64` record.
#[component]
struct CounterContract {
    /// Storage map included so the shared FPI mock-chain helper can initialize the account.
    #[storage(description = "counter contract storage map")]
    count_map: StorageMap<Word, Felt>,
}

#[component]
impl CounterContract {
    /// Returns the six-`u64` record received from the caller.
    pub fn echo_six_u64_record(&self, input: SixU64Record) -> SixU64Record {
        let _ = &self.count_map;
        input
    }
}
"#;

/// Minimal note script source which exercises a six-`u64` record over FPI.
const COUNTER_CALLER_SOURCE: &str = r#"
#![no_std]
#![feature(alloc_error_handler)]

use miden::*;

use crate::bindings::miden::six_u64_struct_account::miden_six_u64_struct_account::SixU64Record;
use crate::bindings::MidenSixU64StructAccount as CounterContract;

/// First double-word value used by the six-`u64` record FPI test.
const FIRST: u64 = 0x0000_0001_0000_0002;
/// Second double-word value used by the six-`u64` record FPI test.
const SECOND: u64 = 0x0000_0003_0000_0004;
/// Third double-word value used by the six-`u64` record FPI test.
const THIRD: u64 = 0x0000_0005_0000_0006;
/// Fourth double-word value used by the six-`u64` record FPI test.
const FOURTH: u64 = 0x0000_0007_0000_0008;
/// Fifth double-word value used by the six-`u64` record FPI test.
const FIFTH: u64 = 0x0000_0009_0000_000a;
/// Sixth double-word value used by the six-`u64` record FPI test.
const SIXTH: u64 = 0x0000_000b_0000_000c;

/// Note script input containing the foreign counter account id.
#[note]
struct CounterCaller {
    /// Account id of the counter contract to invoke through FPI.
    counter_account_id: AccountId,
}

#[note]
impl CounterCaller {
    /// Checks that six `u64` fields cross the direct FPI boundary in both directions.
    #[note_script]
    pub fn run(self, _arg: Word) {
        let count_acc = CounterContract::from_account(self.counter_account_id);
        let result = count_acc.echo_six_u64_record(SixU64Record {
            first: FIRST,
            second: SECOND,
            third: THIRD,
            fourth: FOURTH,
            fifth: FIFTH,
            sixth: SIXTH,
            felt_value: felt!(91),
        });

        assert_u64_eq(result.first, FIRST);
        assert_u64_eq(result.second, SECOND);
        assert_u64_eq(result.third, THIRD);
        assert_u64_eq(result.fourth, FOURTH);
        assert_u64_eq(result.fifth, FIFTH);
        assert_u64_eq(result.sixth, SIXTH);
        assert_eq(result.felt_value, felt!(91));
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
