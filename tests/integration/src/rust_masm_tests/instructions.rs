use std::{any::type_name, marker::PhantomData};

use miden_core::{Felt, Word};
use miden_debug::{FromMidenRepr, ToMidenRepr, push_wasm_ty_to_operand_stack};
use midenc_expect_test::expect_file;
use midenc_frontend_wasm::WasmTranslationConfig;
use midenc_hir::SmallVec;
use num_traits::{Bounded, One, PrimInt, ToBytes, Unsigned, Zero};
use proptest::{
    prelude::*,
    test_runner::{TestError, TestRunner},
};

use super::run_masm_vs_rust;
use crate::{
    CompilerTest,
    testing::{Initializer, eval_package, setup},
};

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
                let package = test.compile_package();

                // Run the Rust and compiled MASM code against a bunch of random inputs and compare the results
                let res = TestRunner::default()
                    .run(&($a_range, $b_range), move |(a, b)| {
                        let rs_out = a $op b;
                        let mut args = Vec::<midenc_hir::Felt>::default();
                        a.push_to_operand_stack(&mut args);
                        b.push_to_operand_stack(&mut args);
                        run_masm_vs_rust(rs_out, &package, &args, &test.session)
                    });
                match res {
                    Err(TestError::Fail(err, value)) => {
                        panic!(
                            "Found minimal(shrinked) failing case: {:?}\nFailure: {err:?}",
                            value
                        );
                    },
                    Ok(_) => (),
                    _ => panic!("Unexpected test result: {:?}", res),
                }
            }
        });
    };
}

macro_rules! test_wide_bin_op {
    ($name:ident, $op:tt, $op_ty:ty, $res_ty:ty, $a_range:expr, $b_range:expr) => {
        test_wide_bin_op!($name, $op, $op_ty, $op_ty, $res_ty, $a_range, $b_range);
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
                let package = test.compile_package();

                let res = TestRunner::default().run(&($a_range, $b_range), move |(a, b)| {
                    let rs_out = a $op b;

                    // Write the operation result to 20 * PAGE_SIZE.
                    let out_addr = 20u32 * 65536;

                    let mut args = Vec::<midenc_hir::Felt>::default();
                    out_addr.push_to_operand_stack(&mut args);
                    a.push_to_operand_stack(&mut args);
                    b.push_to_operand_stack(&mut args);

                    eval_package::<Felt, _, _>(&package, None, &args, &test.session, |trace| {
                        let vm_out_bytes: [u8; 16] =
                            trace.read_from_rust_memory(out_addr)
                                .expect("output was not written");

                        let rs_out_bytes = rs_out.to_le_bytes();

                        prop_assert_eq!(&rs_out_bytes, &vm_out_bytes, "VM output mismatch");
                        Ok(())
                    })?;

                    Ok(())
                });

                match res {
                    Err(TestError::Fail(err, value)) => {
                        panic!(
                            "Found minimal(shrinked) failing case: {:?}\nFailure: {err:?}",
                            value
                        );
                    }
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
                let package = test.compile_package();

                // Run the Rust and compiled MASM code against a bunch of random inputs and compare the results
                let res = TestRunner::default()
                    .run(&($range), move |a| {
                        let rs_out = $op a;
                        let mut args = Vec::<midenc_hir::Felt>::default();
                        a.push_to_operand_stack(&mut args);
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
                let package = test.compile_package();

                // Run the Rust and compiled MASM code against a bunch of random inputs and compare the results
                let res = TestRunner::default()
                    .run(&(0..$a_ty::MAX/2, any::<$b_ty>()), move |(a, b)| {
                        let rust_out = $func(a, b);
                        let mut args = Vec::<midenc_hir::Felt>::default();
                        a.push_to_operand_stack(&mut args);
                        b.push_to_operand_stack(&mut args);
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

// Useful for debugging traces:
// - WK1234 is (1000 << 96) | (2000 << 64) | (3000 << 32) | 4000;
// - WC1234 is (100 << 96) | (200 << 64) | (300 << 32) | 400;
//
// const WK1234: i128 = 79228162551157825753847955460000;
// const WC1234: i128 = 7922816255115782575384795546000;
//
// const WK1234H: i128 = 0x00001000_00002000_00003000_00004000;
// const WC1234H: i128 = 0x00000100_00000200_00000300_00000400;
//
// test_wide_bin_op!(xxx, x, i128, i128, WK1234..=WK1234, WC1234..=WC1234);

test_wide_bin_op!(add, +, u128, u128, 0..=u128::MAX/2, 0..=u128::MAX/2);
test_wide_bin_op!(add, +, i128, i128, i128::MIN/2..=i128::MAX/2, -1..=i128::MAX/2);

test_int_op!(sub, -, u64, u64::MAX/2..=u64::MAX, 0..=u64::MAX/2);
test_int_op!(sub, -, i64, i64::MIN/2..=i64::MAX/2, -1..=i64::MAX/2);
test_int_op!(sub, -, u32, u32::MAX/2..=u32::MAX, 0..=u32::MAX/2);
test_int_op!(sub, -, u16, u16::MAX/2..=u16::MAX, 0..=u16::MAX/2);
test_int_op!(sub, -, u8, u8::MAX/2..=u8::MAX, 0..=u8::MAX/2);
test_int_op!(sub, -, i32, i32::MIN+1..=0, i32::MIN+1..=0);
test_int_op!(sub, -, i16, i16::MIN+1..=0, i16::MIN+1..=0);
test_int_op!(sub, -, i8, i8::MIN+1..=0, i8::MIN+1..=0);

test_wide_bin_op!(sub, -, u128, u128, u128::MAX/2..=u128::MAX, 0..=u128::MAX/2);
test_wide_bin_op!(sub, -, i128, i128, i128::MIN/2..=i128::MAX/2, -1..=i128::MAX/2);

test_int_op!(mul, *, u64, 0u64..=16656, 0u64..=16656);
test_int_op!(mul, *, i64, -65656i64..=65656, -65656i64..=65656);
test_int_op!(mul, *, u32, 0u32..=16656, 0u32..=16656);
test_int_op!(mul, *, u16, 0u16..=255, 0u16..=255);
test_int_op!(mul, *, u8, 0u8..=16, 0u8..=15);
test_int_op!(mul, *, i32, -16656i32..=16656, -16656i32..=16656);
//test_int_op!(mul, *, i16);
//test_int_op!(mul, *, i8);

const MAX_U128_64: u128 = u64::MAX as u128;
const MAX_I128_64: i128 = i64::MAX as i128;
const MIN_I128_64: i128 = i64::MIN as i128;

test_wide_bin_op!(mul, *, u128, u128, 0..=MAX_U128_64, 0..=MAX_U128_64);
test_wide_bin_op!(mul, *, i128, i128, MIN_I128_64..MAX_I128_64, MIN_I128_64..=MAX_I128_64);

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

// enable when https://github.com/0xMiden/compiler/issues/56 is fixed
test_func_two_arg!(min, core::cmp::min, i32, i32, i32);
test_func_two_arg!(min, core::cmp::min, u32, u32, u32);
test_func_two_arg!(min, core::cmp::min, u8, u8, u8);
test_func_two_arg!(max, core::cmp::max, u8, u8, u8);

#[test]
fn test_overflowing_add_u8() {
    test_overflowing_arith(u8::overflowing_add, "overflowing_add", NumericStrategy::full_range());
}

#[test]
fn test_overflowing_add_u16() {
    test_overflowing_arith(u16::overflowing_add, "overflowing_add", NumericStrategy::full_range());
}

#[test]
fn test_overflowing_add_u32() {
    test_overflowing_arith(u32::overflowing_add, "overflowing_add", NumericStrategy::full_range());
}

#[test]
fn test_overflowing_add_u64() {
    test_overflowing_arith(u64::overflowing_add, "overflowing_add", NumericStrategy::full_range());
}

// TODO 960 should be resolved
#[test]
#[ignore = "https://github.com/0xMiden/compiler/issues/960"]
fn test_overflowing_add_i8() {
    test_overflowing_arith(i8::overflowing_add, "overflowing_add", NumericStrategy::full_range());
}

#[test]
#[ignore = "https://github.com/0xMiden/compiler/issues/960"]
fn test_overflowing_add_i16() {
    test_overflowing_arith(i16::overflowing_add, "overflowing_add", NumericStrategy::full_range());
}

#[test]
fn test_overflowing_add_i32() {
    test_overflowing_arith(i32::overflowing_add, "overflowing_add", NumericStrategy::full_range());
}

#[test]
fn test_overflowing_add_i64() {
    test_overflowing_arith(i64::overflowing_add, "overflowing_add", NumericStrategy::full_range());
}

#[test]
fn test_overflowing_sub_u8() {
    test_overflowing_arith(u8::overflowing_sub, "overflowing_sub", NumericStrategy::full_range());
}

#[test]
fn test_overflowing_sub_u16() {
    test_overflowing_arith(u16::overflowing_sub, "overflowing_sub", NumericStrategy::full_range());
}

#[test]
fn test_overflowing_sub_u32() {
    test_overflowing_arith(u32::overflowing_sub, "overflowing_sub", NumericStrategy::full_range());
}

#[test]
fn test_overflowing_sub_u64() {
    test_overflowing_arith(u64::overflowing_sub, "overflowing_sub", NumericStrategy::full_range());
}

#[test]
#[ignore = "https://github.com/0xMiden/compiler/issues/960"]
fn test_overflowing_sub_i8() {
    test_overflowing_arith(i8::overflowing_sub, "overflowing_sub", NumericStrategy::full_range());
}

#[test]
#[ignore = "https://github.com/0xMiden/compiler/issues/960"]
fn test_overflowing_sub_i16() {
    test_overflowing_arith(i16::overflowing_sub, "overflowing_sub", NumericStrategy::full_range());
}

#[test]
fn test_overflowing_sub_i32() {
    test_overflowing_arith(i32::overflowing_sub, "overflowing_sub", NumericStrategy::full_range());
}

#[test]
fn test_overflowing_sub_i64() {
    test_overflowing_arith(i64::overflowing_sub, "overflowing_sub", NumericStrategy::full_range());
}

#[test]
fn test_overflowing_mul_u8() {
    test_overflowing_arith(u8::overflowing_mul, "overflowing_mul", NumericStrategy::full_range());
}

#[test]
fn test_overflowing_mul_u16() {
    test_overflowing_arith(u16::overflowing_mul, "overflowing_mul", NumericStrategy::full_range());
}

#[test]
fn test_overflowing_mul_u32() {
    test_overflowing_arith(u32::overflowing_mul, "overflowing_mul", NumericStrategy::full_range());
}

#[test]
fn test_overflowing_mul_u64() {
    test_overflowing_arith(u64::overflowing_mul, "overflowing_mul", NumericStrategy::full_range());
}

#[test]
#[ignore = "https://github.com/0xMiden/compiler/issues/960"]
fn test_overflowing_mul_i8() {
    test_overflowing_arith(i8::overflowing_mul, "overflowing_mul", NumericStrategy::full_range());
}

#[test]
#[ignore = "https://github.com/0xMiden/compiler/issues/960"]
fn test_overflowing_mul_i16() {
    test_overflowing_arith(i16::overflowing_mul, "overflowing_mul", NumericStrategy::full_range());
}

#[test]
fn test_overflowing_mul_i32() {
    test_overflowing_arith(i32::overflowing_mul, "overflowing_mul", NumericStrategy::full_range());
}

#[test]
fn test_overflowing_mul_i64() {
    test_overflowing_arith(i64::overflowing_mul, "overflowing_mul", NumericStrategy::full_range());
}

#[test]
fn test_overflowing_div_u8() {
    test_overflowing_arith(
        u8::overflowing_div,
        "overflowing_div",
        NumericStrategy::non_zero_rhs_unsigned(),
    );
}

#[test]
fn test_overflowing_div_u16() {
    test_overflowing_arith(
        u16::overflowing_div,
        "overflowing_div",
        NumericStrategy::non_zero_rhs_unsigned(),
    );
}

#[test]
fn test_overflowing_div_u32() {
    test_overflowing_arith(
        u32::overflowing_div,
        "overflowing_div",
        NumericStrategy::non_zero_rhs_unsigned(),
    );
}

#[test]
fn test_overflowing_div_u64() {
    test_overflowing_arith(
        u64::overflowing_div,
        "overflowing_div",
        NumericStrategy::non_zero_rhs_unsigned(),
    );
}

#[test]
#[ignore = "https://github.com/0xMiden/compiler/issues/966"]
fn test_overflowing_div_i8() {
    test_overflowing_arith(
        i8::overflowing_div,
        "overflowing_div",
        NumericStrategy::non_zero_rhs_signed(),
    );
}

#[test]
#[ignore = "https://github.com/0xMiden/compiler/issues/966"]
fn test_overflowing_div_i16() {
    test_overflowing_arith(
        i16::overflowing_div,
        "overflowing_div",
        NumericStrategy::non_zero_rhs_signed(),
    );
}

#[test]
fn test_overflowing_div_i32() {
    test_overflowing_arith(
        i32::overflowing_div,
        "overflowing_div",
        NumericStrategy::non_zero_rhs_signed(),
    );
}

#[test]
fn test_overflowing_div_i64() {
    test_overflowing_arith(
        i64::overflowing_div,
        "overflowing_div",
        NumericStrategy::non_zero_rhs_signed(),
    );
}

#[test]
fn test_overflowing_rem_u8() {
    test_overflowing_arith(
        u8::overflowing_rem,
        "overflowing_rem",
        NumericStrategy::non_zero_rhs_unsigned(),
    );
}

#[test]
fn test_overflowing_rem_u16() {
    test_overflowing_arith(
        u16::overflowing_rem,
        "overflowing_rem",
        NumericStrategy::non_zero_rhs_unsigned(),
    );
}

#[test]
fn test_overflowing_rem_u32() {
    test_overflowing_arith(
        u32::overflowing_rem,
        "overflowing_rem",
        NumericStrategy::non_zero_rhs_unsigned(),
    );
}

#[test]
fn test_overflowing_rem_u64() {
    test_overflowing_arith(
        u64::overflowing_rem,
        "overflowing_rem",
        NumericStrategy::non_zero_rhs_unsigned(),
    );
}

#[test]
#[ignore = "Mod is not supported for signed int"]
fn test_overflowing_rem_i8() {
    test_overflowing_arith(
        i8::overflowing_rem,
        "overflowing_rem",
        NumericStrategy::non_zero_rhs_signed(),
    );
}

#[test]
#[ignore = "Mod is not supported for signed int"]
fn test_overflowing_rem_i16() {
    test_overflowing_arith(
        i16::overflowing_rem,
        "overflowing_rem",
        NumericStrategy::non_zero_rhs_signed(),
    );
}

#[test]
#[ignore = "Mod is not supported for signed int"]
fn test_overflowing_rem_i32() {
    test_overflowing_arith(
        i32::overflowing_rem,
        "overflowing_rem",
        NumericStrategy::non_zero_rhs_signed(),
    );
}

#[test]
#[ignore = "https://github.com/0xMiden/compiler/issues/1000"]
fn test_overflowing_rem_i64() {
    test_overflowing_arith(
        i64::overflowing_rem,
        "overflowing_rem",
        NumericStrategy::non_zero_rhs_signed(),
    );
}

// TODO handle overflowing ops for wide types

#[test]
fn test_checked_add_u8() {
    test_checked_arith(u8::checked_add, "checked_add", NumericStrategy::full_range());
}

#[test]
fn test_checked_add_u16() {
    test_checked_arith(u16::checked_add, "checked_add", NumericStrategy::full_range());
}

#[test]
fn test_checked_add_u32() {
    test_checked_arith(u32::checked_add, "checked_add", NumericStrategy::full_range());
}

#[test]
fn test_checked_add_u64() {
    test_checked_arith(u64::checked_add, "checked_add", NumericStrategy::full_range());
}

#[test]
#[ignore = "https://github.com/0xMiden/compiler/issues/960"]
fn test_checked_add_i8() {
    test_checked_arith(i8::checked_add, "checked_add", NumericStrategy::full_range());
}

#[test]
#[ignore = "https://github.com/0xMiden/compiler/issues/960"]
fn test_checked_add_i16() {
    test_checked_arith(i16::checked_add, "checked_add", NumericStrategy::full_range());
}

#[test]
fn test_checked_add_i32() {
    test_checked_arith(i32::checked_add, "checked_add", NumericStrategy::full_range());
}

#[test]
fn test_checked_add_i64() {
    test_checked_arith(i64::checked_add, "checked_add", NumericStrategy::full_range());
}

#[test]
fn test_checked_sub_u8() {
    test_checked_arith(u8::checked_sub, "checked_sub", NumericStrategy::full_range());
}

#[test]
fn test_checked_sub_u16() {
    test_checked_arith(u16::checked_sub, "checked_sub", NumericStrategy::full_range());
}

#[test]
fn test_checked_sub_u32() {
    test_checked_arith(u32::checked_sub, "checked_sub", NumericStrategy::full_range());
}

#[test]
fn test_checked_sub_u64() {
    test_checked_arith(u64::checked_sub, "checked_sub", NumericStrategy::full_range());
}

#[test]
#[ignore = "https://github.com/0xMiden/compiler/issues/960"]
fn test_checked_sub_i8() {
    test_checked_arith(i8::checked_sub, "checked_sub", NumericStrategy::full_range());
}

#[test]
#[ignore = "https://github.com/0xMiden/compiler/issues/960"]
fn test_checked_sub_i16() {
    test_checked_arith(i16::checked_sub, "checked_sub", NumericStrategy::full_range());
}

#[test]
fn test_checked_sub_i32() {
    test_checked_arith(i32::checked_sub, "checked_sub", NumericStrategy::full_range());
}

#[test]
fn test_checked_sub_i64() {
    test_checked_arith(i64::checked_sub, "checked_sub", NumericStrategy::full_range());
}

#[test]
fn test_checked_mul_u8() {
    test_checked_arith(u8::checked_mul, "checked_mul", NumericStrategy::full_range());
}

#[test]
fn test_checked_mul_u16() {
    test_checked_arith(u16::checked_mul, "checked_mul", NumericStrategy::full_range());
}

#[test]
fn test_checked_mul_u32() {
    test_checked_arith(u32::checked_mul, "checked_mul", NumericStrategy::full_range());
}

#[test]
fn test_checked_mul_u64() {
    test_checked_arith(u64::checked_mul, "checked_mul", NumericStrategy::full_range());
}

#[test]
#[ignore = "https://github.com/0xMiden/compiler/issues/960"]
fn test_checked_mul_i8() {
    test_checked_arith(i8::checked_mul, "checked_mul", NumericStrategy::full_range());
}

#[test]
fn test_checked_mul_i16() {
    test_checked_arith(i16::checked_mul, "checked_mul", NumericStrategy::full_range());
}

#[test]
fn test_checked_mul_i32() {
    test_checked_arith(i32::checked_mul, "checked_mul", NumericStrategy::full_range());
}

#[test]
fn test_checked_mul_i64() {
    test_checked_arith(i64::checked_mul, "checked_mul", NumericStrategy::full_range());
}

// When dividing by zero, `checked_div` returns `None` and doesn't panic. Therefore the full
// range strategy can be used.

#[test]
fn test_checked_div_u8() {
    test_checked_arith(u8::checked_div, "checked_div", NumericStrategy::full_range());
}

#[test]
fn test_checked_div_u16() {
    test_checked_arith(u16::checked_div, "checked_div", NumericStrategy::full_range());
}

#[test]
fn test_checked_div_u32() {
    test_checked_arith(u32::checked_div, "checked_div", NumericStrategy::full_range());
}

#[test]
fn test_checked_div_u64() {
    test_checked_arith(u64::checked_div, "checked_div", NumericStrategy::full_range());
}

#[test]
#[ignore = "https://github.com/0xMiden/compiler/issues/966"]
fn test_checked_div_i8() {
    test_checked_arith(i8::checked_div, "checked_div", NumericStrategy::full_range());
}

#[test]
#[ignore = "https://github.com/0xMiden/compiler/issues/966"]
fn test_checked_div_i16() {
    test_checked_arith(i16::checked_div, "checked_div", NumericStrategy::full_range());
}

#[test]
fn test_checked_div_i32() {
    test_checked_arith(i32::checked_div, "checked_div", NumericStrategy::full_range());
}

#[test]
fn test_checked_div_i64() {
    test_checked_arith(i64::checked_div, "checked_div", NumericStrategy::full_range());
}

#[test]
fn test_checked_rem_u8() {
    test_checked_arith(u8::checked_rem, "checked_rem", NumericStrategy::full_range());
}

#[test]
fn test_checked_rem_u16() {
    test_checked_arith(u16::checked_rem, "checked_rem", NumericStrategy::full_range());
}

#[test]
fn test_checked_rem_u32() {
    test_checked_arith(u32::checked_rem, "checked_rem", NumericStrategy::full_range());
}

#[test]
fn test_checked_rem_u64() {
    test_checked_arith(u64::checked_rem, "checked_rem", NumericStrategy::full_range());
}

#[test]
#[ignore = "Mod is not supported for signed int"]
fn test_checked_rem_i8() {
    test_checked_arith(i8::checked_rem, "checked_rem", NumericStrategy::full_range());
}

#[test]
#[ignore = "Mod is not supported for signed int"]
fn test_checked_rem_i16() {
    test_checked_arith(i16::checked_rem, "checked_rem", NumericStrategy::full_range());
}

#[test]
#[ignore = "Mod is not supported for signed int"]
fn test_checked_rem_i32() {
    test_checked_arith(i32::checked_rem, "checked_rem", NumericStrategy::full_range());
}

#[test]
#[ignore = "https://github.com/0xMiden/compiler/issues/1000"]
fn test_checked_rem_i64() {
    test_checked_arith(i64::checked_rem, "checked_rem", NumericStrategy::full_range());
}

struct NumericStrategy<T> {
    _marker: PhantomData<T>,
}

impl<T> NumericStrategy<T>
where
    T: PrimInt + Arbitrary,
    std::ops::RangeInclusive<T>: Strategy<Value = T>,
{
    fn full_range() -> impl Strategy<Value = (T, T)> {
        (any::<T>(), any::<T>())
    }

    /// Returns a strategy with unrestricted lhs and non-zero rhs.
    fn non_zero_rhs_unsigned() -> impl Strategy<Value = (T, T)>
    where
        T: Unsigned,
    {
        (any::<T>(), T::one()..=T::max_value())
    }

    /// Returns a strategy with unrestricted lhs and non-zero rhs.
    fn non_zero_rhs_signed() -> impl Strategy<Value = (T, T)>
    where
        T: num_traits::Signed,
    {
        (any::<T>(), prop_oneof![T::min_value()..=-T::one(), T::one()..=T::max_value(),])
    }
}

fn test_overflowing_arith<T>(
    op: fn(T, T) -> (T, bool),
    fn_name: &str,
    strategy: impl Strategy<Value = (T, T)>,
) where
    T: ToBytes + ToMidenRepr + FromMidenRepr + PrimInt + Arbitrary,
{
    // The return value of `type_name` isn't stable, but it's good enough for this test.
    let ty_name = type_name::<T>();
    let main_fn = format!(
        r#"(a: {ty_name}, b: {ty_name}) -> ({ty_name}, bool) {{
        a.{fn_name}(b)
    }}"#
    );
    let config = WasmTranslationConfig::default();
    let artifact_name = format!("test_{fn_name}_{ty_name}");
    let mut test =
        CompilerTest::rust_fn_body_with_stdlib_sys(artifact_name.clone(), &main_fn, config, None);
    let package = test.compile_package();

    let res = TestRunner::default().run(&strategy, move |(a, b)| {
        let rust_out = op(a, b);

        // Write the operation result to 20 * PAGE_SIZE.
        let out_addr = 20u32 * 65536;

        let mut args = Vec::<midenc_hir::Felt>::default();
        out_addr.push_to_operand_stack(&mut args);
        a.push_to_operand_stack(&mut args);
        b.push_to_operand_stack(&mut args);

        eval_package::<Felt, _, _>(&package, None, &args, &test.session, |trace| {
            let ty_byte_size = std::mem::size_of::<T>();
            assert!(ty_byte_size <= 8, "cannot handle types larger than 8 bytes");
            // At most 9 bytes are written to memory: ty_byte_size <= 8 and 1 byte for the bool.
            let x: [u8; 9] = trace.read_from_rust_memory(out_addr).expect("output was not written");
            let vm_out_bytes = x[..ty_byte_size + 1].to_vec(); // only take what's actually written

            let rs_out_bytes =
                [rust_out.0.to_le_bytes().as_ref(), &[u8::from(rust_out.1)]].concat();

            prop_assert_eq!(&rs_out_bytes, &vm_out_bytes, "VM output mismatch");
            Ok(())
        })?;
        Ok(())
    });
    match res {
        Err(TestError::Fail(reason, value)) => {
            panic!("Found minimal(shrinked) failing case: {value:?}\nFailure: {reason:?}");
        }
        Ok(_) => (),
        _ => panic!("Unexpected test result: {:?}", res),
    }
}

fn test_checked_arith<T>(
    op: fn(T, T) -> Option<T>,
    fn_name: &str,
    strategy: impl Strategy<Value = (T, T)>,
) where
    T: ToBytes + ToMidenRepr + FromMidenRepr + PrimInt + Arbitrary,
{
    // The return value of `type_name` isn't stable, but it's good enough for this test.
    let ty_name = type_name::<T>();
    let main_fn = format!(
        r#"(a: {ty_name}, b: {ty_name}) -> ({ty_name}, bool) {{
        // Convert `Option<T>` to (T, bool) bool as `Option` is not yet supported (#111)
        match a.{fn_name}(b) {{
            Some(value) => (value, true),
            None => (0 as {ty_name}, false),
        }}
    }}"#
    );
    let config = WasmTranslationConfig::default();
    let artifact_name = format!("test_{fn_name}_{ty_name}");
    let mut test =
        CompilerTest::rust_fn_body_with_stdlib_sys(artifact_name.clone(), &main_fn, config, None);
    let package = test.compile_package();

    let res = TestRunner::default().run(&strategy, move |(a, b)| {
        let rust_out = match op(a, b) {
            Some(value) => (value, true),
            None => (T::zero(), false),
        };

        // Write the operation result to 20 * PAGE_SIZE.
        let out_addr = 20u32 * 65536;

        let mut args = Vec::<midenc_hir::Felt>::default();
        out_addr.push_to_operand_stack(&mut args);
        a.push_to_operand_stack(&mut args);
        b.push_to_operand_stack(&mut args);

        eval_package::<Felt, _, _>(&package, None, &args, &test.session, |trace| {
            let ty_byte_size = std::mem::size_of::<T>();
            assert!(ty_byte_size <= 8, "cannot handle types larger than 8 bytes");
            // At most 9 bytes are written to memory: ty_byte_size <= 8 and 1 byte for the bool.
            let x: [u8; 9] = trace.read_from_rust_memory(out_addr).expect("output was not written");
            let vm_out_bytes = x[..ty_byte_size + 1].to_vec(); // only take what's actually written

            let rs_out_bytes =
                [rust_out.0.to_le_bytes().as_ref(), &[u8::from(rust_out.1)]].concat();

            prop_assert_eq!(&rs_out_bytes, &vm_out_bytes, "VM output mismatch");
            Ok(())
        })?;
        Ok(())
    });
    match res {
        Err(TestError::Fail(reason, value)) => {
            panic!("Found minimal(shrinked) failing case: {value:?}\nFailure: {reason:?}");
        }
        Ok(_) => (),
        _ => panic!("Unexpected test result: {:?}", res),
    }
}

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
test_int_op!(shr, >>, u32, 0..u32::MAX, 0u32..32);
test_int_op!(shr, >>, u16, 0..u16::MAX, 0u32..16);
test_int_op!(shr, >>, u8, 0..u8::MAX, 0u32..8);
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

#[test]
fn test_hmerge() {
    let main_fn = r#"
	        (f0: miden_stdlib_sys::Felt, f1: miden_stdlib_sys::Felt, f2: miden_stdlib_sys::Felt, f3: miden_stdlib_sys::Felt, f4: miden_stdlib_sys::Felt, f5: miden_stdlib_sys::Felt, f6: miden_stdlib_sys::Felt, f7: miden_stdlib_sys::Felt) -> miden_stdlib_sys::Felt {
	            let digest1 = miden_stdlib_sys::Digest::new([f0, f1, f2, f3]);
	            let digest2 = miden_stdlib_sys::Digest::new([f4, f5, f6, f7]);
	            let digests = [digest1, digest2];
	            let res = miden_stdlib_sys::intrinsics::crypto::merge(digests);
	            res.inner[0]
	        }"#
	        .to_string();
    let config = WasmTranslationConfig::default();
    let mut test = CompilerTest::rust_fn_body_with_stdlib_sys("hmerge", &main_fn, config, []);

    let package = test.compile_package();

    // Run the Rust and compiled MASM code against a bunch of random inputs and compare the results
    let config = proptest::test_runner::Config::with_cases(16);
    let res = TestRunner::new(config).run(
        &any::<([miden_debug::Felt; 4], [miden_debug::Felt; 4])>(),
        move |(felts_in1, felts_in2)| {
            let raw_felts_in1: [Felt; 4] = [
                felts_in1[0].into(),
                felts_in1[1].into(),
                felts_in1[2].into(),
                felts_in1[3].into(),
            ];

            let raw_felts_in2: [Felt; 4] = [
                felts_in2[0].into(),
                felts_in2[1].into(),
                felts_in2[2].into(),
                felts_in2[3].into(),
            ];
            let digests_in =
                [miden_core::Word::from(raw_felts_in1), miden_core::Word::from(raw_felts_in2)];
            let digest_out = miden_core::crypto::hash::Poseidon2::merge(&digests_in);

            let felts_out: [miden_debug::Felt; 4] = [
                miden_debug::Felt(digest_out[0]),
                miden_debug::Felt(digest_out[1]),
                miden_debug::Felt(digest_out[2]),
                miden_debug::Felt(digest_out[3]),
            ];

            let args = [
                raw_felts_in1[0],
                raw_felts_in1[1],
                raw_felts_in1[2],
                raw_felts_in1[3],
                raw_felts_in2[0],
                raw_felts_in2[1],
                raw_felts_in2[2],
                raw_felts_in2[3],
            ];
            eval_package::<Felt, _, _>(&package, [], &args, &test.session, |trace| {
                let res: Felt = trace.parse_result().unwrap();
                prop_assert_eq!(res, digest_out[0]);
                Ok(())
            })?;

            Ok(())
        },
    );

    match res {
        Err(TestError::Fail(_, value)) => {
            panic!("Found minimal(shrinked) failing case: {value:?}");
        }
        Ok(_) => (),
        _ => panic!("Unexpected test result: {res:?}"),
    }
}

#[test]
fn test_memory_copy_aligned_bytes() {
    let main_fn = r#"() -> Felt {
        #[inline(never)]
        fn do_copy(dst: &mut [u32; 12], src: &[u32; 16]) {
            unsafe {
                let src_ptr = (src.as_ptr() as *const u8).add(4);
                let dst_ptr = dst.as_mut_ptr() as *mut u8;
                core::ptr::copy_nonoverlapping(src_ptr, dst_ptr, 48);
            }
        }

        let mut src = [0u32; 16];
        let src_bytes = src.as_mut_ptr() as *mut u8;
        let mut i = 0usize;
        while i < 64 {
            unsafe { *src_bytes.add(i) = i as u8; }
            i += 1;
        }

        let mut dst = [0u32; 12];
        do_copy(&mut dst, &src);

        let dst_bytes = dst.as_ptr() as *const u8;
        let mut mismatches = 0u32;
        let mut i = 0usize;
        while i < 48 {
            let observed = unsafe { *dst_bytes.add(i) };
            if observed != (i as u8).wrapping_add(4) {
                mismatches += 1;
            }
            i += 1;
        }

        Felt::from_u32(mismatches)
    }"#;

    setup::enable_compiler_instrumentation();
    let config = WasmTranslationConfig::default();
    let mut test = CompilerTest::rust_fn_body_with_stdlib_sys(
        "memory_copy_aligned_bytes_u8s",
        main_fn,
        config,
        [],
    );

    let package = test.compile_package();
    let args: [Felt; 0] = [];

    eval_package::<Felt, _, _>(&package, [], &args, &test.session, |trace| {
        let res: Felt = trace.parse_result().unwrap();
        assert_eq!(res, Felt::ZERO);
        Ok(())
    })
    .unwrap();
}

#[test]
fn test_memory_copy_u128_fast_path() {
    let main_fn = r#"() -> Felt {
        #[inline(never)]
        fn do_copy(dst: &mut [u128; 2], src: &[u128; 3]) {
            unsafe {
                let src_ptr = src.as_ptr().add(1);
                let dst_ptr = dst.as_mut_ptr();
                core::ptr::copy_nonoverlapping(src_ptr, dst_ptr, 2);
            }
        }

        let src = [
            0x00112233445566778899aabbccddeeff_u128,
            0x102132435465768798a9bacbdcedfe0f_u128,
            0xfedcba98765432100123456789abcdef_u128,
        ];
        let mut dst = [0u128; 2];
        do_copy(&mut dst, &src);

        let expected = [src[1], src[2]];
        let mut mismatches = 0u32;
        let mut i = 0usize;
        while i < 2 {
            if dst[i] != expected[i] {
                mismatches += 1;
            }
            i += 1;
        }

        Felt::from_u32(mismatches)
    }"#;

    setup::enable_compiler_instrumentation();
    let config = WasmTranslationConfig::default();
    let mut test = CompilerTest::rust_fn_body_with_stdlib_sys(
        "memory_copy_u128_fast_path",
        main_fn,
        config,
        [],
    );

    let package = test.compile_package();
    let args: [Felt; 0] = [];

    eval_package::<Felt, _, _>(&package, [], &args, &test.session, |trace| {
        let res: Felt = trace.parse_result().unwrap();
        assert_eq!(res, Felt::ZERO);
        Ok(())
    })
    .unwrap();
}

#[test]
fn test_memory_copy_multiword_fast_path() {
    let main_fn = r#"() -> Felt {
        struct Chunk([u128; 2]);

        #[inline(never)]
        fn do_copy(dst: &mut [Chunk; 1], src: &[Chunk; 2]) {
            unsafe {
                let src_ptr = src.as_ptr().add(1);
                let dst_ptr = dst.as_mut_ptr();
                core::ptr::copy_nonoverlapping(src_ptr, dst_ptr, 1);
            }
        }

        let src = [
            Chunk([
                0x00112233445566778899aabbccddeeff_u128,
                0x112233445566778899aabbccddeeff00_u128,
            ]),
            Chunk([
                0xaabbccddeeff00112233445566778899_u128,
                0xffeeddccbbaa99887766554433221100_u128,
            ]),
        ];
        let mut dst = [Chunk([0u128; 2])];
        do_copy(&mut dst, &src);

        let expected = &src[1].0;
        let observed = &dst[0].0;
        let mut mismatches = 0u32;
        let mut i = 0usize;
        while i < 2 {
            if observed[i] != expected[i] {
                mismatches += 1;
            }
            i += 1;
        }

        Felt::from_u32(mismatches)
    }"#;

    setup::enable_compiler_instrumentation();
    let config = WasmTranslationConfig::default();
    let mut test = CompilerTest::rust_fn_body_with_stdlib_sys(
        "memory_copy_multiword_fast_path",
        main_fn,
        config,
        [],
    );

    let package = test.compile_package();
    let args: [Felt; 0] = [];

    eval_package::<Felt, _, _>(&package, [], &args, &test.session, |trace| {
        let res: Felt = trace.parse_result().unwrap();
        assert_eq!(res, Felt::ZERO);
        Ok(())
    })
    .unwrap();
}

#[test]
fn test_memory_copy_aligned_addresses_misaligned_count() {
    let main_fn = r#"() -> Felt {
        #[inline(never)]
        fn do_copy(dst: &mut [u32; 12], src: &[u32; 16]) {
            unsafe {
                let src_ptr = (src.as_ptr() as *const u8).add(4);
                let dst_ptr = dst.as_mut_ptr() as *mut u8;
                core::ptr::copy_nonoverlapping(src_ptr, dst_ptr, 47);
            }
        }

        let mut src = [0u32; 16];
        let src_bytes = src.as_mut_ptr() as *mut u8;
        let mut i = 0usize;
        while i < 64 {
            unsafe { *src_bytes.add(i) = i as u8; }
            i += 1;
        }

        let mut dst = [0xffff_ffffu32; 12];
        do_copy(&mut dst, &src);

        let dst_bytes = dst.as_ptr() as *const u8;
        let mut mismatches = 0u32;
        let mut i = 0usize;
        while i < 48 {
            let observed = unsafe { *dst_bytes.add(i) };
            let expected = if i < 47 {
                (i as u8).wrapping_add(4)
            } else {
                0xff
            };
            if observed != expected {
                mismatches += 1;
            }
            i += 1;
        }

        Felt::from_u32(mismatches)
    }"#;

    setup::enable_compiler_instrumentation();
    let config = WasmTranslationConfig::default();
    let mut test = CompilerTest::rust_fn_body_with_stdlib_sys(
        "memory_copy_aligned_addresses_misaligned_count_u8s",
        main_fn,
        config,
        [],
    );

    let package = test.compile_package();
    let args: [Felt; 0] = [];

    eval_package::<Felt, _, _>(&package, [], &args, &test.session, |trace| {
        let res: Felt = trace.parse_result().unwrap();
        assert_eq!(res, Felt::ZERO);
        Ok(())
    })
    .unwrap();
}

#[test]
fn test_memory_copy_unaligned() {
    let main_fn = r#"() -> Felt {
        #[inline(never)]
        fn do_copy(dst: &mut [u8; 48], src: &[u8; 64]) {
            unsafe {
                let src_ptr = src.as_ptr().add(3);
                let dst_ptr = dst.as_mut_ptr();
                core::ptr::copy_nonoverlapping(src_ptr, dst_ptr, 48);
            }
        }

        let mut src = [0u8; 64];
        let mut i = 0usize;
        while i < 64 {
            src[i] = i as u8;
            i += 1;
        }

        let mut dst = [0u8; 48];
        do_copy(&mut dst, &src);

        let mut mismatches = 0u32;
        let mut i = 0usize;
        while i < 48 {
            if dst[i] != (i as u8).wrapping_add(3) {
                mismatches += 1;
            }
            i += 1;
        }

        Felt::from_u32(mismatches)
    }"#;

    setup::enable_compiler_instrumentation();
    let config = WasmTranslationConfig::default();
    let mut test = CompilerTest::rust_fn_body_with_stdlib_sys(
        "memory_copy_unaligned_src_len_48_u8s",
        main_fn,
        config,
        [],
    );

    let package = test.compile_package();
    let args: [Felt; 0] = [];

    eval_package::<Felt, _, _>(&package, [], &args, &test.session, |trace| {
        let res: Felt = trace.parse_result().unwrap();
        assert_eq!(res, Felt::ZERO);
        Ok(())
    })
    .unwrap();
}

#[test]
fn test_memory_copy_unaligned_dst() {
    let main_fn = r#"() -> Felt {
        #[inline(never)]
        fn do_copy(dst: &mut [u8; 53], src: &[u8; 64]) {
            unsafe {
                let src_ptr = src.as_ptr().add(3);
                let dst_ptr = dst.as_mut_ptr().add(5);
                core::ptr::copy_nonoverlapping(src_ptr, dst_ptr, 48);
            }
        }

        let mut src = [0u8; 64];
        let mut i = 0usize;
        while i < 64 {
            src[i] = i as u8;
            i += 1;
        }

        let mut dst = [0xffu8; 53];
        do_copy(&mut dst, &src);

        let mut mismatches = 0u32;
        let mut i = 0usize;
        while i < 53 {
            let expected = if i < 5 { 0xff } else { (i as u8).wrapping_sub(2) };
            if dst[i] != expected {
                mismatches += 1;
            }
            i += 1;
        }

        Felt::from_u32(mismatches)
    }"#;

    setup::enable_compiler_instrumentation();
    let config = WasmTranslationConfig::default();
    let mut test = CompilerTest::rust_fn_body_with_stdlib_sys(
        "memory_copy_unaligned_dst_len_48_u8s",
        main_fn,
        config,
        [],
    );

    let package = test.compile_package();
    let args: [Felt; 0] = [];

    eval_package::<Felt, _, _>(&package, [], &args, &test.session, |trace| {
        let res: Felt = trace.parse_result().unwrap();
        assert_eq!(res, Felt::ZERO);
        Ok(())
    })
    .unwrap();
}

#[test]
fn test_memory_copy_unaligned_dst_short_count() {
    let main_fn = r#"() -> Felt {
        #[inline(never)]
        fn do_copy(dst: &mut [u8; 8], src: &[u8; 16]) {
            unsafe {
                let src_ptr = src.as_ptr().add(3);
                let dst_ptr = dst.as_mut_ptr().add(2);
                core::ptr::copy_nonoverlapping(src_ptr, dst_ptr, 3);
            }
        }

        let mut src = [0u8; 16];
        let mut i = 0usize;
        while i < 16 {
            src[i] = i as u8;
            i += 1;
        }

        let mut dst = [0xffu8; 8];
        do_copy(&mut dst, &src);

        let expected = [0xffu8, 0xff, 3, 4, 5, 0xff, 0xff, 0xff];
        let mut mismatches = 0u32;
        let mut i = 0usize;
        while i < 8 {
            if dst[i] != expected[i] {
                mismatches += 1;
            }
            i += 1;
        }

        Felt::from_u32(mismatches)
    }"#;

    setup::enable_compiler_instrumentation();
    let config = WasmTranslationConfig::default();
    let mut test = CompilerTest::rust_fn_body_with_stdlib_sys(
        "memory_copy_unaligned_dst_short_count_u8s",
        main_fn,
        config,
        [],
    );

    let package = test.compile_package();
    let args: [Felt; 0] = [];

    eval_package::<Felt, _, _>(&package, [], &args, &test.session, |trace| {
        let res: Felt = trace.parse_result().unwrap();
        assert_eq!(res, Felt::ZERO);
        Ok(())
    })
    .unwrap();
}

#[test]
fn test_memory_copy_unaligned_zero_count() {
    let main_fn = r#"() -> Felt {
        #[inline(never)]
        fn do_copy(dst: &mut [u8; 8], src: &[u8; 16]) {
            unsafe {
                let src_ptr = src.as_ptr().add(1);
                let dst_ptr = dst.as_mut_ptr().add(2);
                core::ptr::copy_nonoverlapping(src_ptr, dst_ptr, 0);
            }
        }

        let mut src = [0u8; 16];
        let mut i = 0usize;
        while i < 16 {
            src[i] = i as u8;
            i += 1;
        }

        let mut dst = [0xffu8; 8];
        do_copy(&mut dst, &src);

        let expected = [0xffu8; 8];
        let mut mismatches = 0u32;
        let mut i = 0usize;
        while i < 8 {
            if dst[i] != expected[i] {
                mismatches += 1;
            }
            i += 1;
        }

        Felt::from_u32(mismatches)
    }"#;

    setup::enable_compiler_instrumentation();
    let config = WasmTranslationConfig::default();
    let mut test = CompilerTest::rust_fn_body_with_stdlib_sys(
        "memory_copy_unaligned_zero_count_u8s",
        main_fn,
        config,
        [],
    );

    let package = test.compile_package();
    let args: [Felt; 0] = [];

    eval_package::<Felt, _, _>(&package, [], &args, &test.session, |trace| {
        let res: Felt = trace.parse_result().unwrap();
        assert_eq!(res, Felt::ZERO);
        Ok(())
    })
    .unwrap();
}

#[test]
fn test_memory_set_unaligned() {
    let main_fn = r#"() -> Felt {
        #[inline(never)]
        fn do_set(dst: &mut [u8; 11]) {
            unsafe {
                let dst_ptr = dst.as_mut_ptr().add(3);
                core::ptr::write_bytes(dst_ptr, 0x5a, 5);
            }
        }

        let mut dst = [0xffu8; 11];
        do_set(&mut dst);

        let expected = [0xffu8, 0xff, 0xff, 0x5a, 0x5a, 0x5a, 0x5a, 0x5a, 0xff, 0xff, 0xff];
        let mut mismatches = 0u32;
        let mut i = 0usize;
        while i < 11 {
            if dst[i] != expected[i] {
                mismatches += 1;
            }
            i += 1;
        }

        Felt::from_u32(mismatches)
    }"#;

    setup::enable_compiler_instrumentation();
    let config = WasmTranslationConfig::default();
    let mut test =
        CompilerTest::rust_fn_body_with_stdlib_sys("memory_set_unaligned_u8s", main_fn, config, []);

    let package = test.compile_package();
    let args: [Felt; 0] = [];

    eval_package::<Felt, _, _>(&package, [], &args, &test.session, |trace| {
        let res: Felt = trace.parse_result().unwrap();
        assert_eq!(res, Felt::ZERO);
        Ok(())
    })
    .unwrap();
}

#[test]
fn test_memory_set_unaligned_zero_count() {
    let main_fn = r#"() -> Felt {
        #[inline(never)]
        fn do_set(dst: &mut [u8; 11]) {
            unsafe {
                let dst_ptr = dst.as_mut_ptr().add(3);
                core::ptr::write_bytes(dst_ptr, 0x5a, 0);
            }
        }

        let mut dst = [0xffu8; 11];
        do_set(&mut dst);

        let expected = [0xffu8; 11];
        let mut mismatches = 0u32;
        let mut i = 0usize;
        while i < 11 {
            if dst[i] != expected[i] {
                mismatches += 1;
            }
            i += 1;
        }

        Felt::from_u32(mismatches)
    }"#;

    setup::enable_compiler_instrumentation();
    let config = WasmTranslationConfig::default();
    let mut test = CompilerTest::rust_fn_body_with_stdlib_sys(
        "memory_set_unaligned_zero_count_u8s",
        main_fn,
        config,
        [],
    );

    let package = test.compile_package();
    let args: [Felt; 0] = [];

    eval_package::<Felt, _, _>(&package, [], &args, &test.session, |trace| {
        let res: Felt = trace.parse_result().unwrap();
        assert_eq!(res, Felt::ZERO);
        Ok(())
    })
    .unwrap();
}
