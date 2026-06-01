use std::{any::type_name, marker::PhantomData};

use miden_core::Felt;
use miden_debug::{FromMidenRepr, ToMidenRepr, push_wasm_ty_to_operand_stack};
use midenc_frontend_wasm::WasmTranslationConfig;
use num_traits::{PrimInt, ToBytes, Unsigned};
use proptest::{
    prelude::*,
    test_runner::{TestError, TestRunner},
};

use crate::{
    CompilerTest,
    testing::{eval_package, run_masm_vs_rust},
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
                        push_wasm_ty_to_operand_stack(a, &mut args);
                        push_wasm_ty_to_operand_stack(b, &mut args);
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
fn overflowing_add_u8() {
    test_overflowing_arith(u8::overflowing_add, "overflowing_add", NumericStrategy::add_unsigned());
}

#[test]
fn overflowing_add_u16() {
    test_overflowing_arith(
        u16::overflowing_add,
        "overflowing_add",
        NumericStrategy::add_unsigned(),
    );
}

#[test]
fn overflowing_add_u32() {
    test_overflowing_arith(
        u32::overflowing_add,
        "overflowing_add",
        NumericStrategy::add_unsigned(),
    );
}

#[test]
fn overflowing_add_u64() {
    test_overflowing_arith(
        u64::overflowing_add,
        "overflowing_add",
        NumericStrategy::add_unsigned(),
    );
}

#[test]
fn overflowing_add_u128() {
    test_overflowing_arith(
        u128::overflowing_add,
        "overflowing_add",
        NumericStrategy::add_unsigned(),
    );
}

#[test]
fn overflowing_add_i8() {
    test_overflowing_arith(i8::overflowing_add, "overflowing_add", NumericStrategy::add_signed());
}

#[test]
fn overflowing_add_i16() {
    test_overflowing_arith(i16::overflowing_add, "overflowing_add", NumericStrategy::add_signed());
}

#[test]
fn overflowing_add_i32() {
    test_overflowing_arith(i32::overflowing_add, "overflowing_add", NumericStrategy::add_signed());
}

#[test]
fn overflowing_add_i64() {
    test_overflowing_arith(i64::overflowing_add, "overflowing_add", NumericStrategy::add_signed());
}

#[test]
fn overflowing_add_i128() {
    test_overflowing_arith(i128::overflowing_add, "overflowing_add", NumericStrategy::add_signed());
}

#[test]
fn overflowing_sub_u8() {
    test_overflowing_arith(u8::overflowing_sub, "overflowing_sub", NumericStrategy::sub_unsigned());
}

#[test]
fn overflowing_sub_u16() {
    test_overflowing_arith(
        u16::overflowing_sub,
        "overflowing_sub",
        NumericStrategy::sub_unsigned(),
    );
}

#[test]
fn overflowing_sub_u32() {
    test_overflowing_arith(
        u32::overflowing_sub,
        "overflowing_sub",
        NumericStrategy::sub_unsigned(),
    );
}

#[test]
fn overflowing_sub_u64() {
    test_overflowing_arith(
        u64::overflowing_sub,
        "overflowing_sub",
        NumericStrategy::sub_unsigned(),
    );
}

#[test]
fn overflowing_sub_u128() {
    test_overflowing_arith(
        u128::overflowing_sub,
        "overflowing_sub",
        NumericStrategy::sub_unsigned(),
    );
}

#[test]
fn overflowing_sub_i8() {
    test_overflowing_arith(i8::overflowing_sub, "overflowing_sub", NumericStrategy::sub_signed());
}

#[test]
fn overflowing_sub_i16() {
    test_overflowing_arith(i16::overflowing_sub, "overflowing_sub", NumericStrategy::sub_signed());
}

#[test]
fn overflowing_sub_i32() {
    test_overflowing_arith(i32::overflowing_sub, "overflowing_sub", NumericStrategy::sub_signed());
}

#[test]
fn overflowing_sub_i64() {
    test_overflowing_arith(i64::overflowing_sub, "overflowing_sub", NumericStrategy::sub_signed());
}

#[test]
fn overflowing_sub_i128() {
    test_overflowing_arith(i128::overflowing_sub, "overflowing_sub", NumericStrategy::sub_signed());
}

#[test]
fn overflowing_mul_u8() {
    test_overflowing_arith(u8::overflowing_mul, "overflowing_mul", NumericStrategy::mul_unsigned());
}

#[test]
fn overflowing_mul_u16() {
    test_overflowing_arith(
        u16::overflowing_mul,
        "overflowing_mul",
        NumericStrategy::mul_unsigned(),
    );
}

#[test]
fn overflowing_mul_u32() {
    test_overflowing_arith(
        u32::overflowing_mul,
        "overflowing_mul",
        NumericStrategy::mul_unsigned(),
    );
}

#[test]
fn overflowing_mul_u64() {
    test_overflowing_arith(
        u64::overflowing_mul,
        "overflowing_mul",
        NumericStrategy::mul_unsigned(),
    );
}

#[test]
fn overflowing_mul_u128() {
    test_overflowing_arith(
        u128::overflowing_mul,
        "overflowing_mul",
        NumericStrategy::mul_unsigned(),
    );
}

#[test]
fn overflowing_mul_i8() {
    test_overflowing_arith(i8::overflowing_mul, "overflowing_mul", NumericStrategy::mul_signed());
}

#[test]
fn overflowing_mul_i16() {
    test_overflowing_arith(i16::overflowing_mul, "overflowing_mul", NumericStrategy::mul_signed());
}

#[test]
fn overflowing_mul_i32() {
    test_overflowing_arith(i32::overflowing_mul, "overflowing_mul", NumericStrategy::mul_signed());
}

#[test]
fn overflowing_mul_i64() {
    test_overflowing_arith(i64::overflowing_mul, "overflowing_mul", NumericStrategy::mul_signed());
}

#[test]
fn overflowing_mul_i128() {
    test_overflowing_arith(i128::overflowing_mul, "overflowing_mul", NumericStrategy::mul_signed());
}

#[test]
fn overflowing_div_u8() {
    test_overflowing_arith(
        u8::overflowing_div,
        "overflowing_div",
        NumericStrategy::div_unsigned_overflowing(),
    );
}

#[test]
fn overflowing_div_u16() {
    test_overflowing_arith(
        u16::overflowing_div,
        "overflowing_div",
        NumericStrategy::div_unsigned_overflowing(),
    );
}

#[test]
fn overflowing_div_u32() {
    test_overflowing_arith(
        u32::overflowing_div,
        "overflowing_div",
        NumericStrategy::div_unsigned_overflowing(),
    );
}

#[test]
fn overflowing_div_u64() {
    test_overflowing_arith(
        u64::overflowing_div,
        "overflowing_div",
        NumericStrategy::div_unsigned_overflowing(),
    );
}

#[test]
fn overflowing_div_u128() {
    test_overflowing_arith(
        u128::overflowing_div,
        "overflowing_div",
        NumericStrategy::div_unsigned_overflowing(),
    );
}

#[test]
fn overflowing_div_i8() {
    test_overflowing_arith(
        i8::overflowing_div,
        "overflowing_div",
        NumericStrategy::div_signed_overflowing(),
    );
}

#[test]
fn overflowing_div_i16() {
    test_overflowing_arith(
        i16::overflowing_div,
        "overflowing_div",
        NumericStrategy::div_signed_overflowing(),
    );
}

#[test]
fn overflowing_div_i32() {
    test_overflowing_arith(
        i32::overflowing_div,
        "overflowing_div",
        NumericStrategy::div_signed_overflowing(),
    );
}

#[test]
fn overflowing_div_i64() {
    test_overflowing_arith(
        i64::overflowing_div,
        "overflowing_div",
        NumericStrategy::div_signed_overflowing(),
    );
}

#[test]
fn overflowing_div_i128() {
    test_overflowing_arith(
        i128::overflowing_div,
        "overflowing_div",
        NumericStrategy::div_signed_overflowing(),
    );
}

#[test]
fn overflowing_rem_u8() {
    test_overflowing_arith(
        u8::overflowing_rem,
        "overflowing_rem",
        NumericStrategy::rem_unsigned_overflowing(),
    );
}

#[test]
fn overflowing_rem_u16() {
    test_overflowing_arith(
        u16::overflowing_rem,
        "overflowing_rem",
        NumericStrategy::rem_unsigned_overflowing(),
    );
}

#[test]
fn overflowing_rem_u32() {
    test_overflowing_arith(
        u32::overflowing_rem,
        "overflowing_rem",
        NumericStrategy::rem_unsigned_overflowing(),
    );
}

#[test]
fn overflowing_rem_u64() {
    test_overflowing_arith(
        u64::overflowing_rem,
        "overflowing_rem",
        NumericStrategy::rem_unsigned_overflowing(),
    );
}

#[test]
fn overflowing_rem_u128() {
    test_overflowing_arith(
        u128::overflowing_rem,
        "overflowing_rem",
        NumericStrategy::rem_unsigned_overflowing(),
    );
}

#[test]
#[ignore = "https://github.com/0xMiden/compiler/issues/1152"]
fn overflowing_rem_i8() {
    test_overflowing_arith(
        i8::overflowing_rem,
        "overflowing_rem",
        NumericStrategy::rem_signed_overflowing(),
    );
}

#[test]
#[ignore = "https://github.com/0xMiden/compiler/issues/1152"]
fn overflowing_rem_i16() {
    test_overflowing_arith(
        i16::overflowing_rem,
        "overflowing_rem",
        NumericStrategy::rem_signed_overflowing(),
    );
}

#[test]
#[ignore = "https://github.com/0xMiden/compiler/issues/1152"]
fn overflowing_rem_i32() {
    test_overflowing_arith(
        i32::overflowing_rem,
        "overflowing_rem",
        NumericStrategy::rem_signed_overflowing(),
    );
}

#[test]
#[ignore = "https://github.com/0xMiden/compiler/issues/1000"]
fn overflowing_rem_i64() {
    test_overflowing_arith(
        i64::overflowing_rem,
        "overflowing_rem",
        NumericStrategy::rem_signed_overflowing(),
    );
}

#[test]
fn overflowing_rem_i128() {
    test_overflowing_arith(
        i128::overflowing_rem,
        "overflowing_rem",
        NumericStrategy::rem_signed_overflowing(),
    );
}

#[test]
fn checked_add_u8() {
    test_checked_arith(u8::checked_add, "checked_add", NumericStrategy::add_unsigned());
}

#[test]
fn checked_add_u16() {
    test_checked_arith(u16::checked_add, "checked_add", NumericStrategy::add_unsigned());
}

#[test]
fn checked_add_u32() {
    test_checked_arith(u32::checked_add, "checked_add", NumericStrategy::add_unsigned());
}

#[test]
fn checked_add_u64() {
    test_checked_arith(u64::checked_add, "checked_add", NumericStrategy::add_unsigned());
}

#[test]
fn checked_add_i8() {
    test_checked_arith(i8::checked_add, "checked_add", NumericStrategy::add_signed());
}

#[test]
fn checked_add_i16() {
    test_checked_arith(i16::checked_add, "checked_add", NumericStrategy::add_signed());
}

#[test]
fn checked_add_i32() {
    test_checked_arith(i32::checked_add, "checked_add", NumericStrategy::add_signed());
}

#[test]
fn checked_add_i64() {
    test_checked_arith(i64::checked_add, "checked_add", NumericStrategy::add_signed());
}

#[test]
fn checked_sub_u8() {
    test_checked_arith(u8::checked_sub, "checked_sub", NumericStrategy::sub_unsigned());
}

#[test]
fn checked_sub_u16() {
    test_checked_arith(u16::checked_sub, "checked_sub", NumericStrategy::sub_unsigned());
}

#[test]
fn checked_sub_u32() {
    test_checked_arith(u32::checked_sub, "checked_sub", NumericStrategy::sub_unsigned());
}

#[test]
fn checked_sub_u64() {
    test_checked_arith(u64::checked_sub, "checked_sub", NumericStrategy::sub_unsigned());
}

#[test]
fn checked_sub_i8() {
    test_checked_arith(i8::checked_sub, "checked_sub", NumericStrategy::sub_signed());
}

#[test]
fn checked_sub_i16() {
    test_checked_arith(i16::checked_sub, "checked_sub", NumericStrategy::sub_signed());
}

#[test]
fn checked_sub_i32() {
    test_checked_arith(i32::checked_sub, "checked_sub", NumericStrategy::sub_signed());
}

#[test]
fn checked_sub_i64() {
    test_checked_arith(i64::checked_sub, "checked_sub", NumericStrategy::sub_signed());
}

#[test]
fn checked_mul_u8() {
    test_checked_arith(u8::checked_mul, "checked_mul", NumericStrategy::mul_unsigned());
}

#[test]
fn checked_mul_u16() {
    test_checked_arith(u16::checked_mul, "checked_mul", NumericStrategy::mul_unsigned());
}

#[test]
fn checked_mul_u32() {
    test_checked_arith(u32::checked_mul, "checked_mul", NumericStrategy::mul_unsigned());
}

#[test]
fn checked_mul_u64() {
    test_checked_arith(u64::checked_mul, "checked_mul", NumericStrategy::mul_unsigned());
}

#[test]
fn checked_mul_i8() {
    test_checked_arith(i8::checked_mul, "checked_mul", NumericStrategy::mul_signed());
}

#[test]
fn checked_mul_i16() {
    test_checked_arith(i16::checked_mul, "checked_mul", NumericStrategy::mul_signed());
}

#[test]
fn checked_mul_i32() {
    test_checked_arith(i32::checked_mul, "checked_mul", NumericStrategy::mul_signed());
}

#[test]
fn checked_mul_i64_happy_path() {
    test_checked_arith(i64::checked_mul, "checked_mul", (any::<i64>(), any::<i64>()));
}

#[test]
#[ignore = "https://github.com/0xMiden/compiler/issues/1144 once this is resolved, remove \
            checked_mul_i64_happy_path"]
fn checked_mul_i64() {
    test_checked_arith(i64::checked_mul, "checked_mul", NumericStrategy::mul_signed());
}

#[test]
fn checked_div_u8() {
    test_checked_arith(u8::checked_div, "checked_div", NumericStrategy::div_unsigned_checked());
}

#[test]
fn checked_div_u16() {
    test_checked_arith(u16::checked_div, "checked_div", NumericStrategy::div_unsigned_checked());
}

#[test]
fn checked_div_u32() {
    test_checked_arith(u32::checked_div, "checked_div", NumericStrategy::div_unsigned_checked());
}

#[test]
fn checked_div_u64() {
    test_checked_arith(u64::checked_div, "checked_div", NumericStrategy::div_unsigned_checked());
}

#[test]
fn checked_div_i8() {
    test_checked_arith(i8::checked_div, "checked_div", NumericStrategy::div_signed_checked());
}

#[test]
fn checked_div_i16() {
    test_checked_arith(i16::checked_div, "checked_div", NumericStrategy::div_signed_checked());
}

#[test]
fn checked_div_i32() {
    test_checked_arith(i32::checked_div, "checked_div", NumericStrategy::div_signed_checked());
}

#[test]
fn checked_div_i64() {
    test_checked_arith(i64::checked_div, "checked_div", NumericStrategy::div_signed_checked());
}

#[test]
fn checked_rem_u8() {
    test_checked_arith(u8::checked_rem, "checked_rem", NumericStrategy::rem_unsigned_checked());
}

#[test]
fn checked_rem_u16() {
    test_checked_arith(u16::checked_rem, "checked_rem", NumericStrategy::rem_unsigned_checked());
}

#[test]
fn checked_rem_u32() {
    test_checked_arith(u32::checked_rem, "checked_rem", NumericStrategy::rem_unsigned_checked());
}

#[test]
fn checked_rem_u64() {
    test_checked_arith(u64::checked_rem, "checked_rem", NumericStrategy::rem_unsigned_checked());
}

#[test]
#[ignore = "https://github.com/0xMiden/compiler/issues/1152"]
fn checked_rem_i8() {
    test_checked_arith(i8::checked_rem, "checked_rem", NumericStrategy::rem_signed_checked());
}

#[test]
#[ignore = "https://github.com/0xMiden/compiler/issues/1152"]
fn checked_rem_i16() {
    test_checked_arith(i16::checked_rem, "checked_rem", NumericStrategy::rem_signed_checked());
}

#[test]
#[ignore = "https://github.com/0xMiden/compiler/issues/1152"]
fn checked_rem_i32() {
    test_checked_arith(i32::checked_rem, "checked_rem", NumericStrategy::rem_signed_checked());
}

#[test]
#[ignore = "https://github.com/0xMiden/compiler/issues/1000"]
fn checked_rem_i64() {
    test_checked_arith(i64::checked_rem, "checked_rem", NumericStrategy::rem_signed_checked());
}

struct NumericStrategy<T> {
    _marker: PhantomData<T>,
}

impl<T> NumericStrategy<T>
where
    T: PrimInt + Arbitrary + 'static,
    std::ops::RangeInclusive<T>: Strategy<Value = T>,
{
    fn add_unsigned() -> impl Strategy<Value = (T, T)>
    where
        T: Unsigned,
    {
        let v = NumericStrategyValues::<T>::new();
        prop_oneof![
            150 => (any::<T>(), any::<T>()),
            1 => Just((v.max, v.one)),
            1 => Just((v.one, v.max)),
            1 => Just((v.max, v.max)),
            1 => Just((v.half, v.half)),
            1 => Just((v.half, v.half_plus_one)),
            1 => Just((v.half_plus_one, v.half)),
            1 => Just((v.half_plus_one, v.half_plus_one)),
            1 => Just((v.max, v.zero)),
            1 => Just((v.zero, v.max)),
            1 => Just((v.zero, v.zero)),
            1 => Just((v.one, v.zero)),
            1 => Just((v.zero, v.one)),
            1 => Just((v.two, v.max)),
            1 => Just((v.max, v.two)),
            1 => Just((v.three, v.three)),
        ]
    }

    fn add_signed() -> impl Strategy<Value = (T, T)>
    where
        T: num_traits::Signed,
    {
        let v = NumericStrategyValues::<T>::new();
        let neg_one = v.neg_one.unwrap();
        prop_oneof![
            150 => (any::<T>(), any::<T>()),
            1 => Just((v.max, v.one)),
            1 => Just((v.one, v.max)),
            1 => Just((v.max, v.max)),
            1 => Just((v.min, neg_one)),
            1 => Just((neg_one, v.min)),
            1 => Just((v.min, v.min)),
            1 => Just((v.half, v.half_plus_one)),
            1 => Just((v.half_plus_one, v.half)),
            1 => Just((v.zero, v.zero)),
            1 => Just((v.max, v.zero)),
            1 => Just((v.min, v.zero)),
            1 => Just((v.zero, v.max)),
            1 => Just((v.zero, v.min)),
            1 => Just((v.max, neg_one)),
            1 => Just((neg_one, v.max)),
        ]
    }

    fn sub_unsigned() -> impl Strategy<Value = (T, T)>
    where
        T: Unsigned,
    {
        let v = NumericStrategyValues::<T>::new();
        prop_oneof![
            150 => (any::<T>(), any::<T>()),
            1 => Just((v.zero, v.one)),
            1 => Just((v.zero, v.max)),
            1 => Just((v.max, v.max)),
            1 => Just((v.max, v.zero)),
            1 => Just((v.max, v.one)),
            1 => Just((v.half, v.half)),
            1 => Just((v.half_plus_one, v.half)),
            1 => Just((v.half, v.half_plus_one)),
            1 => Just((v.one, v.one)),
            1 => Just((v.zero, v.zero)),
            1 => Just((v.one, v.max)),
            1 => Just((v.two, v.max)),
        ]
    }

    fn sub_signed() -> impl Strategy<Value = (T, T)>
    where
        T: num_traits::Signed,
    {
        let v = NumericStrategyValues::<T>::new();
        let neg_one = v.neg_one.unwrap();
        prop_oneof![
            150 => (any::<T>(), any::<T>()),
            1 => Just((v.min, v.one)),
            1 => Just((v.min, v.max)),
            1 => Just((v.max, v.min)),
            1 => Just((v.max, neg_one)),
            1 => Just((neg_one, v.max)),
            1 => Just((v.min, neg_one)),
            1 => Just((v.zero, v.min)),
            1 => Just((v.max, v.max)),
            1 => Just((v.min, v.min)),
            1 => Just((v.zero, v.zero)),
            1 => Just((v.max, v.zero)),
            1 => Just((v.min, v.zero)),
            1 => Just((v.zero, v.max)),
        ]
    }

    fn mul_unsigned() -> impl Strategy<Value = (T, T)>
    where
        T: Unsigned,
    {
        let v = NumericStrategyValues::<T>::new();
        prop_oneof![
            150 => (any::<T>(), any::<T>()),
            1 => Just((v.max, v.two)),
            1 => Just((v.two, v.max)),
            1 => Just((v.max, v.max)),
            1 => Just((v.half, v.two)),
            1 => Just((v.two, v.half)),
            1 => Just((v.half_plus_one, v.two)),
            1 => Just((v.two, v.half_plus_one)),
            1 => Just((v.max, v.one)),
            1 => Just((v.one, v.max)),
            1 => Just((v.max, v.zero)),
            1 => Just((v.zero, v.max)),
            1 => Just((v.zero, v.zero)),
            1 => Just((v.one, v.one)),
            1 => Just((v.two, v.two)),
            1 => Just((v.three, v.three)),
            1 => Just((v.half, v.half)),
            1 => Just((v.sqrt_max, v.sqrt_max)),
            1 => Just((v.sqrt_max, v.sqrt_max_plus_one)),
            1 => Just((v.sqrt_max_plus_one, v.sqrt_max)),
            1 => Just((v.sqrt_max_plus_one, v.sqrt_max_plus_one)),
            1 => Just((v.max_div_three, v.three)),
            1 => Just((v.three, v.max_div_three)),
            1 => Just((v.max_div_three_plus_one, v.three)),
            1 => Just((v.three, v.max_div_three_plus_one)),
            1 => Just((v.max_div_four, v.four)),
            1 => Just((v.four, v.max_div_four)),
            1 => Just((v.max_div_four_plus_one, v.four)),
            1 => Just((v.four, v.max_div_four_plus_one)),
        ]
    }

    fn mul_signed() -> impl Strategy<Value = (T, T)>
    where
        T: num_traits::Signed + 'static,
    {
        let v = NumericStrategyValues::<T>::new();
        let neg_one = v.neg_one.unwrap();
        let neg_two = v.zero - v.two;
        let neg_three = v.zero - v.three;
        let neg_four = v.zero - v.four;
        let neg_sqrt_max = v.zero - v.sqrt_max;
        let neg_sqrt_max_plus_one = v.zero - v.sqrt_max_plus_one;
        let neg_max_div_two = v.zero - v.half;
        let neg_max_div_two_plus_one = v.zero - v.half_plus_one;
        let neg_max_div_three = v.zero - v.max_div_three;
        let neg_max_div_three_plus_one = v.zero - v.max_div_three_plus_one;
        let neg_max_div_four = v.zero - v.max_div_four;
        let neg_max_div_four_plus_one = v.zero - v.max_div_four_plus_one;
        let min_div_two = v.min / v.two;
        let min_div_two_minus_one = min_div_two - v.one;
        let min_div_three = v.min / v.three;
        let min_div_three_minus_one = min_div_three - v.one;
        let min_div_four = v.min / v.four;
        let min_div_four_minus_one = min_div_four - v.one;
        prop_oneof![
            150 => (any::<T>(), any::<T>()),
            1 => Just((v.max, v.two)),
            1 => Just((v.two, v.max)),
            1 => Just((v.max, v.max)),
            1 => Just((v.half, v.two)),
            1 => Just((v.two, v.half)),
            1 => Just((v.half_plus_one, v.two)),
            1 => Just((v.two, v.half_plus_one)),
            1 => Just((v.max, v.one)),
            1 => Just((v.one, v.max)),
            1 => Just((v.min, v.one)),
            1 => Just((v.one, v.min)),
            1 => Just((v.max, v.zero)),
            1 => Just((v.zero, v.max)),
            1 => Just((v.min, v.zero)),
            1 => Just((v.zero, v.min)),
            1 => Just((v.zero, v.zero)),
            1 => Just((v.one, v.one)),
            1 => Just((v.two, v.two)),
            1 => Just((v.three, v.three)),
            1 => Just((v.min, neg_one)),
            1 => Just((neg_one, v.min)),
            1 => Just((v.max, neg_one)),
            1 => Just((neg_one, v.max)),
            1 => Just((v.sqrt_max, v.sqrt_max)),
            1 => Just((v.sqrt_max, v.sqrt_max_plus_one)),
            1 => Just((v.sqrt_max_plus_one, v.sqrt_max)),
            1 => Just((v.sqrt_max_plus_one, v.sqrt_max_plus_one)),
            1 => Just((neg_sqrt_max, neg_sqrt_max)),
            1 => Just((neg_sqrt_max, neg_sqrt_max_plus_one)),
            1 => Just((neg_sqrt_max_plus_one, neg_sqrt_max)),
            1 => Just((neg_sqrt_max_plus_one, neg_sqrt_max_plus_one)),
            1 => Just((v.max_div_three, v.three)),
            1 => Just((v.three, v.max_div_three)),
            1 => Just((v.max_div_three_plus_one, v.three)),
            1 => Just((v.three, v.max_div_three_plus_one)),
            1 => Just((v.max_div_four, v.four)),
            1 => Just((v.four, v.max_div_four)),
            1 => Just((v.max_div_four_plus_one, v.four)),
            1 => Just((v.four, v.max_div_four_plus_one)),
            1 => Just((neg_max_div_two, neg_two)),
            1 => Just((neg_two, neg_max_div_two)),
            1 => Just((neg_max_div_two_plus_one, neg_two)),
            1 => Just((neg_two, neg_max_div_two_plus_one)),
            1 => Just((neg_max_div_three, neg_three)),
            1 => Just((neg_three, neg_max_div_three)),
            1 => Just((neg_max_div_three_plus_one, neg_three)),
            1 => Just((neg_three, neg_max_div_three_plus_one)),
            1 => Just((neg_max_div_four, neg_four)),
            1 => Just((neg_four, neg_max_div_four)),
            1 => Just((neg_max_div_four_plus_one, neg_four)),
            1 => Just((neg_four, neg_max_div_four_plus_one)),
            1 => Just((min_div_two, v.two)),
            1 => Just((v.two, min_div_two)),
            1 => Just((min_div_two_minus_one, v.two)),
            1 => Just((v.two, min_div_two_minus_one)),
            1 => Just((min_div_three, v.three)),
            1 => Just((v.three, min_div_three)),
            1 => Just((min_div_three_minus_one, v.three)),
            1 => Just((v.three, min_div_three_minus_one)),
            1 => Just((min_div_four, v.four)),
            1 => Just((v.four, min_div_four)),
            1 => Just((min_div_four_minus_one, v.four)),
            1 => Just((v.four, min_div_four_minus_one)),
        ]
    }

    /// Checked remainder and division don't panic on zero rhs.
    fn div_unsigned_checked() -> impl Strategy<Value = (T, T)>
    where
        T: Unsigned,
    {
        let v = NumericStrategyValues::<T>::new();
        prop_oneof![
            150 => (any::<T>(), any::<T>()),
            1 => Just((v.max, v.one)),
            1 => Just((v.max, v.two)),
            1 => Just((v.max, v.max)),
            1 => Just((v.one, v.max)),
            1 => Just((v.zero, v.one)),
            1 => Just((v.zero, v.max)),
            1 => Just((v.half, v.two)),
            1 => Just((v.half_plus_one, v.two)),
            1 => Just((v.two, v.max)),
            1 => Just((v.max, v.zero)),
            1 => Just((v.zero, v.zero)),
            1 => Just((v.one, v.zero)),
        ]
    }

    fn div_unsigned_overflowing() -> impl Strategy<Value = (T, T)>
    where
        T: Unsigned,
    {
        let v = NumericStrategyValues::<T>::new();
        prop_oneof![
            150 => (any::<T>(), v.one..=v.max),
            1 => Just((v.max, v.one)),
            1 => Just((v.max, v.two)),
            1 => Just((v.max, v.max)),
            1 => Just((v.one, v.max)),
            1 => Just((v.zero, v.one)),
            1 => Just((v.zero, v.max)),
            1 => Just((v.half, v.two)),
            1 => Just((v.half_plus_one, v.two)),
            1 => Just((v.two, v.max)),
            1 => Just((v.three, v.max)),
        ]
    }

    /// Checked remainder and division don't panic on zero rhs.
    fn div_signed_checked() -> impl Strategy<Value = (T, T)>
    where
        T: num_traits::Signed,
    {
        let v = NumericStrategyValues::<T>::new();
        let neg_one = v.neg_one.unwrap();
        prop_oneof![
            150 => (any::<T>(), any::<T>()),
            1 => Just((v.max, v.one)),
            1 => Just((v.max, neg_one)),
            1 => Just((v.min, v.one)),
            1 => Just((v.min, neg_one)),
            1 => Just((v.min, v.two)),
            1 => Just((v.max, v.two)),
            1 => Just((v.zero, v.one)),
            1 => Just((v.zero, v.min)),
            1 => Just((v.max, v.zero)),
            1 => Just((v.min, v.zero)),
            1 => Just((v.zero, v.zero)),
        ]
    }

    fn div_signed_overflowing() -> impl Strategy<Value = (T, T)>
    where
        T: num_traits::Signed,
    {
        let v = NumericStrategyValues::<T>::new();
        let neg_one = v.neg_one.unwrap();
        prop_oneof![
            75 => (any::<T>(), v.min..=neg_one),
            75 => (any::<T>(), v.one..=v.max),
            1 => Just((v.max, v.one)),
            1 => Just((v.max, neg_one)),
            1 => Just((v.min, v.one)),
            1 => Just((v.min, neg_one)),
            1 => Just((v.min, v.two)),
            1 => Just((v.max, v.two)),
            1 => Just((v.zero, v.one)),
            1 => Just((v.zero, v.min)),
            1 => Just((neg_one, v.min)),
            1 => Just((neg_one, v.max)),
        ]
    }

    /// Checked remainder and division don't panic on zero rhs.
    fn rem_unsigned_checked() -> impl Strategy<Value = (T, T)>
    where
        T: Unsigned,
    {
        let v = NumericStrategyValues::<T>::new();
        prop_oneof![
            150 => (any::<T>(), any::<T>()),
            1 => Just((v.max, v.one)),
            1 => Just((v.max, v.two)),
            1 => Just((v.max, v.max)),
            1 => Just((v.one, v.max)),
            1 => Just((v.zero, v.one)),
            1 => Just((v.zero, v.max)),
            1 => Just((v.half, v.two)),
            1 => Just((v.half_plus_one, v.two)),
            1 => Just((v.max, v.zero)),
            1 => Just((v.zero, v.zero)),
            1 => Just((v.one, v.zero)),
        ]
    }

    fn rem_unsigned_overflowing() -> impl Strategy<Value = (T, T)>
    where
        T: Unsigned,
    {
        let v = NumericStrategyValues::<T>::new();
        prop_oneof![
            150 => (any::<T>(), v.one..=v.max),
            1 => Just((v.max, v.one)),
            1 => Just((v.max, v.two)),
            1 => Just((v.max, v.max)),
            1 => Just((v.one, v.max)),
            1 => Just((v.zero, v.one)),
            1 => Just((v.zero, v.max)),
            1 => Just((v.half, v.two)),
            1 => Just((v.half_plus_one, v.two)),
            1 => Just((v.two, v.max)),
        ]
    }

    /// Checked remainder and division don't panic on zero rhs.
    fn rem_signed_checked() -> impl Strategy<Value = (T, T)>
    where
        T: num_traits::Signed,
    {
        let v = NumericStrategyValues::<T>::new();
        let neg_one = v.neg_one.unwrap();
        prop_oneof![
            150 => (any::<T>(), any::<T>()),
            1 => Just((v.max, v.one)),
            1 => Just((v.max, neg_one)),
            1 => Just((v.min, v.one)),
            1 => Just((v.min, neg_one)),
            1 => Just((v.min, v.two)),
            1 => Just((v.max, v.two)),
            1 => Just((v.zero, v.one)),
            1 => Just((v.zero, v.min)),
            1 => Just((v.max, v.zero)),
            1 => Just((v.min, v.zero)),
            1 => Just((v.zero, v.zero)),
        ]
    }

    fn rem_signed_overflowing() -> impl Strategy<Value = (T, T)>
    where
        T: num_traits::Signed,
    {
        let v = NumericStrategyValues::<T>::new();
        let neg_one = v.neg_one.unwrap();
        prop_oneof![
            75 => (any::<T>(), v.min..=neg_one),
            75 => (any::<T>(), v.one..=v.max),
            1 => Just((v.max, v.one)),
            1 => Just((v.max, neg_one)),
            1 => Just((v.min, v.one)),
            1 => Just((v.min, neg_one)),
            1 => Just((v.min, v.two)),
            1 => Just((v.max, v.two)),
            1 => Just((v.zero, v.one)),
            1 => Just((v.zero, v.min)),
            1 => Just((neg_one, v.min)),
            1 => Just((neg_one, v.max)),
        ]
    }
}

/// Common values frequently used in [`NumericStrategy`].
struct NumericStrategyValues<T: PrimInt> {
    zero: T,
    one: T,
    two: T,
    three: T,
    four: T,
    half: T,
    half_plus_one: T,
    sqrt_max: T,
    sqrt_max_plus_one: T,
    max_div_three: T,
    max_div_three_plus_one: T,
    max_div_four: T,
    max_div_four_plus_one: T,
    max: T,
    min: T,
    /// Only signed types can have negative values.
    neg_one: Option<T>,
}

impl<T: PrimInt> NumericStrategyValues<T> {
    fn new() -> Self {
        let two = T::one() + T::one();
        let three = two + T::one();
        let four = two + two;
        let max = T::max_value();
        let sqrt_max = integer_sqrt(max);
        let is_signed = T::min_value() < T::zero();
        Self {
            zero: T::zero(),
            one: T::one(),
            two,
            three,
            four,
            max,
            min: T::min_value(),
            half: max / two,
            half_plus_one: max / two + T::one(),
            sqrt_max,
            sqrt_max_plus_one: sqrt_max + T::one(),
            max_div_three: max / three,
            max_div_three_plus_one: max / three + T::one(),
            max_div_four: max / four,
            max_div_four_plus_one: max / four + T::one(),
            neg_one: is_signed.then(|| T::zero() - T::one()),
        }
    }
}

fn integer_sqrt<T: PrimInt>(n: T) -> T {
    let zero = T::zero();
    let one = T::one();
    let two = one + one;
    let mut low = one;
    let mut high = n;
    let mut result = zero;

    while low <= high {
        let mid = low + (high - low) / two;
        if mid <= n / mid {
            result = mid;
            low = mid + one;
        } else {
            high = mid - one;
        }
    }

    result
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
        push_wasm_ty_to_operand_stack(a, &mut args);
        push_wasm_ty_to_operand_stack(b, &mut args);

        eval_package::<Felt, _, _>(&package, None, &args, &test.session, |trace| {
            let ty_byte_size = std::mem::size_of::<T>();
            assert!(ty_byte_size <= 16, "cannot handle types larger than 16 bytes");
            // At most 17 bytes are written to memory: ty_byte_size <= 16 and 1 byte for the bool.
            let x: [u8; 17] =
                trace.read_from_rust_memory(out_addr).expect("output was not written");
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

fn test_binary_fn<T, U>(op: fn(T, U) -> T, fn_name: &str, strategy: impl Strategy<Value = (T, U)>)
where
    T: ToBytes + ToMidenRepr + FromMidenRepr + PrimInt + Arbitrary + std::fmt::Debug,
    U: ToMidenRepr + PrimInt + Arbitrary,
{
    // The return value of `type_name` isn't stable, but it's good enough for this test.
    let lhs_ty_name = type_name::<T>();
    let rhs_ty_name = type_name::<U>();

    // Write the result to memory to handle all integer widths with one `main_fn`.
    // If the result were to be returned, it would be written to memory for 128 bit wide ints
    // and returned on the stack for smaller ints.
    let main_fn = format!(
        r#"(out: *mut {lhs_ty_name}, a: {lhs_ty_name}, b: {rhs_ty_name}) -> u32 {{
        unsafe {{ core::ptr::write(out, a.{fn_name}(b)); }}
        0
    }}"#
    );
    let config = WasmTranslationConfig::default();
    let artifact_name = format!("test_{fn_name}_{lhs_ty_name}_{rhs_ty_name}");
    let mut test =
        CompilerTest::rust_fn_body_with_stdlib_sys(artifact_name.clone(), &main_fn, config, None);
    let package = test.compile_package();

    let res = TestRunner::default().run(&strategy, move |(a, b)| {
        let rust_out = op(a, b);

        // Write the operation result to 20 * PAGE_SIZE.
        let out_addr = 20u32 * 65536;
        let mut args = Vec::<midenc_hir::Felt>::default();
        out_addr.push_to_operand_stack(&mut args);
        push_wasm_ty_to_operand_stack(a, &mut args);
        push_wasm_ty_to_operand_stack(b, &mut args);

        eval_package::<u32, _, _>(&package, None, &args, &test.session, |trace| {
            let ty_byte_size = std::mem::size_of::<T>();
            // At most 16 bytes are written to memory.
            assert!(ty_byte_size <= 16, "cannot handle types larger than 16 bytes");
            let x: [u8; 16] =
                trace.read_from_rust_memory(out_addr).expect("output was not written");
            let vm_out_bytes = x[..ty_byte_size].to_vec(); // only take what's written
            let rs_out_bytes = rust_out.to_le_bytes();

            prop_assert_eq!(rs_out_bytes.as_ref(), &vm_out_bytes, "VM output mismatch");
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
        push_wasm_ty_to_operand_stack(a, &mut args);
        push_wasm_ty_to_operand_stack(b, &mut args);

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

#[test]
fn wrapping_shl_u8() {
    test_binary_fn(u8::wrapping_shl, "wrapping_shl", (any::<u8>(), any::<u32>()));
}

#[test]
fn wrapping_shl_u16() {
    test_binary_fn(u16::wrapping_shl, "wrapping_shl", (any::<u16>(), any::<u32>()));
}

#[test]
fn wrapping_shl_u32() {
    test_binary_fn(u32::wrapping_shl, "wrapping_shl", (any::<u32>(), any::<u32>()));
}

#[test]
fn wrapping_shl_u64() {
    test_binary_fn(u64::wrapping_shl, "wrapping_shl", (any::<u64>(), any::<u32>()));
}

#[test]
fn wrapping_shl_u128() {
    test_binary_fn(u128::wrapping_shl, "wrapping_shl", (any::<u128>(), any::<u32>()));
}

#[test]
fn wrapping_shl_i8() {
    test_binary_fn(i8::wrapping_shl, "wrapping_shl", (any::<i8>(), any::<u32>()));
}

#[test]
fn wrapping_shl_i16() {
    test_binary_fn(i16::wrapping_shl, "wrapping_shl", (any::<i16>(), any::<u32>()));
}

#[test]
fn wrapping_shl_i32() {
    test_binary_fn(i32::wrapping_shl, "wrapping_shl", (any::<i32>(), any::<u32>()));
}

#[test]
fn wrapping_shl_i64() {
    test_binary_fn(i64::wrapping_shl, "wrapping_shl", (any::<i64>(), any::<u32>()));
}

#[test]
fn wrapping_shl_i128() {
    test_binary_fn(i128::wrapping_shl, "wrapping_shl", (any::<i128>(), any::<u32>()));
}

test_int_op!(shr, >>, i64, i64::MIN..=i64::MAX, 0u64..=63);
test_int_op!(shr, >>, u64, 0..=u64::MAX, 0u64..=63);
test_int_op!(shr, >>, u32, 0..u32::MAX, 0u32..32);
test_int_op!(shr, >>, u16, 0..u16::MAX, 0u32..16);
test_int_op!(shr, >>, u8, 0..u8::MAX, 0u32..8);
// # The following tests use small signed operands which we don't fully support yet
//test_int_op!(shr, >>, i8, i8::MIN..=i8::MAX, 0..=7);
//test_int_op!(shr, >>, i16, i16::MIN..=i16::MAX, 0..=15);
//test_int_op!(shr, >>, i32, i32::MIN..=i32::MAX, 0..=31);

#[test]
fn wrapping_shr_u8() {
    test_binary_fn(u8::wrapping_shr, "wrapping_shr", (any::<u8>(), any::<u32>()));
}

#[test]
fn wrapping_shr_u16() {
    test_binary_fn(u16::wrapping_shr, "wrapping_shr", (any::<u16>(), any::<u32>()));
}

#[test]
fn wrapping_shr_u32() {
    test_binary_fn(u32::wrapping_shr, "wrapping_shr", (any::<u32>(), any::<u32>()));
}

#[test]
fn wrapping_shr_u64() {
    test_binary_fn(u64::wrapping_shr, "wrapping_shr", (any::<u64>(), any::<u32>()));
}

#[test]
fn wrapping_shr_u128() {
    test_binary_fn(u128::wrapping_shr, "wrapping_shr", (any::<u128>(), any::<u32>()));
}

#[test]
fn wrapping_shr_i8() {
    test_binary_fn(i8::wrapping_shr, "wrapping_shr", (any::<i8>(), any::<u32>()));
}

#[test]
fn wrapping_shr_i16() {
    test_binary_fn(i16::wrapping_shr, "wrapping_shr", (any::<i16>(), any::<u32>()));
}

#[test]
fn wrapping_shr_i32() {
    test_binary_fn(i32::wrapping_shr, "wrapping_shr", (any::<i32>(), any::<u32>()));
}

#[test]
fn wrapping_shr_i64() {
    test_binary_fn(i64::wrapping_shr, "wrapping_shr", (any::<i64>(), any::<u32>()));
}

#[test]
fn wrapping_shr_i128() {
    test_binary_fn(i128::wrapping_shr, "wrapping_shr", (any::<i128>(), any::<u32>()));
}

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
