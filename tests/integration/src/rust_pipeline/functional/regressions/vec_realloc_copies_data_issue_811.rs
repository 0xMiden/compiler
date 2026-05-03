use miden_core::Felt;
use midenc_expect_test::expect_file;
use midenc_frontend_wasm::WasmTranslationConfig;

use crate::{
    CompilerTest,
    testing::{eval_package, setup},
};

/// Regression test for https://github.com/0xMiden/compiler/issues/811.
#[test]
fn test_vec_realloc_copies_data_issue_811() {
    let main_fn = r#"() -> Felt {
        extern crate alloc;
        use alloc::vec::Vec;

        // Create a Vec with a tiny capacity to make growth (and thus reallocation) likely.
        let mut v: Vec<Felt> = Vec::with_capacity(1);

        v.push(felt!(11111));
        let mut last_ptr = v.as_ptr() as u32;
        let mut moves: u32 = 0;

        v.push(felt!(22222));
        let ptr = v.as_ptr() as u32;
        if ptr != last_ptr {
            moves += 1;
            last_ptr = ptr;
        }

        v.push(felt!(33333));
        let ptr = v.as_ptr() as u32;
        if ptr != last_ptr {
            moves += 1;
            last_ptr = ptr;
        }

        v.push(felt!(44444));
        let ptr = v.as_ptr() as u32;
        if ptr != last_ptr {
            moves += 1;
            last_ptr = ptr;
        }

        v.push(felt!(55555));
        let ptr = v.as_ptr() as u32;
        if ptr != last_ptr {
            moves += 1;
            last_ptr = ptr;
        }

        // Sum all elements - if realloc doesn't copy, the first 4 elements will be garbage.
        let sum = v[0] + v[1] + v[2] + v[3] + v[4];
        if moves >= 2 { sum } else { felt!(0) }
    }"#;

    setup::enable_compiler_instrumentation();
    let config = WasmTranslationConfig::default();
    let mut test =
        CompilerTest::rust_fn_body_with_stdlib_sys("vec_realloc_copies_data", main_fn, config, []);

    let package = test.compile_package();
    let args: [Felt; 0] = [];

    eval_package::<Felt, _, _>(&package, [], &args, &test.session, |trace| {
        let result: u64 = trace.parse_result::<Felt>().unwrap().as_canonical_u64();
        assert_eq!(result, 166_665, "Vec reallocation failed to copy existing elements");
        Ok(())
    })
    .unwrap();
}
