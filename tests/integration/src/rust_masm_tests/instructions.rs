use expect_test::expect_file;
use midenc_debug::PushToStack;
use proptest::{
    prelude::*,
    test_runner::{TestError, TestRunner},
};

use super::run_masm_vs_rust;
use crate::CompilerTest;

macro_rules! test_bin_op {
    ($name:ident, $op:tt, $op_ty:ty, $res_ty:ty, $a_range:expr, $b_range:expr) => {
        test_bin_op!($name, $op, $op_ty, $op_ty, $res_ty, $a_range, $b_range);
    };

    ($name:ident, $op:tt, $a_ty:ty, $b_ty:ty, $res_ty:tt, $a_range:expr, $b_range:expr) => {
        concat_idents::concat_idents!(test_name = $name, _, $a_ty {
            #[test]
            fn test_name() {
                let op_str = stringify!($op);
                let a_ty_str = stringify!($a_ty);
                let b_ty_str = stringify!($b_ty);
                let res_ty_str = stringify!($res_ty);
                let main_fn = format!("(a: {a_ty_str}, b: {b_ty_str}) -> {res_ty_str} {{ a {op_str} b }}");
                let mut test = CompilerTest::rust_fn_body(&main_fn, None);
                // Test expected compilation artifacts
                let artifact_name = format!("{}_{}", stringify!($name), stringify!($a_ty));
                test.expect_wasm(expect_file![format!("../../expected/{artifact_name}.wat")]);
                test.expect_ir(expect_file![format!("../../expected/{artifact_name}.hir")]);
                test.expect_masm(expect_file![format!("../../expected/{artifact_name}.masm")]);
                let package = test.compiled_package();

                // Run the Rust and compiled MASM code against a bunch of random inputs and compare the results
                let res = TestRunner::default()
                    .run(&($a_range, $b_range), move |(a, b)| {
                        dbg!(a, b);
                        let rs_out = a $op b;
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

macro_rules! test_unary_op {
    ($name:ident, $op:tt, $op_ty:tt, $range:expr) => {
        concat_idents::concat_idents!(test_name = $name, _, $op_ty {
            #[test]
            fn test_name() {
                let op_str = stringify!($op);
                let op_ty_str = stringify!($op_ty);
                let res_ty_str = stringify!($op_ty);
                let main_fn = format!("(a: {op_ty_str}) -> {res_ty_str} {{ {op_str}a }}");
                let mut test = CompilerTest::rust_fn_body(&main_fn, None);
                // Test expected compilation artifacts
                let artifact_name = format!("{}_{}", stringify!($name), stringify!($op_ty));
                test.expect_wasm(expect_file![format!("../../expected/{artifact_name}.wat")]);
                test.expect_ir(expect_file![format!("../../expected/{artifact_name}.hir")]);
                test.expect_masm(expect_file![format!("../../expected/{artifact_name}.masm")]);
                let package = test.compiled_package();

                // Run the Rust and compiled MASM code against a bunch of random inputs and compare the results
                let res = TestRunner::default()
                    .run(&($range), move |a| {
                        let rs_out = $op a;
                        dbg!(&rs_out);
                        let mut args = Vec::<midenc_hir::Felt>::default();
                        a.try_push(&mut args);
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

macro_rules! test_func_two_arg {
    ($name:ident, $func:path, $a_ty:tt, $b_ty:tt, $res_ty:tt) => {
        concat_idents::concat_idents!(test_name = $name, _, $a_ty, _, $b_ty {
            #[test]
            fn test_name() {
                let func_name_str = stringify!($func);
                let a_ty_str = stringify!($a_ty);
                let b_ty_str = stringify!($b_ty);
                let res_ty_str = stringify!($res_ty);
                let main_fn = format!("(a: {a_ty_str}, b: {b_ty_str}) -> {res_ty_str} {{ {func_name_str}(a, b) }}");
                let mut test = CompilerTest::rust_fn_body(&main_fn, None);
                // Test expected compilation artifacts
                let artifact_name = format!("{}_{}_{}", stringify!($func), stringify!($a_ty), stringify!($b_ty));
                test.expect_wasm(expect_file![format!("../../expected/{artifact_name}.wat")]);
                test.expect_ir(expect_file![format!("../../expected/{artifact_name}.hir")]);
                test.expect_masm(expect_file![format!("../../expected/{artifact_name}.masm")]);
                let package = test.compiled_package();

                // Run the Rust and compiled MASM code against a bunch of random inputs and compare the results
                let res = TestRunner::default()
                    .run(&(0..$a_ty::MAX/2, any::<$b_ty>()), move |(a, b)| {
                        let rust_out = $func(a, b);
                        dbg!(&rust_out);
                        let mut args = Vec::<midenc_hir::Felt>::default();
                        b.try_push(&mut args);
                        a.try_push(&mut args);
                        run_masm_vs_rust(rust_out, &package, &args, &test.session)
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

macro_rules! test_bool_op_total {
    ($name:ident, $op:tt, $op_ty:tt) => {
        test_bin_op!($name, $op, $op_ty, bool, any::<$op_ty>(), any::<$op_ty>());
    };
}

macro_rules! test_int_op {
    ($name:ident, $op:tt, $op_ty:ty, $a_range:expr, $b_range:expr) => {
        test_bin_op!($name, $op, $op_ty, $op_ty, $a_range, $b_range);
    };

    ($name:ident, $op:tt, $a_ty:ty, $b_ty:ty, $a_range:expr, $b_range:expr) => {
        test_bin_op!($name, $op, $a_ty, $b_ty, $a_ty, $a_range, $b_range);
    };
}

macro_rules! test_int_op_total {
    ($name:ident, $op:tt, $op_ty:tt) => {
        test_bin_op!($name, $op, $op_ty, $op_ty, any::<$op_ty>(), any::<$op_ty>());
    };
}

macro_rules! test_unary_op_total {
    ($name:ident, $op:tt, $op_ty:tt) => {
        test_unary_op!($name, $op, $op_ty, any::<$op_ty>());
    };
}

// Arithmetic ops
//
// NOTE: We're testing a limited range of inputs for now to sidestep overflow

test_int_op!(add, +, u64, 0..=u64::MAX/2, 0..=u64::MAX/2);
test_int_op!(add, +, i64, i64::MIN/2..=i64::MAX/2, -1..=i64::MAX/2);
test_int_op!(add, +, u32, 0..=u32::MAX/2, 0..=u32::MAX/2);
test_int_op!(add, +, u16, 0..=u16::MAX/2, 0..=u16::MAX/2);
test_int_op!(add, +, u8, 0..=u8::MAX/2, 0..=u8::MAX/2);
test_int_op!(add, +, i32, 0..=i32::MAX/2, 0..=i32::MAX/2);
test_int_op!(add, +, i16, 0..=i16::MAX/2, 0..=i16::MAX/2);
test_int_op!(add, +, i8, 0..=i8::MAX/2, 0..=i8::MAX/2);

test_int_op!(sub, -, u64, u64::MAX/2..=u64::MAX, 0..=u64::MAX/2);
test_int_op!(sub, -, i64, i64::MIN/2..=i64::MAX/2, -1..=i64::MAX/2);
test_int_op!(sub, -, u32, u32::MAX/2..=u32::MAX, 0..=u32::MAX/2);
test_int_op!(sub, -, u16, u16::MAX/2..=u16::MAX, 0..=u16::MAX/2);
test_int_op!(sub, -, u8, u8::MAX/2..=u8::MAX, 0..=u8::MAX/2);
test_int_op!(sub, -, i32, i32::MIN+1..=0, i32::MIN+1..=0);
test_int_op!(sub, -, i16, i16::MIN+1..=0, i16::MIN+1..=0);
test_int_op!(sub, -, i8, i8::MIN+1..=0, i8::MIN+1..=0);

test_int_op!(mul, *, u64, 0u64..=16656, 0u64..=16656);
test_int_op!(mul, *, i64, -65656i64..=65656, -65656i64..=65656);
test_int_op!(mul, *, u32, 0u32..=16656, 0u32..=16656);
test_int_op!(mul, *, u16, 0u16..=255, 0u16..=255);
test_int_op!(mul, *, u8, 0u8..=16, 0u8..=15);
test_int_op!(mul, *, i32, -16656i32..=16656, -16656i32..=16656);
//test_int_op!(mul, *, i16);
//test_int_op!(mul, *, i8);

// TODO: build with cargo to avoid core::panicking
// TODO: separate macro for div and rem tests to filter out division by zero
// test_int_op!(div, /, u32);
// ...
// add tests for div, rem,
//test_int_op!(div, /, u64, 0..=u64::MAX, 1..=u64::MAX);
//test_int_op!(div, /, i64, i64::MIN..=i64::MAX, 1..=i64::MAX);
//test_int_op!(rem, %, u64, 0..=u64::MAX, 1..=u64::MAX);
//test_int_op!(rem, %, i64, i64::MIN..=i64::MAX, 1..=i64::MAX);

test_unary_op!(neg, -, i64, (i64::MIN + 1)..=i64::MAX);

// Comparison ops

// enable when https://github.com/0xPolygonMiden/compiler/issues/56 is fixed
test_func_two_arg!(min, core::cmp::min, i32, i32, i32);
test_func_two_arg!(min, core::cmp::min, u32, u32, u32);
test_func_two_arg!(min, core::cmp::min, u8, u8, u8);
test_func_two_arg!(max, core::cmp::max, u8, u8, u8);

test_bool_op_total!(ge, >=, u64);
test_bool_op_total!(ge, >=, i64);
test_bool_op_total!(ge, >=, u32);
test_bool_op_total!(ge, >=, i32);
test_bool_op_total!(ge, >=, u16);
test_bool_op_total!(ge, >=, u8);
//test_bool_op_total!(ge, >=, i16);
//test_bool_op_total!(ge, >=, i8);

test_bool_op_total!(gt, >, u64);
test_bool_op_total!(gt, >, i64);
test_bool_op_total!(gt, >, u32);
test_bool_op_total!(gt, >, u16);
test_bool_op_total!(gt, >, i32);
test_bool_op_total!(gt, >, u8);
//test_bool_op_total!(gt, >, i16);
//test_bool_op_total!(gt, >, i8);

test_bool_op_total!(le, <=, u64);
test_bool_op_total!(le, <=, i64);
test_bool_op_total!(le, <=, u32);
test_bool_op_total!(le, <=, i32);
test_bool_op_total!(le, <=, u16);
test_bool_op_total!(le, <=, u8);
//test_bool_op_total!(le, <=, i16);
//test_bool_op_total!(le, <=, i8);

test_bool_op_total!(lt, <, u64);
test_bool_op_total!(lt, <, i64);
test_bool_op_total!(lt, <, u32);
test_bool_op_total!(lt, <, i32);
test_bool_op_total!(lt, <, u16);
test_bool_op_total!(lt, <, u8);
//test_bool_op_total!(lt, <, i16);
//test_bool_op_total!(lt, <, i8);

test_bool_op_total!(eq, ==, u64);
test_bool_op_total!(eq, ==, u32);
test_bool_op_total!(eq, ==, u16);
test_bool_op_total!(eq, ==, u8);
test_bool_op_total!(eq, ==, i64);
test_bool_op_total!(eq, ==, i32);
test_bool_op_total!(eq, ==, i16);
test_bool_op_total!(eq, ==, i8);

// Logical ops

test_bool_op_total!(and, &&, bool);
test_bool_op_total!(or, ||, bool);
test_bool_op_total!(xor, ^, bool);

// Bitwise ops

test_int_op_total!(band, &, u8);
test_int_op_total!(band, &, u16);
test_int_op_total!(band, &, u32);
test_int_op_total!(band, &, u64);
test_int_op_total!(band, &, i8);
test_int_op_total!(band, &, i16);
test_int_op_total!(band, &, i32);
test_int_op_total!(band, &, i64);

test_int_op_total!(bor, |, u8);
test_int_op_total!(bor, |, u16);
test_int_op_total!(bor, |, u32);
test_int_op_total!(bor, |, u64);
test_int_op_total!(bor, |, i8);
test_int_op_total!(bor, |, i16);
test_int_op_total!(bor, |, i32);
test_int_op_total!(bor, |, i64);

test_int_op_total!(bxor, ^, u8);
test_int_op_total!(bxor, ^, u16);
test_int_op_total!(bxor, ^, u32);
test_int_op_total!(bxor, ^, u64);
test_int_op_total!(bxor, ^, i8);
test_int_op_total!(bxor, ^, i16);
test_int_op_total!(bxor, ^, i32);
test_int_op_total!(bxor, ^, i64);

test_int_op!(shl, <<, u64, 0..=u64::MAX, 0u64..=63);
test_int_op!(shl, <<, u32, 0..u32::MAX, 0u32..32);
test_int_op!(shl, <<, u16, 0..u16::MAX, 0u16..16);
test_int_op!(shl, <<, u8, 0..u8::MAX, 0u8..8);
test_int_op!(shl, <<, i64, i64::MIN..=i64::MAX, 0u64..=63);
test_int_op!(shl, <<, i32, 0..i32::MAX, 0u32..32);
test_int_op!(shl, <<, i16, 0..i16::MAX, 0u16..16);
test_int_op!(shl, <<, i8, 0..i8::MAX, 0u8..8);

test_int_op!(shr, >>, i64, i64::MIN..=i64::MAX, 0u64..=63);
test_int_op!(shr, >>, u64, 0..=u64::MAX, 0u64..=63);
test_int_op!(shr, >>, u32, 0..u32::MAX, 0..32);
test_int_op!(shr, >>, u16, 0..u16::MAX, 0..16);
test_int_op!(shr, >>, u8, 0..u8::MAX, 0..8);
// # The following tests use small signed operands which we don't fully support yet
//test_int_op!(shr, >>, i8, i8::MIN..=i8::MAX, 0..=7);
//test_int_op!(shr, >>, i16, i16::MIN..=i16::MAX, 0..=15);
//test_int_op!(shr, >>, i32, i32::MIN..=i32::MAX, 0..=31);

test_unary_op!(neg, -, i32, (i32::MIN + 1)..=i32::MAX);
test_unary_op!(neg, -, i16, (i16::MIN + 1)..=i16::MAX);
test_unary_op!(neg, -, i8, (i8::MIN + 1)..=i8::MAX);

test_unary_op_total!(bnot, !, i64);
test_unary_op_total!(bnot, !, i32);
test_unary_op_total!(bnot, !, i16);
test_unary_op_total!(bnot, !, i8);
test_unary_op_total!(bnot, !, u64);
test_unary_op_total!(bnot, !, u32);
test_unary_op_total!(bnot, !, u16);
test_unary_op_total!(bnot, !, u8);
test_unary_op_total!(bnot, !, bool);
