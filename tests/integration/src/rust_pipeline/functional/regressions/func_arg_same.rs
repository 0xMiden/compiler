use miden_core::Felt;
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

    let package = test.compile_package();

    let addr1: u32 = 10 * 65536;
    let addr2: u32 = 11 * 65536;

    // Test 1: addr1 is passed as x and should be returned
    let args1 = [Felt::from(addr1), Felt::from(addr2)];
    eval_package::<i32, _, _>(&package, [], &args1, &test.session, |trace| {
        let result: u32 = trace.parse_result().unwrap();
        assert_eq!(result, addr1);
        Ok(())
    })
    .unwrap();

    // Test 1: addr2 is passed as x and should be returned
    let args2 = [Felt::from(addr2), Felt::from(addr1)];
    eval_package::<i32, _, _>(&package, [], &args2, &test.session, |trace| {
        let result: u32 = trace.parse_result().unwrap();
        assert_eq!(result, addr2);
        Ok(())
    })
    .unwrap();
}
