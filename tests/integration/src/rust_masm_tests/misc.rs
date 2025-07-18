use miden_core::{Felt, FieldElement};
use midenc_expect_test::expect_file;
use midenc_frontend_wasm::WasmTranslationConfig;

use crate::{
    testing::{eval_package, setup},
    CompilerTest,
};

#[test]
fn test_func_arg_same() {
    // Test for issue https://github.com/0xMiden/compiler/issues/600
    // Verifies that intrinsic function arguments are passed in the correct order. The bug was that
    // operations with exactly 2 arguments were incorrectly treated as binary operations for stack
    // scheduling, causing the arguments order to be "frozen", i.e. (x, y) was the same as (y, x).
    let main_fn = r#"
        (x: &mut Felt, y: &mut Felt) -> i32 {
            intrinsic(x, y)
        }

        #[no_mangle]
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
            // ATTENTION: the digests_ptr is correct (__stack_pointer - 96)
            // see wat/hir
            assert_eq(Felt::from_u32(digests_ptr as u32), Felt::from_u32(1048480));

            let mut ret_area = ::core::mem::MaybeUninit::<Word>::uninit();
            let result_ptr = ret_area.as_mut_ptr().addr() as u32;
            // ATTENTION: the result_ptr is expected to be 1048544 (__stack_pointer - 96 + 64)
            // see wat/hir
            assert_eq(Felt::from_u32(result_ptr as u32), Felt::from_u32(1048544));

            intrinsic({} as *const Felt, {} as *mut Felt);

            Digest::from_word(ret_area.assume_init())
        }}
    }}

    #[no_mangle]
    fn intrinsic(digests_ptr: *const Felt, result_ptr: *mut Felt) {{
        // ATTENTION: the digests_ptr is expected to be 1048480 (__stack_pointer - 96)
        // see assert_eq above, before the call
        assert_eq(Felt::from_u32(digests_ptr as u32), Felt::from_u32(1048480));
        // ATTENTION: the result_ptr is expected to be 1048544 (__stack_pointer - 96 + 64)
        // see assert_eq above, before the call
        assert_eq(Felt::from_u32(result_ptr as u32), Felt::from_u32(1048544));
    }}
        "#,
            digest_ptr_name, result_ptr_name
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
