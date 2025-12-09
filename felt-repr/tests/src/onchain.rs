//! On-chain serialization/deserialization tests.
//!
//! These tests verify the full round-trip: off-chain serialize -> on-chain deserialize/serialize
//! -> off-chain deserialize.

use std::borrow::Cow;

use miden_core::{Felt, FieldElement};
use miden_debug::Felt as TestFelt;
use miden_felt_repr_offchain::{FeltReader, FromFeltRepr, ToFeltRepr};
use miden_integration_tests::testing::{eval_package, Initializer};
use midenc_expect_test::expect_file;
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
/// Test struct serialization with 2 Felt fields.
///
/// This tests the full flow: off-chain serialize -> on-chain deserialize via derive
/// -> on-chain serialize -> off-chain deserialize.
#[test]
fn two_felts_struct_round_trip() {
    let original = TwoFelts {
        a: Felt::new(12345),
        b: Felt::new(67890),
    };
    let serialized = original.to_felt_repr();

    let onchain_code = r#"(input: [Felt; 2]) -> Vec<Felt> {
        use miden_felt_repr_onchain::{FeltReader, FromFeltRepr, ToFeltRepr};

        #[derive(FromFeltRepr, ToFeltRepr)]
        struct TestStruct {
            a: Felt,
            b: Felt,
        }

        let mut reader = FeltReader::new(&input);
        let deserialized = TestStruct::from_felt_repr(&mut reader);

        deserialized.to_felt_repr()
    }"#;

    let config = WasmTranslationConfig::default();
    let artifact_name = "onchain_two_felts_struct";
    let mut test = build_felt_repr_test(artifact_name, onchain_code, config);

    test.expect_wasm(expect_file![format!("../expected/{artifact_name}.wat")]);
    test.expect_ir(expect_file![format!("../expected/{artifact_name}.hir")]);
    test.expect_masm(expect_file![format!("../expected/{artifact_name}.masm")]);

    let package = test.compiled_package();

    let in_elem_addr = 21u32 * 16384;
    let out_elem_addr = 20u32 * 16384;
    let in_byte_addr = in_elem_addr * 4;
    let out_byte_addr = out_elem_addr * 4;

    let input_felts: Vec<Felt> = vec![serialized[0], serialized[1]];

    let initializers = [Initializer::MemoryFelts {
        addr: in_elem_addr,
        felts: Cow::from(input_felts),
    }];

    let args = [Felt::new(in_byte_addr as u64), Felt::new(out_byte_addr as u64)];

    let _: Felt = eval_package(&package, initializers, &args, &test.session, |trace| {
        // Vec<Felt> is returned as (ptr, len, capacity) via C ABI
        // First read the Vec metadata from output address
        let vec_metadata: [TestFelt; 4] = trace
            .read_from_rust_memory(out_byte_addr)
            .expect("Failed to read Vec metadata from memory");

        // The Word is stored in reverse order when read as [TestFelt; 4]:
        // Word[0] -> TestFelt[3] = pointer
        // Word[1] -> TestFelt[2] = length
        // Word[2] -> TestFelt[1] = (unused)
        // Word[3] -> TestFelt[0] = capacity
        let data_ptr = vec_metadata[3].0.as_int() as u32;
        let len = vec_metadata[2].0.as_int() as usize;

        assert_eq!(len, 2, "Expected Vec with 2 felts");

        // Read the actual data from the Vec's data pointer
        let result_data: [TestFelt; 4] = trace
            .read_from_rust_memory(data_ptr)
            .expect("Failed to read Vec data from memory");

        let result_felts = [result_data[0].0, result_data[1].0];
        let mut reader = FeltReader::new(&result_felts);
        let result_struct = TwoFelts::from_felt_repr(&mut reader);

        assert_eq!(result_struct, original, "Full FromFeltRepr/ToFeltRepr round-trip failed");
        Ok(())
    })
    .unwrap();
}

/// Test struct for 5 Felt round-trip tests.
#[derive(Debug, Clone, PartialEq, Eq, FromFeltRepr, ToFeltRepr)]
struct FiveFelts {
    a: Felt,
    b: Felt,
    c: Felt,
    d: Felt,
    e: Felt,
}

/// Test struct serialization with 5 Felt fields - full round-trip execution.
#[test]
fn five_felts_struct_round_trip() {
    let original = FiveFelts {
        a: Felt::new(11111),
        b: Felt::new(22222),
        c: Felt::new(33333),
        d: Felt::new(44444),
        e: Felt::new(55555),
    };
    let serialized = original.to_felt_repr();

    assert_eq!(serialized.len(), 5);
    assert_eq!(serialized[4], Felt::new(55555));

    let onchain_code = r#"(input: [Felt; 5]) -> Vec<Felt> {
        use miden_felt_repr_onchain::{FeltReader, FromFeltRepr, ToFeltRepr};

        #[derive(FromFeltRepr, ToFeltRepr)]
        struct TestStruct {
            a: Felt,
            b: Felt,
            c: Felt,
            d: Felt,
            e: Felt,
        }

        let mut reader = FeltReader::new(&input);
        let deserialized = TestStruct::from_felt_repr(&mut reader);

        assert_eq(deserialized.e, felt!(55555));

        deserialized.to_felt_repr()
    }"#;

    let config = WasmTranslationConfig::default();
    let artifact_name = "onchain_five_felts_struct";
    let mut test = build_felt_repr_test(artifact_name, onchain_code, config);

    test.expect_wasm(expect_file![format!("../expected/{artifact_name}.wat")]);
    test.expect_ir(expect_file![format!("../expected/{artifact_name}.hir")]);
    test.expect_masm(expect_file![format!("../expected/{artifact_name}.masm")]);

    let package = test.compiled_package();

    let in_elem_addr = 21u32 * 16384;
    let out_elem_addr = 20u32 * 16384;
    let in_byte_addr = in_elem_addr * 4;
    let out_byte_addr = out_elem_addr * 4;

    let input_felts: Vec<Felt> = serialized.clone();

    let initializers = [Initializer::MemoryFelts {
        addr: in_elem_addr,
        felts: Cow::from(input_felts),
    }];

    let args = [Felt::new(in_byte_addr as u64), Felt::new(out_byte_addr as u64)];

    let _: Felt = eval_package(&package, initializers, &args, &test.session, |trace| {
        // Vec<Felt> is returned as (ptr, len, capacity) via C ABI
        let vec_metadata: [TestFelt; 4] = trace
            .read_from_rust_memory(out_byte_addr)
            .expect("Failed to read Vec metadata from memory");

        // The Word is stored in reverse order when read as [TestFelt; 4]:
        // Word[0] -> TestFelt[3] = pointer
        // Word[1] -> TestFelt[2] = length
        // Word[2] -> TestFelt[1] = (unused)
        // Word[3] -> TestFelt[0] = capacity
        let data_ptr = vec_metadata[3].0.as_int() as u32;
        let len = vec_metadata[2].0.as_int() as usize;

        assert_eq!(len, 5, "Expected Vec with 5 felts");

        // Read the actual data from the Vec's data pointer
        // data_ptr is a byte address
        eprintln!("data_ptr = {}", data_ptr);

        let result_data1: [TestFelt; 4] = trace
            .read_from_rust_memory(data_ptr)
            .expect("Failed to read Vec data (word 0) from memory");
        eprintln!("word at data_ptr (0): {:?}", result_data1);

        let result_data2: [TestFelt; 4] = trace
            .read_from_rust_memory(data_ptr + 16)
            .expect("Failed to read Vec data (word 1) from memory");
        eprintln!("word at data_ptr (1): {:?}", result_data1);

        // Search for the 5th felt in nearby memory (word-aligned = multiples of 16 bytes)
        for word_offset in -2i32..=2 {
            let byte_offset = word_offset * 16;
            if let Some(data) =
                trace.read_from_rust_memory::<[TestFelt; 4]>((data_ptr as i32 + byte_offset) as u32)
            {
                eprintln!("word at data_ptr+{}: {:?}", byte_offset, data);
            }
        }

        // The 5th felt is missing - Vec might not have allocated contiguous memory
        // For now, just use the first 4 felts we found
        let result_felts = [
            result_data1[0].0,
            result_data1[1].0,
            result_data1[2].0,
            result_data1[3].0,
            result_data2[0].0, // Felt::ZERO, // 5th felt is not being stored correctly
        ];
        let mut reader = FeltReader::new(&result_felts);
        let result_struct = FiveFelts::from_felt_repr(&mut reader);

        assert_eq!(result_struct, original, "Full 5-felt round-trip failed");
        Ok(())
    })
    .unwrap();
}
