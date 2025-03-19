use core::panic;

use expect_test::expect_file;
use miden_core::Felt;
use midenc_debug::{PushToStack, TestFelt};
use proptest::{
    arbitrary::any,
    test_runner::{TestError, TestRunner},
};

use crate::{rust_masm_tests::run_masm_vs_rust, CompilerTest};

/// Compiles, runs VM vs. Rust fuzzing the inputs via proptest
macro_rules! test_bin_op {
    ($name:ident, $op:tt, $op_ty:tt, $res_ty:tt, $a_range:expr, $b_range:expr) => {
        concat_idents::concat_idents!(test_name = $name {
            #[test]
            fn test_name() {
                let op_str = stringify!($op);
                let op_ty_str = stringify!($op_ty);
                let res_ty_str = stringify!($res_ty);
                let main_fn = format!("(a: {op_ty_str}, b: {op_ty_str}) -> {res_ty_str} {{ a {op_str} b }}");
                let artifact_name = format!("{}_{}", stringify!($name), stringify!($op_ty).to_lowercase());
                let mut test = CompilerTest::rust_fn_body_with_stdlib_sys(artifact_name.clone(), &main_fn, false, None);
                // Test expected compilation artifacts
                test.expect_wasm(expect_file![format!("../../expected/{artifact_name}.wat")]);
                test.expect_ir(expect_file![format!("../../expected/{artifact_name}.hir")]);
                test.expect_masm(expect_file![format!("../../expected/{artifact_name}.masm")]);
                let package = test.compiled_package();

                // Run the Rust and compiled MASM code against a bunch of random inputs and compare the results
                let res = TestRunner::default()
                    .run(&($a_range, $b_range), move |(a, b)| {
                        dbg!(a, b);
                        let a_felt: Felt = a.0;
                        let b_felt: Felt = b.0;
                        let rs_out = a_felt $op b_felt;
                        dbg!(&rs_out);
                        let mut args = Vec::<midenc_hir::Felt>::default();
                        PushToStack::try_push(&b, &mut args);
                        PushToStack::try_push(&a, &mut args);
                        run_masm_vs_rust(rs_out, &package, &args, &test.session)
                    });
                match res {
                    Err(TestError::Fail(_, value)) => {
                        panic!("Found minimal(shrinked) failing case: {:?}", value);
                    },
                    Ok(_) => (),
                    _ => panic!("Unexpected test result: {:?}", res),
    }
            }
        });
    };
}

/// Compiles given binary operation
macro_rules! test_compile_comparison_op {
    ($name:ident, $op:tt) => {
        concat_idents::concat_idents!(test_name = $name {
            #[test]
            fn test_name() {
                let op_str = stringify!($op);
                let main_fn = format!("(a: Felt, b: Felt) -> bool {{ a {op_str} b }}");
                let artifact_name = format!("{}_felt", stringify!($name));
                let mut test = CompilerTest::rust_fn_body_with_stdlib_sys(artifact_name.clone(), &main_fn, false, None);
                // Test expected compilation artifacts
                test.expect_wasm(expect_file![format!("../../expected/{artifact_name}.wat")]);
                test.expect_ir(expect_file![format!("../../expected/{artifact_name}.hir")]);
                test.expect_masm(expect_file![format!("../../expected/{artifact_name}.masm")]);
            }
        });
    };
}

macro_rules! test_bin_op_total {
    ($name:ident, $op:tt) => {
        test_bin_op!($name, $op, Felt, Felt, any::<TestFelt>(), any::<TestFelt>());
    };
}

macro_rules! test_bool_op_total {
    ($name:ident, $op:tt) => {
        test_bin_op!($name, $op, Felt, bool, any::<TestFelt>(), any::<TestFelt>());
    };
}

test_bin_op_total!(add, +);
test_bin_op_total!(sub, -);
test_bin_op_total!(mul, *);
test_bin_op_total!(div, /);
test_bin_op_total!(neg, -);

test_bool_op_total!(eq, ==);

// TODO: Comparison operators are not defined for Felt, so we cannot compile a Rust equivalent for
// the semantic test
// see https://github.com/0xPolygonMiden/compiler/issues/175
// test_bool_op_total!(gt, >);
// test_bool_op_total!(lt, <);
// test_bool_op_total!(ge, >=);
// test_bool_op_total!(le, <=);

test_compile_comparison_op!(gt, >);
test_compile_comparison_op!(lt, <);
test_compile_comparison_op!(ge, >=);
test_compile_comparison_op!(le, <=);
