//! On-chain serialization/deserialization tests.
//!
//! These tests verify the full round-trip: off-chain serialize -> on-chain deserialize/serialize
//! -> off-chain deserialize.

use std::borrow::Cow;

use miden_debug::{ExecutionTrace, Felt as TestFelt};
use miden_felt::Felt;
use miden_felt_repr::{Felt as ReprFelt, FeltReader, FromFeltRepr, ToFeltRepr};
use miden_integration_tests::testing::{Initializer, eval_package};
use midenc_frontend_wasm::WasmTranslationConfig;
use temp_dir::TempDir;

use crate::build_felt_repr_test;

/// Converts `miden-felt-repr` felts to `miden-core` felts for VM memory initialization.
fn to_core_felts(felts: &[ReprFelt]) -> Vec<miden_core::Felt> {
    felts.iter().copied().map(Into::into).collect()
}

/// Reads a `Vec<Felt>` returned via the Rust ABI from VM memory.
fn read_vec_felts(
    trace: &ExecutionTrace,
    vec_meta_addr: u32,
    expected_len: usize,
) -> Vec<ReprFelt> {
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
        result.push(word[elem_in_word].0.into());
    }

    result
}

/// Test struct for round-trip tests.
#[derive(Debug, Clone, PartialEq, Eq, FromFeltRepr, ToFeltRepr)]
struct TwoFelts {
    a: Felt,
    b: Felt,
}

/// Test using `FeltReader` from `miden-felt-repr`.
#[test]
fn test_felt_reader() {
    let original = TwoFelts {
        a: Felt::from_u64_unchecked(12345),
        b: Felt::from_u64_unchecked(67890),
    };
    let serialized = original.to_felt_repr();

    let onchain_code = r#"(input: Word) -> Word {
        use miden_felt_repr::FeltReader;

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

    let input_word: Vec<miden_core::Felt> = vec![
        serialized[0].into(),
        serialized[1].into(),
        miden_core::Felt::new(0),
        miden_core::Felt::new(0),
    ];

    let initializers = [Initializer::MemoryFelts {
        addr: in_elem_addr,
        felts: Cow::from(input_word),
    }];

    let args = [
        miden_core::Felt::new(in_byte_addr as u64),
        miden_core::Felt::new(out_byte_addr as u64),
    ];

    let _: miden_core::Felt = eval_package(&package, initializers, &args, &test.session, |trace| {
        let result_word: [TestFelt; 4] = trace
            .read_from_rust_memory(out_byte_addr)
            .expect("Failed to read result from memory");

        let result_felts = [result_word[0].0.into(), result_word[1].0.into()];
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
        a: Felt::from_u64_unchecked(12345),
        b: Felt::from_u64_unchecked(67890),
    };
    let serialized = original.to_felt_repr();

    let onchain_code = r#"(input: [Felt; 2]) -> Vec<Felt> {
        use miden_felt_repr::{FeltReader, FromFeltRepr, ToFeltRepr};

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

    let input_felts: Vec<miden_core::Felt> = vec![serialized[0].into(), serialized[1].into()];

    let initializers = [Initializer::MemoryFelts {
        addr: in_elem_addr,
        felts: Cow::from(input_felts),
    }];

    let args = [
        miden_core::Felt::new(in_byte_addr as u64),
        miden_core::Felt::new(out_byte_addr as u64),
    ];

    let _: miden_core::Felt = eval_package(&package, initializers, &args, &test.session, |trace| {
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
        a: Felt::from_u64_unchecked(11111),
        b: Felt::from_u64_unchecked(22222),
        c: Felt::from_u64_unchecked(33333),
        d: Felt::from_u64_unchecked(44444),
        e: Felt::from_u64_unchecked(55555),
    };
    let serialized = original.to_felt_repr();

    assert_eq!(serialized.len(), 5);
    assert_eq!(serialized[4], Felt::from_u64_unchecked(55555));

    let onchain_code = r#"(input: [Felt; 5]) -> Vec<Felt> {
        use miden_felt_repr::{FeltReader, FromFeltRepr, ToFeltRepr};

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

    let input_felts: Vec<miden_core::Felt> = to_core_felts(&serialized);

    let initializers = [Initializer::MemoryFelts {
        addr: in_elem_addr,
        felts: Cow::from(input_felts),
    }];

    let args = [
        miden_core::Felt::new(in_byte_addr as u64),
        miden_core::Felt::new(out_byte_addr as u64),
    ];

    let _: miden_core::Felt = eval_package(&package, initializers, &args, &test.session, |trace| {
        let result_felts = read_vec_felts(trace, out_byte_addr, 5);
        let mut reader = FeltReader::new(&result_felts);
        let result_struct = FiveFelts::from_felt_repr(&mut reader);

        assert_eq!(result_struct, original, "Full 5-felt round-trip failed");
        Ok(())
    })
    .unwrap();
}

/// Minimal struct used to reproduce issue #815 ("u64 in a struct leads to heap MAGIC corruption").
///
/// The field mix is chosen to increase register pressure during (de)serialization and force values
/// to be spilled to procedure locals.
#[derive(Debug, Clone, PartialEq, Eq, FromFeltRepr, ToFeltRepr)]
struct MinimalU64Bug {
    n1: u64,
    a: u32,
    b: u32,
    x: u32,
    y: u32,
}

/// Regression test for issue https://github.com/0xMiden/compiler/issues/815
/// Historically, spilling to procedure locals could overwrite the heap header magic, causing the
/// first `Vec` allocation to fail with "rust heap has not been initialized".
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
        use miden_felt_repr::{FeltReader, FromFeltRepr, ToFeltRepr};

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

        // NOTE: Keep `y` live until `to_felt_repr()` to force spilling to locals.

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

    let input_felts: Vec<miden_core::Felt> = to_core_felts(&serialized);

    let initializers = [Initializer::MemoryFelts {
        addr: in_elem_addr,
        felts: Cow::from(input_felts),
    }];

    let args = [
        miden_core::Felt::new(in_byte_addr as u64),
        miden_core::Felt::new(out_byte_addr as u64),
    ];

    let _: miden_core::Felt = eval_package(&package, initializers, &args, &test.session, |trace| {
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
        f1: Felt::from_u64_unchecked(111111),
        f2: Felt::from_u64_unchecked(222222),
        f3: Felt::from_u64_unchecked(333333),
        f4: Felt::from_u64_unchecked(444444),
        x: 55555,
        y: 66,
    };
    let serialized = original.to_felt_repr();

    // Each field serializes to one Felt
    assert_eq!(serialized.len(), 6);

    let onchain_code = r#"(input: [Felt; 6]) -> Vec<Felt> {
        use miden_felt_repr::{FeltReader, FromFeltRepr, ToFeltRepr};

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

    let input_felts: Vec<miden_core::Felt> = to_core_felts(&serialized);

    let initializers = [Initializer::MemoryFelts {
        addr: in_elem_addr,
        felts: Cow::from(input_felts),
    }];

    let args = [
        miden_core::Felt::new(in_byte_addr as u64),
        miden_core::Felt::new(out_byte_addr as u64),
    ];

    let _: miden_core::Felt = eval_package(&package, initializers, &args, &test.session, |trace| {
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
        a: Felt::from_u64_unchecked(111111),
        inner: Inner {
            x: Felt::from_u64_unchecked(222222),
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
        use miden_felt_repr::{FeltReader, FromFeltRepr, ToFeltRepr};

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

    let input_felts: Vec<miden_core::Felt> = to_core_felts(&serialized);

    let initializers = [Initializer::MemoryFelts {
        addr: in_elem_addr,
        felts: Cow::from(input_felts),
    }];

    let args = [
        miden_core::Felt::new(in_byte_addr as u64),
        miden_core::Felt::new(out_byte_addr as u64),
    ];

    let _: miden_core::Felt = eval_package(&package, initializers, &args, &test.session, |trace| {
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
        pad: Felt::from_u64_unchecked(999),
        value: SimpleEnum::B,
    };
    let serialized = original.to_felt_repr();

    assert_eq!(serialized.len(), 2);
    assert_eq!(serialized[0], Felt::from_u64_unchecked(999));
    assert_eq!(serialized[1], Felt::from_u64_unchecked(1));

    let onchain_code = r#"(input: [Felt; 2]) -> Vec<Felt> {
        use miden_felt_repr::{FeltReader, FromFeltRepr, ToFeltRepr};

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
        felts: Cow::from(to_core_felts(&serialized)),
    }];

    let args = [
        miden_core::Felt::new(in_byte_addr as u64),
        miden_core::Felt::new(out_byte_addr as u64),
    ];

    let _: miden_core::Felt = eval_package(&package, initializers, &args, &test.session, |trace| {
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

    let original = MixedEnum::Pair(Felt::from_u64_unchecked(111), 222);
    let serialized = original.to_felt_repr();

    assert_eq!(serialized.len(), 3);
    assert_eq!(serialized[0], Felt::from_u64_unchecked(1));
    assert_eq!(serialized[1], Felt::from_u64_unchecked(111));
    assert_eq!(serialized[2], Felt::from_u64_unchecked(222));

    let onchain_code = r#"(input: [Felt; 3]) -> Vec<Felt> {
        use miden_felt_repr::{FeltReader, FromFeltRepr, ToFeltRepr};

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
        felts: Cow::from(to_core_felts(&serialized)),
    }];

    let args = [
        miden_core::Felt::new(in_byte_addr as u64),
        miden_core::Felt::new(out_byte_addr as u64),
    ];

    let _: miden_core::Felt = eval_package(&package, initializers, &args, &test.session, |trace| {
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
        prefix: Felt::from_u64_unchecked(999),
        kind: Kind::Inline {
            inner: Inner {
                a: Felt::from_u64_unchecked(111),
                b: 222,
            },
            ok: true,
        },
        suffix: 9,
    };
    let serialized = original.to_felt_repr();

    assert_eq!(serialized.len(), 6);

    let onchain_code = r#"(input: [Felt; 6]) -> Vec<Felt> {
        use miden_felt_repr::{FeltReader, FromFeltRepr, ToFeltRepr};

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
        felts: Cow::from(to_core_felts(&serialized)),
    }];

    let args = [
        miden_core::Felt::new(in_byte_addr as u64),
        miden_core::Felt::new(out_byte_addr as u64),
    ];

    let _: miden_core::Felt = eval_package(&package, initializers, &args, &test.session, |trace| {
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
        left: Felt::from_u64_unchecked(7),
        right: Felt::from_u64_unchecked(8),
        state: State::B {
            n: 999_999,
            f: false,
        },
    });
    let serialized = original.to_felt_repr();

    assert_eq!(serialized.len(), 6);

    let onchain_code = r#"(input: [Felt; 6]) -> Vec<Felt> {
        use miden_felt_repr::{FeltReader, FromFeltRepr, ToFeltRepr};

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
        felts: Cow::from(to_core_felts(&serialized)),
    }];

    let args = [
        miden_core::Felt::new(in_byte_addr as u64),
        miden_core::Felt::new(out_byte_addr as u64),
    ];

    let _: miden_core::Felt = eval_package(&package, initializers, &args, &test.session, |trace| {
        let result_felts = read_vec_felts(trace, out_byte_addr, 6);
        let mut reader = FeltReader::new(&result_felts);
        let result_enum = Top::from_felt_repr(&mut reader);
        assert_eq!(result_enum, original, "Nested enum round-trip failed");
        Ok(())
    })
    .unwrap();
}

/// Test struct containing an `Option` field for on-chain/off-chain round-trip tests.
#[derive(Debug, Clone, PartialEq, Eq, FromFeltRepr, ToFeltRepr)]
struct WithOption {
    prefix: Felt,
    maybe: Option<u32>,
    suffix: bool,
}

#[test]
fn test_struct_with_option_round_trip() {
    let original_none = WithOption {
        prefix: Felt::from_u64_unchecked(7),
        maybe: None,
        suffix: false,
    };
    let original_some = WithOption {
        prefix: Felt::from_u64_unchecked(5),
        maybe: Some(42),
        suffix: true,
    };

    let serialized_none = original_none.to_felt_repr();
    let serialized_some = original_some.to_felt_repr();

    assert_eq!(serialized_none.len(), 3);
    assert_eq!(serialized_some.len(), 4);

    let onchain_code = r#"(input: [Felt; 4]) -> Vec<Felt> {
        use miden_felt_repr::{FeltReader, FromFeltRepr, ToFeltRepr};

        #[derive(FromFeltRepr, ToFeltRepr)]
        struct WithOption {
            prefix: Felt,
            maybe: Option<u32>,
            suffix: bool,
        }

        let mut reader = FeltReader::new(&input);
        let deserialized = WithOption::from_felt_repr(&mut reader);

        deserialized.to_felt_repr()
    }"#;

    let config = WasmTranslationConfig::default();
    let name = "onchain_struct_with_option";
    let temp_dir = TempDir::with_prefix(name).unwrap();
    let mut test = build_felt_repr_test(&temp_dir, name, onchain_code, config);
    let package = test.compiled_package();

    let in_elem_addr = 21u32 * 16384;
    let out_elem_addr = 20u32 * 16384;
    let in_byte_addr = in_elem_addr * 4;
    let out_byte_addr = out_elem_addr * 4;

    // Case 1: `None` serializes to 3 felts, but the compiled on-chain entrypoint takes
    // `[Felt; 4]` so we can reuse the same compiled package for both `None` and `Some`.
    // The extra trailing `0` is never read by `FromFeltRepr`.
    let mut input_none = serialized_none.clone();
    input_none.resize(4, ReprFelt::from_u64_unchecked(0));
    let initializers = [Initializer::MemoryFelts {
        addr: in_elem_addr,
        felts: Cow::from(to_core_felts(&input_none)),
    }];
    let args = [
        miden_core::Felt::new(in_byte_addr as u64),
        miden_core::Felt::new(out_byte_addr as u64),
    ];
    let _: miden_core::Felt = eval_package(&package, initializers, &args, &test.session, |trace| {
        let result_felts = read_vec_felts(trace, out_byte_addr, serialized_none.len());
        let mut reader = FeltReader::new(&result_felts);
        let result_struct = WithOption::from_felt_repr(&mut reader);
        assert_eq!(result_struct, original_none, "Option round-trip (None) failed");
        Ok(())
    })
    .unwrap();

    // Case 2: Some
    let initializers = [Initializer::MemoryFelts {
        addr: in_elem_addr,
        felts: Cow::from(to_core_felts(&serialized_some)),
    }];
    let _: miden_core::Felt = eval_package(&package, initializers, &args, &test.session, |trace| {
        let result_felts = read_vec_felts(trace, out_byte_addr, serialized_some.len());
        let mut reader = FeltReader::new(&result_felts);
        let result_struct = WithOption::from_felt_repr(&mut reader);
        assert_eq!(result_struct, original_some, "Option round-trip (Some) failed");
        Ok(())
    })
    .unwrap();
}

/// Test struct containing a `Vec` field for on-chain/off-chain round-trip tests.
#[derive(Debug, Clone, PartialEq, Eq, FromFeltRepr, ToFeltRepr)]
struct WithVec {
    prefix: Felt,
    items: Vec<u8>,
    suffix: bool,
}

#[test]
fn test_struct_with_vec_round_trip() {
    let original = WithVec {
        prefix: Felt::from_u64_unchecked(9),
        items: vec![1, 2, 3],
        suffix: true,
    };
    let serialized = original.to_felt_repr();
    assert_eq!(serialized.len(), 6);

    let onchain_code = r#"(input: [Felt; 6]) -> Vec<Felt> {
        use miden_felt_repr::{FeltReader, FromFeltRepr, ToFeltRepr};

        #[derive(FromFeltRepr, ToFeltRepr)]
        struct WithVec {
            prefix: Felt,
            items: Vec<u8>,
            suffix: bool,
        }

        let mut reader = FeltReader::new(&input);
        let deserialized = WithVec::from_felt_repr(&mut reader);
        deserialized.to_felt_repr()
    }"#;

    let config = WasmTranslationConfig::default();
    let name = "onchain_struct_with_vec";
    let temp_dir = TempDir::with_prefix(name).unwrap();
    let mut test = build_felt_repr_test(&temp_dir, name, onchain_code, config);
    let package = test.compiled_package();

    let in_elem_addr = 21u32 * 16384;
    let out_elem_addr = 20u32 * 16384;
    let in_byte_addr = in_elem_addr * 4;
    let out_byte_addr = out_elem_addr * 4;

    let initializers = [Initializer::MemoryFelts {
        addr: in_elem_addr,
        felts: Cow::from(to_core_felts(&serialized)),
    }];

    let args = [
        miden_core::Felt::new(in_byte_addr as u64),
        miden_core::Felt::new(out_byte_addr as u64),
    ];

    let _: miden_core::Felt = eval_package(&package, initializers, &args, &test.session, |trace| {
        let result_felts = read_vec_felts(trace, out_byte_addr, 6);
        let mut reader = FeltReader::new(&result_felts);
        let result_struct = WithVec::from_felt_repr(&mut reader);
        assert_eq!(result_struct, original, "Vec round-trip failed");
        Ok(())
    })
    .unwrap();
}

/// Test tuple struct serialization - full round-trip execution.
#[derive(Debug, Clone, PartialEq, Eq, FromFeltRepr, ToFeltRepr)]
struct TupleStruct(u32, bool, Felt);

#[test]
fn test_tuple_struct_round_trip() {
    let original = TupleStruct(22, true, Felt::from_u64_unchecked(33));
    let serialized = original.to_felt_repr();
    assert_eq!(
        serialized,
        vec![
            ReprFelt::from_u64_unchecked(22),
            ReprFelt::from_u64_unchecked(1),
            ReprFelt::from_u64_unchecked(33),
        ]
    );

    let onchain_code = r#"(input: [Felt; 3]) -> Vec<Felt> {
        use miden_felt_repr::{FeltReader, FromFeltRepr, ToFeltRepr};

        #[derive(FromFeltRepr, ToFeltRepr)]
        struct TupleStruct(u32, bool, Felt);

        let mut reader = FeltReader::new(&input);
        let deserialized = TupleStruct::from_felt_repr(&mut reader);
        deserialized.to_felt_repr()
    }"#;

    let config = WasmTranslationConfig::default();
    let name = "onchain_tuple_struct";
    let temp_dir = TempDir::with_prefix(name).unwrap();
    let mut test = build_felt_repr_test(&temp_dir, name, onchain_code, config);
    let package = test.compiled_package();

    let in_elem_addr = 21u32 * 16384;
    let out_elem_addr = 20u32 * 16384;
    let in_byte_addr = in_elem_addr * 4;
    let out_byte_addr = out_elem_addr * 4;

    let initializers = [Initializer::MemoryFelts {
        addr: in_elem_addr,
        felts: Cow::from(to_core_felts(&serialized)),
    }];

    let args = [
        miden_core::Felt::new(in_byte_addr as u64),
        miden_core::Felt::new(out_byte_addr as u64),
    ];

    let _: miden_core::Felt = eval_package(&package, initializers, &args, &test.session, |trace| {
        let result_felts = read_vec_felts(trace, out_byte_addr, 3);
        let mut reader = FeltReader::new(&result_felts);
        let result_struct = TupleStruct::from_felt_repr(&mut reader);
        assert_eq!(result_struct, original, "Tuple struct round-trip failed");
        Ok(())
    })
    .unwrap();
}
