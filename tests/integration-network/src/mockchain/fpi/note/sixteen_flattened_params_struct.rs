//! Foreign procedure invocation tests for a record with sixteen flattened input felts.

use miden_client::Word;
use miden_core::Felt;

use super::super::common::{build_fpi_test_packages, execute_counter_caller_note};

/// Deploys a counter contract and consumes a note which passes a sixteen-felt record through FPI.
#[test]
pub fn sixteen_flattened_params_struct() {
    let (counter_package, caller_note_package, counter_storage_slot) = build_fpi_test_packages(
        "sixteen_flattened_params_struct",
        COUNTER_CONTRACT_SOURCE,
        COUNTER_CALLER_SOURCE,
    );

    execute_counter_caller_note(
        counter_package,
        caller_note_package,
        counter_storage_slot,
        sixteen_flattened_params_struct_storage_key(),
        100,
    );
}

/// Returns the non-zero storage key used by the sixteen-felt record FPI test.
fn sixteen_flattened_params_struct_storage_key() -> Word {
    Word::new([
        Felt::new(51).unwrap(),
        Felt::new(52).unwrap(),
        Felt::new(53).unwrap(),
        Felt::new(54).unwrap(),
    ])
}

/// Minimal counter account component source used by the sixteen-felt record FPI test.
const COUNTER_CONTRACT_SOURCE: &str = r#"
#![no_std]
#![feature(alloc_error_handler)]

use miden::{component, export_type, Felt, StorageMap, Word};

/// Record whose fields flatten to sixteen procedure input felts.
#[export_type]
pub struct SixteenFlattenedParams {
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
    /// Byte-sized integer field.
    pub u8_value: u8,
    /// Short integer field.
    pub u16_value: u16,
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

/// Account component whose FPI method echoes a sixteen-felt record.
#[component]
struct CounterContract {
    /// Storage map included so the shared FPI mock-chain helper can initialize the account.
    #[storage(description = "counter contract storage map")]
    count_map: StorageMap<Word, Felt>,
}

#[component]
impl CounterContract {
    /// Returns the sixteen-felt record received from the caller.
    pub fn echo_sixteen_flattened_params(
        &self,
        input: SixteenFlattenedParams,
    ) -> SixteenFlattenedParams {
        let _ = &self.count_map;
        input
    }
}
"#;

/// Minimal note script source which exercises a sixteen-felt record argument and result over FPI.
const COUNTER_CALLER_SOURCE: &str = r#"
#![no_std]
#![feature(alloc_error_handler)]

use miden::*;

use crate::bindings::miden::sixteen_flattened_params_struct_account::miden_sixteen_flattened_params_struct_account::SixteenFlattenedParams;
#[account(sixteen_flattened_params_struct_account)]
struct CounterContract;

/// Byte-sized value used by the sixteen-felt record FPI test.
const U8_VALUE: u8 = 0xab;
/// Short integer value used by the sixteen-felt record FPI test.
const U16_VALUE: u16 = 0xbeef;
/// First single-word value used by the sixteen-felt record FPI test.
const U32_0: u32 = 0x0001_0002;
/// Second single-word value used by the sixteen-felt record FPI test.
const U32_1: u32 = 0x0003_0004;
/// Third single-word value used by the sixteen-felt record FPI test.
const U32_2: u32 = 0x0005_0006;
/// Fourth single-word value used by the sixteen-felt record FPI test.
const U32_3: u32 = 0x0007_0008;
/// Fifth single-word value used by the sixteen-felt record FPI test.
const U32_4: u32 = 0x0009_000a;
/// Sixth single-word value used by the sixteen-felt record FPI test.
const U32_5: u32 = 0x000b_000c;

/// Note script input containing the foreign counter account id.
#[note]
struct CounterCaller {
    /// Account id of the counter contract to invoke through FPI.
    counter_account_id: AccountId,
}

#[note]
impl CounterCaller {
    /// Checks that sixteen flattened record fields cross the FPI boundary in both directions.
    #[note_script]
    pub fn run(self, _arg: Word) {
        let count_acc = CounterContract::new(self.counter_account_id);
        let result = count_acc.echo_sixteen_flattened_params(SixteenFlattenedParams {
            felt0: felt!(101),
            felt1: felt!(102),
            felt2: felt!(103),
            felt3: felt!(104),
            felt4: felt!(105),
            felt5: felt!(106),
            felt6: felt!(107),
            felt7: felt!(108),
            u8_value: U8_VALUE,
            u16_value: U16_VALUE,
            u32_0: U32_0,
            u32_1: U32_1,
            u32_2: U32_2,
            u32_3: U32_3,
            u32_4: U32_4,
            u32_5: U32_5,
        });

        assert_eq(result.felt0, felt!(101));
        assert_eq(result.felt1, felt!(102));
        assert_eq(result.felt2, felt!(103));
        assert_eq(result.felt3, felt!(104));
        assert_eq(result.felt4, felt!(105));
        assert_eq(result.felt5, felt!(106));
        assert_eq(result.felt6, felt!(107));
        assert_eq(result.felt7, felt!(108));
        assert_eq(Felt::from_u32(result.u8_value as u32), Felt::from_u32(U8_VALUE as u32));
        assert_eq(Felt::from_u32(result.u16_value as u32), Felt::from_u32(U16_VALUE as u32));
        assert_eq(Felt::from_u32(result.u32_0), Felt::from_u32(U32_0));
        assert_eq(Felt::from_u32(result.u32_1), Felt::from_u32(U32_1));
        assert_eq(Felt::from_u32(result.u32_2), Felt::from_u32(U32_2));
        assert_eq(Felt::from_u32(result.u32_3), Felt::from_u32(U32_3));
        assert_eq(Felt::from_u32(result.u32_4), Felt::from_u32(U32_4));
        assert_eq(Felt::from_u32(result.u32_5), Felt::from_u32(U32_5));
    }
}
"#;
