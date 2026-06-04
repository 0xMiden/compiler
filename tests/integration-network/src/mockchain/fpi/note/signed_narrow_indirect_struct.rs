//! Foreign procedure invocation tests for indirect records with negative narrow signed fields.

use miden_client::Word;
use miden_core::Felt;

use super::super::common::{build_fpi_test_packages, execute_counter_caller_note};

/// Deploys a counter contract and consumes a note which passes negative signed narrow fields
/// through the indirect FPI path.
#[test]
pub fn signed_narrow_indirect_struct() {
    let (counter_package, caller_note_package, counter_storage_slot) = build_fpi_test_packages(
        "signed_narrow_indirect_struct",
        COUNTER_CONTRACT_SOURCE,
        COUNTER_CALLER_SOURCE,
    );

    execute_counter_caller_note(
        counter_package,
        caller_note_package,
        counter_storage_slot,
        signed_narrow_indirect_struct_storage_key(),
        100,
    );
}

/// Returns the non-zero storage key used by the signed-narrow indirect FPI test.
fn signed_narrow_indirect_struct_storage_key() -> Word {
    Word::new([
        Felt::new(61).unwrap(),
        Felt::new(62).unwrap(),
        Felt::new(63).unwrap(),
        Felt::new(64).unwrap(),
    ])
}

/// Minimal counter account component source used by the signed-narrow indirect FPI test.
const COUNTER_CONTRACT_SOURCE: &str = r#"
#![no_std]
#![feature(alloc_error_handler)]

use miden::{component, export_type, Felt, StorageMap, Word};

/// Record whose fields force indirect FPI while carrying signed narrow values.
#[export_type]
pub struct SignedNarrowRecord {
    /// First felt field.
    pub felt0: Felt,
    /// Second felt field.
    pub felt1: Felt,
    /// Third felt field.
    pub felt2: Felt,
    /// Fourth felt field.
    pub felt3: Felt,
    /// Fifth felt field.
    pub felt4: Felt,
    /// Sixth felt field.
    pub felt5: Felt,
    /// Seventh felt field.
    pub felt6: Felt,
    /// Eighth felt field.
    pub felt7: Felt,
    /// Negative byte-sized integer field.
    pub i8_value: i8,
    /// Negative short integer field.
    pub i16_value: i16,
    /// First single-word integer field.
    pub u32_0: u32,
    /// Second single-word integer field.
    pub u32_1: u32,
    /// Third single-word integer field.
    pub u32_2: u32,
    /// Fourth single-word integer field.
    pub u32_3: u32,
    /// Fifth single-word integer field.
    pub u32_4: u32,
    /// Sixth single-word integer field.
    pub u32_5: u32,
}

/// Account component whose FPI method echoes a signed-narrow record.
#[component]
struct CounterContract {
    /// Storage map included so the shared FPI mock-chain helper can initialize the account.
    #[storage(description = "counter contract storage map")]
    count_map: StorageMap<Word, Felt>,
}

#[component]
impl CounterContract {
    /// Returns the signed-narrow record received from the caller.
    pub fn echo_signed_narrow_record(&self, input: SignedNarrowRecord) -> SignedNarrowRecord {
        let _ = &self.count_map;
        assert_eq!(input.i8_value, -7);
        assert_eq!(input.i16_value, -1234);
        input
    }
}
"#;

/// Minimal note script source which exercises negative signed narrow fields over indirect FPI.
const COUNTER_CALLER_SOURCE: &str = r#"
#![no_std]
#![feature(alloc_error_handler)]

use miden::*;

use crate::bindings::miden::signed_narrow_indirect_struct_account::miden_signed_narrow_indirect_struct_account::SignedNarrowRecord;
#[account(signed_narrow_indirect_struct_account)]
struct CounterContract;

/// Negative byte-sized value used by the signed-narrow indirect FPI test.
const I8_VALUE: i8 = -7;
/// Negative short integer value used by the signed-narrow indirect FPI test.
const I16_VALUE: i16 = -1234;
/// First single-word value used by the signed-narrow indirect FPI test.
const U32_0: u32 = 0x0001_0002;
/// Second single-word value used by the signed-narrow indirect FPI test.
const U32_1: u32 = 0x0003_0004;
/// Third single-word value used by the signed-narrow indirect FPI test.
const U32_2: u32 = 0x0005_0006;
/// Fourth single-word value used by the signed-narrow indirect FPI test.
const U32_3: u32 = 0x0007_0008;
/// Fifth single-word value used by the signed-narrow indirect FPI test.
const U32_4: u32 = 0x0009_000a;
/// Sixth single-word value used by the signed-narrow indirect FPI test.
const U32_5: u32 = 0x000b_000c;

/// Note script input containing the foreign counter account id.
#[note]
struct CounterCaller {
    /// Account id of the counter contract to invoke through FPI.
    counter_account_id: AccountId,
}

#[note]
impl CounterCaller {
    /// Checks that negative signed narrow fields cross the indirect FPI boundary.
    #[note_script]
    pub fn run(self, _arg: Word) {
        let count_acc = CounterContract::new(self.counter_account_id);
        let result = count_acc.echo_signed_narrow_record(SignedNarrowRecord {
            felt0: felt!(201),
            felt1: felt!(202),
            felt2: felt!(203),
            felt3: felt!(204),
            felt4: felt!(205),
            felt5: felt!(206),
            felt6: felt!(207),
            felt7: felt!(208),
            i8_value: I8_VALUE,
            i16_value: I16_VALUE,
            u32_0: U32_0,
            u32_1: U32_1,
            u32_2: U32_2,
            u32_3: U32_3,
            u32_4: U32_4,
            u32_5: U32_5,
        });

        assert_eq(result.felt0, felt!(201));
        assert_eq(result.felt1, felt!(202));
        assert_eq(result.felt2, felt!(203));
        assert_eq(result.felt3, felt!(204));
        assert_eq(result.felt4, felt!(205));
        assert_eq(result.felt5, felt!(206));
        assert_eq(result.felt6, felt!(207));
        assert_eq(result.felt7, felt!(208));
        assert_eq!(result.i8_value, I8_VALUE);
        assert_eq!(result.i16_value, I16_VALUE);
        assert_eq(Felt::from_u32(result.u32_0), Felt::from_u32(U32_0));
        assert_eq(Felt::from_u32(result.u32_1), Felt::from_u32(U32_1));
        assert_eq(Felt::from_u32(result.u32_2), Felt::from_u32(U32_2));
        assert_eq(Felt::from_u32(result.u32_3), Felt::from_u32(U32_3));
        assert_eq(Felt::from_u32(result.u32_4), Felt::from_u32(U32_4));
        assert_eq(Felt::from_u32(result.u32_5), Felt::from_u32(U32_5));
    }
}
"#;
