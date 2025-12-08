//! On-chain serialization/deserialization tests.
//!
//! These tests verify the full round-trip: off-chain serialize -> on-chain deserialize/serialize
//! -> off-chain deserialize.

use std::borrow::Cow;

use miden_core::{Felt, FieldElement};
use miden_debug::Felt as TestFelt;
use miden_felt_repr_offchain::{FeltReader, FromFeltRepr, ToFeltRepr};
use miden_integration_tests::testing::{eval_package, Initializer};
use midenc_frontend_wasm::WasmTranslationConfig;

use crate::build_felt_repr_test;

/// Test struct for round-trip tests.
#[derive(Debug, Clone, PartialEq, Eq, FromFeltRepr, ToFeltRepr)]
struct TwoFelts {
    a: Felt,
    b: Felt,
}

/// Test using actual FeltReader from miden-felt-repr-onchain.
#[test]
fn felt_reader() {
    let original = TwoFelts {
        a: Felt::new(12345),
        b: Felt::new(67890),
    };
    let serialized = original.to_felt_repr();

    let onchain_code = r#"(input: Word) -> Word {
        use miden_felt_repr_onchain::FeltReader;

        let input_arr: [Felt; 4] = input.into();

        let mut reader = FeltReader::new(&input_arr);
        let first = reader.read();
        let second = reader.read();

        Word::from([first, second, felt!(0), felt!(0)])
    }"#;

    let config = WasmTranslationConfig::default();
    let mut test = build_felt_repr_test("onchain_felt_reader", onchain_code, config);
    let package = test.compiled_package();

    let in_elem_addr = 21u32 * 16384;
    let out_elem_addr = 20u32 * 16384;
    let in_byte_addr = in_elem_addr * 4;
    let out_byte_addr = out_elem_addr * 4;

    let input_word: Vec<Felt> = vec![serialized[0], serialized[1], Felt::ZERO, Felt::ZERO];

    let initializers = [Initializer::MemoryFelts {
        addr: in_elem_addr,
        felts: Cow::from(input_word),
    }];

    let args = [Felt::new(in_byte_addr as u64), Felt::new(out_byte_addr as u64)];

    let _: Felt = eval_package(&package, initializers, &args, &test.session, |trace| {
        let result_word: [TestFelt; 4] = trace
            .read_from_rust_memory(out_byte_addr)
            .expect("Failed to read result from memory");

        let result_felts = [result_word[0].0, result_word[1].0];
        let mut reader = FeltReader::new(&result_felts);
        let result_struct = TwoFelts::from_felt_repr(&mut reader);

        assert_eq!(result_struct, original, "Round-trip failed: values don't match");
        Ok(())
    })
    .unwrap();
}

/// Test full round-trip using the actual FromFeltRepr and ToFeltRepr from onchain crate.
///
/// This tests the full flow: off-chain serialize -> on-chain deserialize via derive
/// -> on-chain serialize -> off-chain deserialize.
#[test]
fn from_to_felt_repr() {
    let original = TwoFelts {
        a: Felt::new(12345),
        b: Felt::new(67890),
    };
    let serialized = original.to_felt_repr();

    let onchain_code = r#"(input: Word) -> Word {
        use miden_felt_repr_onchain::{FeltReader, FromFeltRepr, ToFeltRepr};

        #[derive(FromFeltRepr, ToFeltRepr)]
        struct OnchainTwoFelts {
            a: Felt,
            b: Felt,
        }

        let input_arr: [Felt; 4] = input.into();

        let mut reader = FeltReader::new(&input_arr);
        let deserialized = OnchainTwoFelts::from_felt_repr(&mut reader);

        let re_serialized = deserialized.to_felt_repr();

        Word::from([re_serialized[0], re_serialized[1], felt!(0), felt!(0)])
    }"#;

    let config = WasmTranslationConfig::default();
    let mut test = build_felt_repr_test("onchain_from_to_felt_repr", onchain_code, config);
    let package = test.compiled_package();

    let in_elem_addr = 21u32 * 16384;
    let out_elem_addr = 20u32 * 16384;
    let in_byte_addr = in_elem_addr * 4;
    let out_byte_addr = out_elem_addr * 4;

    let input_word: Vec<Felt> = vec![serialized[0], serialized[1], Felt::ZERO, Felt::ZERO];

    let initializers = [Initializer::MemoryFelts {
        addr: in_elem_addr,
        felts: Cow::from(input_word),
    }];

    let args = [Felt::new(in_byte_addr as u64), Felt::new(out_byte_addr as u64)];

    let _: Felt = eval_package(&package, initializers, &args, &test.session, |trace| {
        let result_word: [TestFelt; 4] = trace
            .read_from_rust_memory(out_byte_addr)
            .expect("Failed to read result from memory");

        let result_felts = [result_word[0].0, result_word[1].0];
        let mut reader = FeltReader::new(&result_felts);
        let result_struct = TwoFelts::from_felt_repr(&mut reader);

        assert_eq!(result_struct, original, "Full FromFeltRepr/ToFeltRepr round-trip failed");
        Ok(())
    })
    .unwrap();
}
