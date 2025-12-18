//! On-chain serialization/deserialization tests.
//!
//! These tests verify the full round-trip: off-chain serialize -> on-chain deserialize/serialize
//! -> off-chain deserialize.

use std::borrow::Cow;

use miden_core::{Felt, FieldElement};
use miden_debug::{ExecutionTrace, Felt as TestFelt};
use miden_felt_repr_offchain::{FeltReader, FromFeltRepr, ToFeltRepr};
use miden_integration_tests::testing::{Initializer, eval_package};
use midenc_frontend_wasm::WasmTranslationConfig;
use temp_dir::TempDir;

use crate::build_felt_repr_test;

/// Reads a `Vec<Felt>` returned via the Rust ABI from VM memory.
fn read_vec_felts(trace: &ExecutionTrace, vec_meta_addr: u32, expected_len: usize) -> Vec<Felt> {
    let vec_metadata: [TestFelt; 4] = trace
        .read_from_rust_memory(vec_meta_addr)
        .expect("Failed to read Vec metadata from memory");
    // Vec metadata layout is: [capacity, ptr, len, ?]
    let data_ptr = vec_metadata[1].0.as_int() as u32;
    let len = vec_metadata[2].0.as_int() as usize;

    assert_eq!(len, expected_len, "Unexpected Vec length");

    let elem_addr = data_ptr / 4;
    let mut result = Vec::with_capacity(len);
    for i in 0..len {
        let byte_addr = (elem_addr + i as u32) * 4;
        let word_addr = (byte_addr / 16) * 16;
        let word: [TestFelt; 4] = trace
            .read_from_rust_memory(word_addr)
            .unwrap_or_else(|| panic!("Failed to read word for element {i}"));
        let elem_in_word = ((byte_addr % 16) / 4) as usize;
        result.push(word[elem_in_word].0);
    }

    result
}

/// Test struct for round-trip tests.
#[derive(Debug, Clone, PartialEq, Eq, FromFeltRepr, ToFeltRepr)]
struct TwoFelts {
    a: Felt,
    b: Felt,
}

/// Test using actual FeltReader from miden-felt-repr-onchain.
#[test]
fn test_felt_reader() {
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
    let name = "onchain_felt_reader";
    let temp_dir = TempDir::with_prefix(name).unwrap();
    let mut test = build_felt_repr_test(&temp_dir, name, onchain_code, config);
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
fn test_two_felts_struct_round_trip() {
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
    let name = "onchain_two_felts_struct";
    let temp_dir = TempDir::with_prefix(name).unwrap();
    let mut test = build_felt_repr_test(&temp_dir, name, onchain_code, config);
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
        let result_felts = read_vec_felts(trace, out_byte_addr, 2);
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
fn test_five_felts_struct_round_trip() {
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

        deserialized.to_felt_repr()
    }"#;

    let config = WasmTranslationConfig::default();
    let name = "onchain_five_felts_struct";
    let temp_dir = TempDir::with_prefix(name).unwrap();
    let mut test = build_felt_repr_test(&temp_dir, name, onchain_code, config);
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
        let result_felts = read_vec_felts(trace, out_byte_addr, 5);
        let mut reader = FeltReader::new(&result_felts);
        let result_struct = FiveFelts::from_felt_repr(&mut reader);

        assert_eq!(result_struct, original, "Full 5-felt round-trip failed");
        Ok(())
    })
    .unwrap();
}

/// Minimal struct to reproduce u64 stack tracking bug.
/// The bug requires: 1 u64 + 4 smaller integer types (u8 or u32).
/// With Felt fields it passes; with u8/u32 fields it fails.
/// The difference is that u8/u32 use `as_u32` intrinsic after reading.
#[derive(Debug, Clone, PartialEq, Eq, FromFeltRepr, ToFeltRepr)]
struct MinimalU64Bug {
    n1: u64,
    a: u32,
    b: u32,
    x: u32,
    y: u32,
}

/// Minimal test case for u64 stack tracking bug.
///
/// The bug occurs when:
/// 1. Multiple u64 fields are read (as_u64 returns 2 felts on stack each)
/// 2. Another field (y) is NOT immediately consumed (no assert_eq)
/// 3. The value needs to be spilled to a local variable
///
/// This causes incorrect stack position tracking, spilling the wrong value.
#[ignore = "until https://github.com/0xMiden/compiler/issues/815 is resolved"]
#[test]
fn test_minimal_u64_bug() {
    let original = MinimalU64Bug {
        n1: 111111,
        a: 22,
        b: 33,
        x: 44,
        y: 55,
    };
    let serialized = original.to_felt_repr();

    assert_eq!(serialized.len(), 5);

    let onchain_code = r#"(input: [Felt; 5]) -> Vec<Felt> {
        use miden_felt_repr_onchain::{FeltReader, FromFeltRepr, ToFeltRepr};

        assert_eq(input[0], felt!(111111));
        assert_eq(input[4], felt!(55));

        #[derive(FromFeltRepr, ToFeltRepr)]
        struct TestStruct {
            n1: u64,
            a: u32,
            b: u32,
            x: u32,
            y: u32,
        }

        let mut reader = FeltReader::new(&input);
        let deserialized = TestStruct::from_felt_repr(&mut reader);

        // NOT using assert_eq on y - this triggers the bug
        // The y value needs to survive until to_felt_repr()

        deserialized.to_felt_repr()
    }"#;

    let config = WasmTranslationConfig::default();
    let name = "onchain_minimal_u64_bug";
    let temp_dir = TempDir::with_prefix(name).unwrap();
    let mut test = build_felt_repr_test(&temp_dir, name, onchain_code, config);
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
        let result_felts = read_vec_felts(trace, out_byte_addr, 5);
        let mut reader = FeltReader::new(&result_felts);
        let result_struct = MinimalU64Bug::from_felt_repr(&mut reader);

        assert_eq!(result_struct, original, "Minimal u64 bug round-trip failed");
        Ok(())
    })
    .unwrap();
}

/// Test struct with Felt fields instead of u64 (to test if u64 causes the stack tracking bug).
#[derive(Debug, Clone, PartialEq, Eq, FromFeltRepr, ToFeltRepr)]
struct MixedTypesNoU64 {
    f1: Felt,
    f2: Felt,
    f3: Felt,
    f4: Felt,
    x: u32,
    y: u8,
}

/// Test struct serialization with Felt fields instead of u64 - to verify u64 involvement in bug.
///
/// Tests a struct with 4 Felt, 1 u32, and 1 u8 fields (no u64).
/// Each field is serialized as one Felt, so total is 6 Felts.
#[test]
fn test_mixed_types_no_u64_round_trip() {
    let original = MixedTypesNoU64 {
        f1: Felt::new(111111),
        f2: Felt::new(222222),
        f3: Felt::new(333333),
        f4: Felt::new(444444),
        x: 55555,
        y: 66,
    };
    let serialized = original.to_felt_repr();

    // Each field serializes to one Felt
    assert_eq!(serialized.len(), 6);

    let onchain_code = r#"(input: [Felt; 6]) -> Vec<Felt> {
        use miden_felt_repr_onchain::{FeltReader, FromFeltRepr, ToFeltRepr};

        #[derive(FromFeltRepr, ToFeltRepr)]
        struct TestStruct {
            f1: Felt,
            f2: Felt,
            f3: Felt,
            f4: Felt,
            x: u32,
            y: u8,
        }

        let mut reader = FeltReader::new(&input);
        let deserialized = TestStruct::from_felt_repr(&mut reader);

        // Deliberately NOT using assert_eq on y to trigger the bug (if u64 is involved)
        // assert_eq(Felt::from(deserialized.y as u32), felt!(66));

        deserialized.to_felt_repr()
    }"#;

    let config = WasmTranslationConfig::default();
    let name = "onchain_mixed_types_no_u64";
    let temp_dir = TempDir::with_prefix(name).unwrap();
    let mut test = build_felt_repr_test(&temp_dir, name, onchain_code, config);
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
        let result_felts = read_vec_felts(trace, out_byte_addr, 6);
        let mut reader = FeltReader::new(&result_felts);
        let result_struct = MixedTypesNoU64::from_felt_repr(&mut reader);

        assert_eq!(result_struct, original, "Mixed types (no u64) round-trip failed");
        Ok(())
    })
    .unwrap();
}

/// Inner struct for nested struct tests.
#[derive(Debug, Clone, PartialEq, Eq, FromFeltRepr, ToFeltRepr)]
struct Inner {
    x: Felt,
    y: u64,
}

/// Outer struct containing nested Inner struct.
#[derive(Debug, Clone, PartialEq, Eq, FromFeltRepr, ToFeltRepr)]
struct Outer {
    a: Felt,
    inner: Inner,
    b: u32,
    flag1: bool,
    flag2: bool,
}

/// Test nested struct serialization - full round-trip execution.
///
/// Tests a struct containing another struct as a field, plus bool fields.
/// Outer has: 1 Felt + Inner(1 Felt + 1 u64) + 1 u32 + 2 bool = 6 Felts total.
#[test]
fn test_nested_struct_round_trip() {
    let original = Outer {
        a: Felt::new(111111),
        inner: Inner {
            x: Felt::new(222222),
            y: 333333,
        },
        b: 44444,
        flag1: true,
        flag2: false,
    };
    let serialized = original.to_felt_repr();

    // Outer.a (1) + Inner.x (1) + Inner.y (1) + Outer.b (1) + flag1 (1) + flag2 (1) = 6 Felts
    assert_eq!(serialized.len(), 6);

    let onchain_code = r#"(input: [Felt; 6]) -> Vec<Felt> {
        use miden_felt_repr_onchain::{FeltReader, FromFeltRepr, ToFeltRepr};

        #[derive(FromFeltRepr, ToFeltRepr)]
        struct Inner {
            x: Felt,
            y: u64,
        }

        #[derive(FromFeltRepr, ToFeltRepr)]
        struct Outer {
            a: Felt,
            inner: Inner,
            b: u32,
            flag1: bool,
            flag2: bool,
        }

        let mut reader = FeltReader::new(&input);
        let deserialized = Outer::from_felt_repr(&mut reader);

        // Verify fields were deserialized correctly
        assert_eq(deserialized.a, felt!(111111));
        assert_eq(deserialized.inner.x, felt!(222222));
        assert_eq(Felt::from(deserialized.b), felt!(44444));
        assert_eq(Felt::from(deserialized.flag1 as u32), felt!(1));
        assert_eq(Felt::from(deserialized.flag2 as u32), felt!(0));

        deserialized.to_felt_repr()
    }"#;

    let config = WasmTranslationConfig::default();
    let name = "onchain_nested_struct";
    let temp_dir = TempDir::with_prefix(name).unwrap();
    let mut test = build_felt_repr_test(&temp_dir, name, onchain_code, config);
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
        let result_felts = read_vec_felts(trace, out_byte_addr, 6);
        let mut reader = FeltReader::new(&result_felts);
        let result_struct = Outer::from_felt_repr(&mut reader);

        assert_eq!(result_struct, original, "Nested struct round-trip failed");
        Ok(())
    })
    .unwrap();
}

#[test]
fn test_enum_unit_round_trip() {
    #[derive(Debug, Clone, PartialEq, Eq, FromFeltRepr, ToFeltRepr)]
    enum SimpleEnum {
        A,
        B,
        C,
    }

    #[derive(Debug, Clone, PartialEq, Eq, FromFeltRepr, ToFeltRepr)]
    struct Wrapper {
        pad: Felt,
        value: SimpleEnum,
    }

    let original = Wrapper {
        pad: Felt::new(999),
        value: SimpleEnum::B,
    };
    let serialized = original.to_felt_repr();

    assert_eq!(serialized.len(), 2);
    assert_eq!(serialized[0], Felt::new(999));
    assert_eq!(serialized[1], Felt::new(1));

    let onchain_code = r#"(input: [Felt; 2]) -> Vec<Felt> {
        use miden_felt_repr_onchain::{FeltReader, FromFeltRepr, ToFeltRepr};

        #[derive(FromFeltRepr, ToFeltRepr)]
        enum SimpleEnum {
            A,
            B,
            C,
        }

        #[derive(FromFeltRepr, ToFeltRepr)]
        struct Wrapper {
            pad: Felt,
            value: SimpleEnum,
        }

        let mut reader = FeltReader::new(&input);
        let deserialized = Wrapper::from_felt_repr(&mut reader);
        deserialized.to_felt_repr()
    }"#;

    let config = WasmTranslationConfig::default();
    let name = "onchain_enum_unit";
    let temp_dir = TempDir::with_prefix(name).unwrap();
    let mut test = build_felt_repr_test(&temp_dir, name, onchain_code, config);
    let package = test.compiled_package();

    let in_elem_addr = 21u32 * 16384;
    let out_elem_addr = 20u32 * 16384;
    let in_byte_addr = in_elem_addr * 4;
    let out_byte_addr = out_elem_addr * 4;

    let initializers = [Initializer::MemoryFelts {
        addr: in_elem_addr,
        felts: Cow::from(serialized.clone()),
    }];

    let args = [Felt::new(in_byte_addr as u64), Felt::new(out_byte_addr as u64)];

    let _: Felt = eval_package(&package, initializers, &args, &test.session, |trace| {
        let result_felts = read_vec_felts(trace, out_byte_addr, 2);
        let mut reader = FeltReader::new(&result_felts);
        let result_struct = Wrapper::from_felt_repr(&mut reader);
        assert_eq!(result_struct, original, "Unit enum round-trip failed");
        Ok(())
    })
    .unwrap();
}

#[test]
fn test_enum_tuple_round_trip() {
    #[derive(Debug, Clone, PartialEq, Eq, FromFeltRepr, ToFeltRepr)]
    enum MixedEnum {
        Unit,
        Pair(Felt, u32),
        Struct { x: u64, flag: bool },
    }

    let original = MixedEnum::Pair(Felt::new(111), 222);
    let serialized = original.to_felt_repr();

    assert_eq!(serialized.len(), 3);
    assert_eq!(serialized[0], Felt::new(1));
    assert_eq!(serialized[1], Felt::new(111));
    assert_eq!(serialized[2], Felt::new(222));

    let onchain_code = r#"(input: [Felt; 3]) -> Vec<Felt> {
        use miden_felt_repr_onchain::{FeltReader, FromFeltRepr, ToFeltRepr};

        #[derive(FromFeltRepr, ToFeltRepr)]
        enum MixedEnum {
            Unit,
            Pair(Felt, u32),
            Struct { x: u64, flag: bool },
        }

        let mut reader = FeltReader::new(&input);
        let deserialized = MixedEnum::from_felt_repr(&mut reader);
        deserialized.to_felt_repr()
    }"#;

    let config = WasmTranslationConfig::default();
    let name = "onchain_enum_tuple";
    let temp_dir = TempDir::with_prefix(name).unwrap();
    let mut test = build_felt_repr_test(&temp_dir, name, onchain_code, config);
    let package = test.compiled_package();

    let in_elem_addr = 21u32 * 16384;
    let out_elem_addr = 20u32 * 16384;
    let in_byte_addr = in_elem_addr * 4;
    let out_byte_addr = out_elem_addr * 4;

    let initializers = [Initializer::MemoryFelts {
        addr: in_elem_addr,
        felts: Cow::from(serialized.clone()),
    }];

    let args = [Felt::new(in_byte_addr as u64), Felt::new(out_byte_addr as u64)];

    let _: Felt = eval_package(&package, initializers, &args, &test.session, |trace| {
        let result_felts = read_vec_felts(trace, out_byte_addr, 3);
        let mut reader = FeltReader::new(&result_felts);
        let result_enum = MixedEnum::from_felt_repr(&mut reader);
        assert_eq!(result_enum, original, "Tuple enum round-trip failed");
        Ok(())
    })
    .unwrap();
}

#[test]
fn test_struct_with_enum_round_trip() {
    #[derive(Debug, Clone, PartialEq, Eq, FromFeltRepr, ToFeltRepr)]
    struct Inner {
        a: Felt,
        b: u32,
    }

    #[derive(Debug, Clone, PartialEq, Eq, FromFeltRepr, ToFeltRepr)]
    enum Kind {
        Empty,
        Inline { inner: Inner, ok: bool },
        Tuple(Inner, u64),
    }

    #[derive(Debug, Clone, PartialEq, Eq, FromFeltRepr, ToFeltRepr)]
    struct Outer {
        prefix: Felt,
        kind: Kind,
        suffix: u8,
    }

    let original = Outer {
        prefix: Felt::new(999),
        kind: Kind::Inline {
            inner: Inner {
                a: Felt::new(111),
                b: 222,
            },
            ok: true,
        },
        suffix: 9,
    };
    let serialized = original.to_felt_repr();

    assert_eq!(serialized.len(), 6);

    let onchain_code = r#"(input: [Felt; 6]) -> Vec<Felt> {
        use miden_felt_repr_onchain::{FeltReader, FromFeltRepr, ToFeltRepr};

        #[derive(FromFeltRepr, ToFeltRepr)]
        struct Inner {
            a: Felt,
            b: u32,
        }

        #[derive(FromFeltRepr, ToFeltRepr)]
        enum Kind {
            Empty,
            Inline { inner: Inner, ok: bool },
            Tuple(Inner, u64),
        }

        #[derive(FromFeltRepr, ToFeltRepr)]
        struct Outer {
            prefix: Felt,
            kind: Kind,
            suffix: u8,
        }

        let mut reader = FeltReader::new(&input);
        let deserialized = Outer::from_felt_repr(&mut reader);
        deserialized.to_felt_repr()
    }"#;

    let config = WasmTranslationConfig::default();
    let name = "onchain_struct_with_enum";
    let temp_dir = TempDir::with_prefix(name).unwrap();
    let mut test = build_felt_repr_test(&temp_dir, name, onchain_code, config);
    let package = test.compiled_package();

    let in_elem_addr = 21u32 * 16384;
    let out_elem_addr = 20u32 * 16384;
    let in_byte_addr = in_elem_addr * 4;
    let out_byte_addr = out_elem_addr * 4;

    let initializers = [Initializer::MemoryFelts {
        addr: in_elem_addr,
        felts: Cow::from(serialized.clone()),
    }];

    let args = [Felt::new(in_byte_addr as u64), Felt::new(out_byte_addr as u64)];

    let _: Felt = eval_package(&package, initializers, &args, &test.session, |trace| {
        let result_felts = read_vec_felts(trace, out_byte_addr, 6);
        let mut reader = FeltReader::new(&result_felts);
        let result_struct = Outer::from_felt_repr(&mut reader);
        assert_eq!(result_struct, original, "Struct-with-enum round-trip failed");
        Ok(())
    })
    .unwrap();
}

#[test]
fn test_enum_nested_with_struct_round_trip() {
    #[derive(Debug, Clone, PartialEq, Eq, FromFeltRepr, ToFeltRepr)]
    enum State {
        A,
        B { n: u64, f: bool },
    }

    #[derive(Debug, Clone, PartialEq, Eq, FromFeltRepr, ToFeltRepr)]
    struct Wrapper {
        left: Felt,
        right: Felt,
        state: State,
    }

    #[derive(Debug, Clone, PartialEq, Eq, FromFeltRepr, ToFeltRepr)]
    enum Top {
        None,
        Wrap(Wrapper),
    }

    let original = Top::Wrap(Wrapper {
        left: Felt::new(7),
        right: Felt::new(8),
        state: State::B {
            n: 999_999,
            f: false,
        },
    });
    let serialized = original.to_felt_repr();

    assert_eq!(serialized.len(), 6);

    let onchain_code = r#"(input: [Felt; 6]) -> Vec<Felt> {
        use miden_felt_repr_onchain::{FeltReader, FromFeltRepr, ToFeltRepr};

        #[derive(FromFeltRepr, ToFeltRepr)]
        enum State {
            A,
            B { n: u64, f: bool },
        }

        #[derive(FromFeltRepr, ToFeltRepr)]
        struct Wrapper {
            left: Felt,
            right: Felt,
            state: State,
        }

        #[derive(FromFeltRepr, ToFeltRepr)]
        enum Top {
            None,
            Wrap(Wrapper),
        }

        let mut reader = FeltReader::new(&input);
        let deserialized = Top::from_felt_repr(&mut reader);
        deserialized.to_felt_repr()
    }"#;

    let config = WasmTranslationConfig::default();
    let name = "onchain_enum_nested_struct";
    let temp_dir = TempDir::with_prefix(name).unwrap();
    let mut test = build_felt_repr_test(&temp_dir, name, onchain_code, config);
    let package = test.compiled_package();

    let in_elem_addr = 21u32 * 16384;
    let out_elem_addr = 20u32 * 16384;
    let in_byte_addr = in_elem_addr * 4;
    let out_byte_addr = out_elem_addr * 4;

    let initializers = [Initializer::MemoryFelts {
        addr: in_elem_addr,
        felts: Cow::from(serialized.clone()),
    }];

    let args = [Felt::new(in_byte_addr as u64), Felt::new(out_byte_addr as u64)];

    let _: Felt = eval_package(&package, initializers, &args, &test.session, |trace| {
        let result_felts = read_vec_felts(trace, out_byte_addr, 6);
        let mut reader = FeltReader::new(&result_felts);
        let result_enum = Top::from_felt_repr(&mut reader);
        assert_eq!(result_enum, original, "Nested enum round-trip failed");
        Ok(())
    })
    .unwrap();
}
