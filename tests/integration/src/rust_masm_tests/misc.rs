use miden_core::{Felt, FieldElement};
use midenc_expect_test::expect_file;
use midenc_frontend_wasm::WasmTranslationConfig;

use crate::{
    CompilerTest,
    testing::{eval_package, setup},
};

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

/// Regression test for https://github.com/0xMiden/compiler/issues/872
///
/// Previously, compilation could panic during stack manipulation with:
/// `invalid stack index: only the first 16 elements on the stack are directly accessible, got 16`.
#[test]
fn test_invalid_stack_index_16_issue_872() {
    let main_fn = r#"
        (a0: Felt, a1: Felt, a2: Felt, a3: Felt, a4: Felt, a5: Felt, a6: Felt, a7: Felt,
         a8: Felt, a9: Felt, a10: Felt, a11: Felt, a12: Felt, a13: Felt, a14: Felt, a15: Felt) -> Felt {
            // Keep locals live across the call which are used only after the call, so that the 16
            // call arguments are not at the top of the operand stack at call time.
            let post = a0 + miden_stdlib_sys::felt!(1);

            let res = callee_16(a0, a1, a2, a3, a4, a5, a6, a7, a8, a9, a10, a11, a12, a13, a14, a15);

            // Use all post-call locals to prevent DCE.
            res + post
        }

        #[inline(never)]
        fn callee_16(
            a0: Felt, a1: Felt, a2: Felt, a3: Felt, a4: Felt, a5: Felt, a6: Felt, a7: Felt,
            a8: Felt, a9: Felt, a10: Felt, a11: Felt, a12: Felt, a13: Felt, a14: Felt, a15: Felt,
        ) -> Felt {
            a0 + a1 + a2 + a3 + a4 + a5 + a6 + a7 + a8 + a9 + a10 + a11 + a12 + a13 + a14 + a15
        }
    "#;

    setup::enable_compiler_instrumentation();
    let config = WasmTranslationConfig::default();
    let mut test =
        CompilerTest::rust_fn_body_with_stdlib_sys("movup_16_issue_831", main_fn, config, []);

    let package = test.compiled_package();

    // This should execute and return the expected value.
    // Arguments are pushed in reverse order on stack.
    let args: [Felt; 16] = [
        Felt::from(16u32),
        Felt::from(15u32),
        Felt::from(14u32),
        Felt::from(13u32),
        Felt::from(12u32),
        Felt::from(11u32),
        Felt::from(10u32),
        Felt::from(9u32),
        Felt::from(8u32),
        Felt::from(7u32),
        Felt::from(6u32),
        Felt::from(5u32),
        Felt::from(4u32),
        Felt::from(3u32),
        Felt::from(2u32),
        Felt::from(1u32),
    ];

    let expected = (1u32..=16u32).fold(Felt::ZERO, |acc, x| acc + Felt::from(x)) + Felt::from(2u32);

    eval_package::<Felt, _, _>(&package, [], &args, &test.session, |trace| {
        let res: Felt = trace.parse_result().unwrap();
        assert_eq!(res, expected);
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
