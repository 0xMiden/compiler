use miden_core::Felt;
use midenc_expect_test::expect_file;
use midenc_frontend_wasm::WasmTranslationConfig;

use crate::{testing::eval_package, CompilerTest};

#[test]
fn test_func_arg_order() {
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

    let config = WasmTranslationConfig::default();
    let mut test = CompilerTest::rust_fn_body_with_stdlib_sys("arg_order", main_fn, config, []);

    test.expect_ir(expect_file!["../../expected/arg_order.hir"]);
    test.expect_masm(expect_file!["../../expected/arg_order.masm"]);

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
