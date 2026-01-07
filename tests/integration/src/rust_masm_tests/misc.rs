use miden_core::{Felt, FieldElement};
use midenc_expect_test::expect_file;
use midenc_frontend_wasm::WasmTranslationConfig;

use crate::{
    CompilerTest,
    testing::{Initializer, eval_package, setup},
};

/// Compiles a Rust entrypoint body using `miden-stdlib-sys` and returns the resulting test harness.
///
/// This is useful for regressions where the issue may occur during compilation or execution.
fn compile_rust_fn_body_with_stdlib_sys(
    name: &'static str,
    main_fn: &str,
    midenc_flags: impl IntoIterator<Item = String>,
) -> CompilerTest {
    let config = WasmTranslationConfig::default();
    CompilerTest::rust_fn_body_with_stdlib_sys(name, main_fn, config, midenc_flags)
}

#[test]
fn test_func_arg_same() {
    // This test reproduces the https://github.com/0xMiden/compiler/issues/606
    let main_fn = r#"
        (x: &mut Felt, y: &mut Felt) -> i32 {
            intrinsic(x, y)
        }

        #[unsafe(no_mangle)]
        #[inline(never)]
        fn intrinsic(a: &mut Felt, b: &mut Felt) -> i32 {
            unsafe { (a as *mut Felt) as i32 }
        }

    "#;

    setup::enable_compiler_instrumentation();
    let config = WasmTranslationConfig::default();
    let mut test = CompilerTest::rust_fn_body_with_stdlib_sys("func_arg_same", main_fn, config, []);

    test.expect_wasm(expect_file!["../../expected/func_arg_same.wat"]);
    test.expect_ir(expect_file!["../../expected/func_arg_same.hir"]);
    test.expect_masm(expect_file!["../../expected/func_arg_same.masm"]);

    let package = test.compiled_package();

    let addr1: u32 = 10 * 65536;
    let addr2: u32 = 11 * 65536;

    // Test 1: addr1 is passed as x and should be returned
    let args1 = [Felt::from(addr2), Felt::from(addr1)]; // Arguments are pushed in reverse order on stack
    eval_package::<i32, _, _>(&package, [], &args1, &test.session, |trace| {
        let result: u32 = trace.parse_result().unwrap();
        assert_eq!(result, addr1);
        Ok(())
    })
    .unwrap();

    // Test 1: addr2 is passed as x and should be returned
    let args2 = [Felt::from(addr1), Felt::from(addr2)]; // Arguments are pushed in reverse order on stack
    eval_package::<i32, _, _>(&package, [], &args2, &test.session, |trace| {
        let result: u32 = trace.parse_result().unwrap();
        assert_eq!(result, addr2);
        Ok(())
    })
    .unwrap();
}

#[ignore = "too fragile (depends on mem addrs), this bug is also covered by the test_hmerge test"]
#[test]
fn test_func_arg_order() {
    // This test reproduces the "swapped and frozen" function arguments issue
    // https://github.com/0xMiden/compiler/pull/576 discovered while working on hmerge VM op
    // The issue manifests in "intrinsic" function parameters being in the wrong order
    // (see assert_eq before the call and inside the function)
    // on the stack AND the their order is not changing when the parameters are
    // swapped at the call site (see expect_masm with the same file name, i.e. the MASM
    // do not change when she parameters are swapped).
    fn main_fn_template(digest_ptr_name: &str, result_ptr_name: &str) -> String {
        format!(
            r#"
    (f0: miden_stdlib_sys::Felt, f1: miden_stdlib_sys::Felt, f2: miden_stdlib_sys::Felt, f3: miden_stdlib_sys::Felt, f4: miden_stdlib_sys::Felt, f5: miden_stdlib_sys::Felt, f6: miden_stdlib_sys::Felt, f7: miden_stdlib_sys::Felt) -> miden_stdlib_sys::Felt {{
        let digest1 = miden_stdlib_sys::Digest::new([f0, f1, f2, f3]);
        let digest2 = miden_stdlib_sys::Digest::new([f4, f5, f6, f7]);
        let digests = [digest1, digest2];
        let res = merge(digests);
        res.inner.inner.0
    }}

    #[inline]
    pub fn merge(digests: [Digest; 2]) -> Digest {{
        unsafe {{
            let digests_ptr = digests.as_ptr().addr() as u32;
            assert_eq(Felt::from_u32(digests_ptr as u32), Felt::from_u32(1048528));

            let mut ret_area = ::core::mem::MaybeUninit::<Word>::uninit();
            let result_ptr = ret_area.as_mut_ptr().addr() as u32;
            assert_eq(Felt::from_u32(result_ptr as u32), Felt::from_u32(1048560));

            intrinsic({digest_ptr_name} as *const Felt, {result_ptr_name} as *mut Felt);

            Digest::from_word(ret_area.assume_init())
        }}
    }}

    #[unsafe(no_mangle)]
    fn intrinsic(digests_ptr: *const Felt, result_ptr: *mut Felt) {{
        // see assert_eq above, before the call
        assert_eq(Felt::from_u32(digests_ptr as u32), Felt::from_u32(1048528));
        // see assert_eq above, before the call
        assert_eq(Felt::from_u32(result_ptr as u32), Felt::from_u32(1048560));
    }}
        "#
        )
    }

    let config = WasmTranslationConfig::default();
    let main_fn_correct = main_fn_template("digests_ptr", "result_ptr");
    let mut test = CompilerTest::rust_fn_body_with_stdlib_sys(
        "func_arg_order",
        &main_fn_correct,
        config.clone(),
        [],
    );

    test.expect_wasm(expect_file![format!("../../expected/func_arg_order.wat")]);
    test.expect_ir(expect_file![format!("../../expected/func_arg_order.hir")]);
    test.expect_masm(expect_file![format!("../../expected/func_arg_order.masm")]);

    let args = [
        Felt::ZERO,
        Felt::ZERO,
        Felt::ZERO,
        Felt::ZERO,
        Felt::ZERO,
        Felt::ZERO,
        Felt::ZERO,
        Felt::ZERO,
    ];

    eval_package::<Felt, _, _>(&test.compiled_package(), [], &args, &test.session, |trace| Ok(()))
        .unwrap();
}

#[test]
fn test_issue_831_invalid_stack_offset_movup_16_args_15() {
    // This test reproduces https://github.com/0xMiden/compiler/issues/831
    //
    // The callee has a flattened argument payload size of 15 felts:
    // - 7 `Felt` (7)
    // - 4 `u64`  (8)
    // Total: 15
    let main_fn = r#"(a0: Felt, a1: Felt, a2: Felt, a3: Felt, a4: Felt, a5: Felt, a6: Felt, a7: Felt, a8: Felt, a9: Felt, a10: Felt, a11: Felt, a12: Felt, a13: Felt, a14: Felt) -> Felt {
            let b0 = a0 + a1;
            let b1 = a2 * a3;
            let b2 = a4 + a5;
            let b3 = a6 + a7;

            consume_15(a0, a1, b0, b1, b2, b3, a8, a9.as_u64(), a10.as_u64(), a11.as_u64(), a12.as_u64(), {
                let v = alloc::vec![a13, a14, b0, b1];
                let _ = v[0];
            })
        }

        #[inline(never)]
        fn consume_15(
            a0: Felt,
            a1: Felt,
            a2: Felt,
            a3: Felt,
            a4: Felt,
            a5: Felt,
            a6: Felt,
            d0: u64,
            d1: u64,
            d2: u64,
            d3: u64,
            _: (),
        ) -> Felt {
            let mix = (d0 ^ d1 ^ d2 ^ d3) as u32;
            a0 + a1 + a2 + a3 + a4 + a5 + a6 + Felt::from_u32(mix)
        }
    "#;

    let mut test = compile_rust_fn_body_with_stdlib_sys("issue_831_args_15", main_fn, []);
    let package = test.compiled_package();

    let a0 = Felt::from(1u32);
    let a1 = Felt::from(2u32);
    let a2 = Felt::from(3u32);
    let a3 = Felt::from(4u32);
    let a4 = Felt::from(5u32);
    let a5 = Felt::from(6u32);
    let a6 = Felt::from(7u32);
    let a7 = Felt::from(8u32);
    let a8 = Felt::from(9u32);
    let a9 = Felt::from(10u32);
    let a10 = Felt::from(20u32);
    let a11 = Felt::from(30u32);
    let a12 = Felt::from(40u32);
    let a13 = Felt::from(13u32);
    let a14 = Felt::from(14u32);

    // Note: arguments are pushed on the operand stack in reverse order.
    let args = [a14, a13, a12, a11, a10, a9, a8, a7, a6, a5, a4, a3, a2, a1, a0];

    let b0 = a0 + a1;
    let b1 = a2 * a3;
    let b2 = a4 + a5;
    let b3 = a6 + a7;
    let d0 = a9.as_int();
    let d1 = a10.as_int();
    let d2 = a11.as_int();
    let d3 = a12.as_int();
    let mix = (d0 ^ d1 ^ d2 ^ d3) as u32;
    let expected = a0 + a1 + b0 + b1 + b2 + b3 + a8 + Felt::from(mix);

    let output =
        eval_package::<Felt, _, _>(&package, [], &args, &test.session, |_| Ok(())).unwrap();
    assert_eq!(output, expected);
}

#[test]
fn test_issue_831_invalid_stack_offset_movup_16_args_16() {
    // This test reproduces https://github.com/0xMiden/compiler/issues/831
    //
    // The callee has a flattened argument payload size of 16 felts:
    // - 8 `Felt` (8)
    // - 4 `u64`  (8)
    // Total: 16
    let main_fn = r#"(a0: Felt, a1: Felt, a2: Felt, a3: Felt, a4: Felt, a5: Felt, a6: Felt, a7: Felt, a8: Felt, a9: Felt, a10: Felt, a11: Felt, a12: Felt, a13: Felt, a14: Felt, a15: Felt) -> Felt {
            let b0 = a0 + a1;
            let b1 = a2 + a3;
            let b2 = a4 * a5;
            let b3 = a6 + a7;

            consume_16(a0, a1, b0, b1, b2, b3, a8, a9, a10.as_u64(), a11.as_u64(), a12.as_u64(), a13.as_u64(), {
                let v = alloc::vec![a14, a15, b0, b2];
                let _ = v[0];
            })
        }

        #[inline(never)]
        fn consume_16(
            a0: Felt,
            a1: Felt,
            a2: Felt,
            a3: Felt,
            a4: Felt,
            a5: Felt,
            a6: Felt,
            a7: Felt,
            d0: u64,
            d1: u64,
            d2: u64,
            d3: u64,
            _: (),
        ) -> Felt {
            let mix = (d0 ^ d1 ^ d2 ^ d3) as u32;
            a0 + a1 + a2 + a3 + a4 + a5 + a6 + a7 + Felt::from_u32(mix)
        }
    "#;

    let mut test = compile_rust_fn_body_with_stdlib_sys("issue_831_args_16", main_fn, []);
    let package = test.compiled_package();

    let a0 = Felt::from(1u32);
    let a1 = Felt::from(2u32);
    let a2 = Felt::from(3u32);
    let a3 = Felt::from(4u32);
    let a4 = Felt::from(5u32);
    let a5 = Felt::from(6u32);
    let a6 = Felt::from(7u32);
    let a7 = Felt::from(8u32);
    let a8 = Felt::from(9u32);
    let a9 = Felt::from(10u32);
    let a10 = Felt::from(10u32);
    let a11 = Felt::from(20u32);
    let a12 = Felt::from(30u32);
    let a13 = Felt::from(40u32);
    let a14 = Felt::from(14u32);
    let a15 = Felt::from(15u32);

    // Note: arguments are pushed on the operand stack in reverse order.
    let args = [a15, a14, a13, a12, a11, a10, a9, a8, a7, a6, a5, a4, a3, a2, a1, a0];

    let b0 = a0 + a1;
    let b1 = a2 + a3;
    let b2 = a4 * a5;
    let b3 = a6 + a7;
    let d0 = a10.as_int();
    let d1 = a11.as_int();
    let d2 = a12.as_int();
    let d3 = a13.as_int();
    let mix = (d0 ^ d1 ^ d2 ^ d3) as u32;
    let expected = a0 + a1 + b0 + b1 + b2 + b3 + a8 + a9 + Felt::from(mix);

    let output =
        eval_package::<Felt, _, _>(&package, [], &args, &test.session, |_| Ok(())).unwrap();
    assert_eq!(output, expected);
}

#[test]
fn test_issue_831_invalid_stack_offset_movup_16_args_17() {
    // This test reproduces https://github.com/0xMiden/compiler/issues/831
    //
    // 17 felts is above the 16-felt cutoff, so these should be passed indirectly via a pointer.
    let main_fn = r#"
        (
            args: [Felt; 17],
        ) -> Felt {
            consume_17(args.as_ptr())
        }

        #[inline(never)]
        fn consume_17(args_ptr: *const Felt) -> Felt {
            let args = unsafe { ::core::slice::from_raw_parts(args_ptr, 17) };
            args.iter()
                .copied()
                .fold(Felt::from_u32(0), |acc, item| acc + item)
        }
    "#;

    let mut test = compile_rust_fn_body_with_stdlib_sys(
        "issue_831_args_17",
        main_fn,
        ["--test-harness".into()],
    );
    let package = test.compiled_package();

    // `eval_package` only supports up to 16 operand stack inputs, so pass a pointer to the 17-felt
    // argument payload, and initialize the payload via the test harness initializers.
    //
    // NOTE: The payload is initialized in element-addressable space. The pointer passed here must
    // match the addressing convention expected by the ABI for indirect arguments.
    let args_base_addr = 20u32 * 65536;
    let payload_element_addr = args_base_addr / 4;

    let payload = vec![
        Felt::from(1u32),
        Felt::from(2u32),
        Felt::from(3u32),
        Felt::from(4u32),
        Felt::from(5u32),
        Felt::from(6u32),
        Felt::from(7u32),
        Felt::from(8u32),
        Felt::from(9u32),
        Felt::from(10u32),
        Felt::from(11u32),
        Felt::from(12u32),
        Felt::from(13u32),
        Felt::from(14u32),
        Felt::from(15u32),
        Felt::from(16u32),
        Felt::from(17u32),
    ];
    let expected = payload.iter().copied().fold(Felt::ZERO, |acc, item| acc + item);

    let initializers = [Initializer::MemoryFelts {
        addr: payload_element_addr,
        felts: (&payload).into(),
    }];

    // Pass the payload pointer as a single argument (see note above).
    let args = [Felt::new(args_base_addr as u64)];

    let output =
        eval_package::<Felt, _, _>(&package, initializers, &args, &test.session, |_| Ok(()))
            .unwrap();
    assert_eq!(output, expected);
}
