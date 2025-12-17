use midenc_expect_test::expect_file;
use midenc_frontend_wasm::WasmTranslationConfig;
use midenc_hir::Felt;
use proptest::{
    arbitrary::any,
    prop_assert_eq,
    test_runner::{TestError, TestRunner},
};
use sha2::{Digest, Sha256};

use crate::{
    CompilerTest,
    testing::{Initializer, eval_package},
};

/// Runs the provided stdlib hash function against random 32-byte inputs and compares the outputs
/// between Rust and the compiled MASM program.
fn run_stdlib_hash_1to1_test<F>(artifact_name: &'static str, rust_call_expr: &str, expected_fn: F)
where
    F: Fn(&[u8; 32]) -> [u8; 32] + Copy,
{
    let main_fn = format!("(a: [u8; 32]) -> [u8; 32] {{ {rust_call_expr} }}");
    let config = WasmTranslationConfig::default();
    let mut test = CompilerTest::rust_fn_body_with_stdlib_sys(
        artifact_name,
        &main_fn,
        config,
        ["--test-harness".into()],
    );
    test.expect_wasm(expect_file![format!("../../../../expected/{artifact_name}.wat")]);
    test.expect_ir(expect_file![format!("../../../../expected/{artifact_name}.hir")]);
    test.expect_masm(expect_file![format!("../../../../expected/{artifact_name}.masm")]);

    let package = test.compiled_package();

    let config = proptest::test_runner::Config::with_cases(10);
    let res = TestRunner::new(config).run(&any::<[u8; 32]>(), move |ibytes| {
        let rs_out = expected_fn(&ibytes);

        let in_addr = 21u32 * 65536;
        let out_addr = 20u32 * 65536;
        let initializers = [Initializer::MemoryBytes {
            addr: in_addr,
            bytes: &ibytes,
        }];

        let args = [Felt::new(in_addr as u64), Felt::new(out_addr as u64)];
        eval_package::<Felt, _, _>(&package, initializers, &args, &test.session, |trace| {
            let vm_in: [u8; 32] = trace
                .read_from_rust_memory(in_addr)
                .expect("expected memory to have been written");
            prop_assert_eq!(&ibytes, &vm_in, "VM input mismatch");
            let vm_out: [u8; 32] = trace
                .read_from_rust_memory(out_addr)
                .expect("expected memory to have been written");
            prop_assert_eq!(&rs_out, &vm_out, "VM output mismatch");
            Ok(())
        })?;

        Ok(())
    });

    if let Err(TestError::Fail(_, value)) = res {
        panic!("Found minimal(shrunk) failing case: {value:?}");
    }

    assert!(res.is_ok(), "Unexpected test result: {res:?}");
}

/// Runs the provided stdlib hash function against random 64-byte inputs and compares the outputs
/// between Rust and the compiled MASM program.
fn run_stdlib_hash_2to1_test<F>(artifact_name: &'static str, rust_call_expr: &str, expected_fn: F)
where
    F: Fn(&[u8; 64]) -> [u8; 32] + Copy,
{
    let main_fn = format!("(a: [u8; 64]) -> [u8; 32] {{ {rust_call_expr} }}");
    let config = WasmTranslationConfig::default();
    let mut test = CompilerTest::rust_fn_body_with_stdlib_sys(
        artifact_name,
        &main_fn,
        config,
        ["--test-harness".into()],
    );

    let package = test.compiled_package();

    let config = proptest::test_runner::Config::with_cases(4);
    let res = TestRunner::new(config).run(&any::<[u8; 64]>(), move |ibytes| {
        let rs_out = expected_fn(&ibytes);

        let in_addr = 21u32 * 65536;
        let out_addr = 20u32 * 65536;
        let initializers = [Initializer::MemoryBytes {
            addr: in_addr,
            bytes: &ibytes,
        }];

        let args = [Felt::new(in_addr as u64), Felt::new(out_addr as u64)];
        eval_package::<Felt, _, _>(&package, initializers, &args, &test.session, |trace| {
            let vm_in: [u8; 64] = trace
                .read_from_rust_memory(in_addr)
                .expect("expected memory to have been written");
            prop_assert_eq!(&ibytes, &vm_in, "VM input mismatch");
            let vm_out: [u8; 32] = trace
                .read_from_rust_memory(out_addr)
                .expect("expected memory to have been written");
            prop_assert_eq!(&rs_out, &vm_out, "VM output mismatch");
            Ok(())
        })?;

        Ok(())
    });

    if let Err(TestError::Fail(_, value)) = res {
        panic!("Found minimal(shrunk) failing case: {value:?}");
    }

    assert!(res.is_ok(), "Unexpected test result: {res:?}");
}

/// Tests the BLAKE3 hash helper exported by the Rust stdlib bindings.
#[test]
fn test_blake3_1to1_hash() {
    run_stdlib_hash_1to1_test(
        "abi_transform_stdlib_blake3_hash",
        "miden_stdlib_sys::blake3_hash_1to1(a)",
        |ibytes| {
            let hash = blake3::hash(ibytes);
            let mut output = [0u8; 32];
            output.copy_from_slice(hash.as_bytes());
            output
        },
    );
}

/// Tests the SHA-256 hash helper exported by the Rust stdlib bindings.
#[test]
fn test_sha256_1to1_hash() {
    run_stdlib_hash_1to1_test(
        "rust_sdk_stdlib_sha256_hash",
        "miden_stdlib_sys::sha256_hash_1to1(a)",
        |ibytes| {
            let hash = Sha256::digest(ibytes);
            let mut output = [0u8; 32];
            output.copy_from_slice(&hash);
            output
        },
    );
}

/// Tests the BLAKE3 hash helper (2-to-1) via the full compilation pipeline.
#[test]
#[ignore = "requires large stack frame; kept for reference"]
fn test_blake3_hash_2to1() {
    run_stdlib_hash_2to1_test(
        "abi_transform_stdlib_blake3_hash_2to1",
        "miden_stdlib_sys::blake3_hash_2to1(a)",
        |ibytes| {
            let hash = blake3::hash(ibytes);
            let mut output = [0u8; 32];
            output.copy_from_slice(hash.as_bytes());
            output
        },
    );
}

/// Tests the SHA-256 hash helper (2-to-1) via the full compilation pipeline.
#[test]
#[ignore = "requires large stack frame; kept for reference"]
fn test_sha256_hash_2to1() {
    run_stdlib_hash_2to1_test(
        "rust_sdk_stdlib_sha256_hash_2to1",
        "miden_stdlib_sys::sha256_hash_2to1(a)",
        |ibytes| {
            let hash = Sha256::digest(ibytes);
            let mut output = [0u8; 32];
            output.copy_from_slice(&hash);
            output
        },
    );
}
