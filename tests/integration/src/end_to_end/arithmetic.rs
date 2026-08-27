use std::any::type_name;

use miden_core::Felt;
use miden_debug::{DebugQuery, FromMidenRepr, ToMidenRepr, push_wasm_ty_to_operand_stack};
use num_traits::{PrimInt, ToBytes};
use proptest::{
    prelude::*,
    test_runner::{TestError, TestRunner},
};

use super::support::NumericStrategy;
use crate::{
    CompilerTest,
    testing::{eval_package, run_masm_vs_rust},
};

macro_rules! push_wasm_test_arg {
    // `bool` already uses its Wasm ABI representation and is not a `PrimInt`.
    ($value:expr, bool, $stack:expr) => {
        $value.push_to_operand_stack($stack)
    };

    ($value:expr, $ty:tt, $stack:expr) => {
        push_wasm_ty_to_operand_stack($value, $stack)
    };
}

macro_rules! test_bin_op {
    ($name:ident, $op:tt, $op_ty:tt, $res_ty:tt, $a_range:expr, $b_range:expr) => {
        test_bin_op!($name, $op, $op_ty, $op_ty, $res_ty, $a_range, $b_range);
    };

    ($name:ident, $op:tt, $a_ty:tt, $b_ty:tt, $res_ty:tt, $a_range:expr, $b_range:expr) => {
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
                        push_wasm_test_arg!(a, $a_ty, &mut args);
                        push_wasm_test_arg!(b, $b_ty, &mut args);
                        run_masm_vs_rust(rs_out, package.clone(), &args, &test.session)
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

                    eval_package::<Felt, _, _>(package.clone(), None, &args, &test.session, |trace| {
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
                        run_masm_vs_rust(rs_out, package.clone(), &args, &test.session)
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
                        run_masm_vs_rust(rust_out, package.clone(), &args, &test.session)
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
    ($name:ident, $op:tt, $op_ty:tt, $a_range:expr, $b_range:expr) => {
        test_bin_op!($name, $op, $op_ty, $op_ty, $a_range, $b_range);
    };

    ($name:ident, $op:tt, $a_ty:tt, $b_ty:tt, $a_range:expr, $b_range:expr) => {
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

macro_rules! test_saturating_arith {
    ($fn_name:ident, $strategy:ident, $($(#[$a:meta])* $op_ty:ty),+ $(,)?) => {
        $(
            concat_idents::concat_idents!(test_name = $fn_name, _, $op_ty {
                #[test]
                $(#[$a])*
                fn test_name() {
                    test_binary_fn(
                        $op_ty::$fn_name,
                        stringify!($fn_name),
                        NumericStrategy::<$op_ty>::$strategy(),
                    );
                }
            });
        )+
    };
}

macro_rules! test_overflowing_arith {
    ($fn_name:ident, $strategy:ident, $($(#[$a:meta])* $op_ty:ty),+ $(,)?) => {
        $(
            concat_idents::concat_idents!(test_name = $fn_name, _, $op_ty {
                #[test]
                $(#[$a])*
                fn test_name() {
                    test_overflowing_arith(
                        $op_ty::$fn_name,
                        stringify!($fn_name),
                        NumericStrategy::<$op_ty>::$strategy(),
                    );
                }
            });
        )+
    };
}

macro_rules! test_checked_arith {
    ($fn_name:ident, $strategy:ident, $($(#[$a:meta])* $op_ty:ty),+ $(,)?) => {
        $(
            concat_idents::concat_idents!(test_name = $fn_name, _, $op_ty {
                #[test]
                $(#[$a])*
                fn test_name() {
                    test_checked_arith(
                        $op_ty::$fn_name,
                        stringify!($fn_name),
                        NumericStrategy::<$op_ty>::$strategy(),
                    );
                }
            });
        )+
    };
}

macro_rules! test_shift {
    ($fn_name:ident, $($(#[$a:meta])* $op_ty:ty),+ $(,)?) => {
        $(
            concat_idents::concat_idents!(test_name = $fn_name, _, $op_ty {
                #[test]
                $(#[$a])*
                fn test_name() {
                    test_binary_fn(
                        $op_ty::$fn_name,
                        stringify!($fn_name),
                        (any::<$op_ty>(), any::<u32>())
                    );
                }
            });
        )+
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
test_int_op!(mul, *, i16, -181i16..=181, -181i16..=181);
test_int_op!(mul, *, i8, -11i8..=11, -11i8..=11);

const MAX_U128_64: u128 = u64::MAX as u128;
const MAX_I128_64: i128 = i64::MAX as i128;
const MIN_I128_64: i128 = i64::MIN as i128;

test_wide_bin_op!(mul, *, u128, u128, 0..=MAX_U128_64, 0..=MAX_U128_64);
test_wide_bin_op!(mul, *, i128, i128, MIN_I128_64..MAX_I128_64, MIN_I128_64..=MAX_I128_64);

test_int_op!(div, /, u64, 0..=u64::MAX, 1..=u64::MAX);
test_int_op!(div, /, i64, i64::MIN..=i64::MAX, 1..=i64::MAX);
test_int_op!(div, /, u32, 0..=u32::MAX, 1..=u32::MAX);
test_int_op!(div, /, i32, i32::MIN..=i32::MAX, 1..=i32::MAX);
test_int_op!(div, /, u16, 0..=u16::MAX, 1..=u16::MAX);
test_int_op!(div, /, i16, i16::MIN..=i16::MAX, 1..=i16::MAX);
test_int_op!(div, /, u8, 0..=u8::MAX, 1..=u8::MAX);
test_int_op!(div, /, i8, i8::MIN..=i8::MAX, 1..=i8::MAX);
test_wide_bin_op!(div, /, u128, u128, 0..=u128::MAX, 1..=u128::MAX);
test_wide_bin_op!(div, /, i128, i128, i128::MIN..=i128::MAX, 1..=i128::MAX);

test_int_op!(rem, %, u64, 0..=u64::MAX, 1..=u64::MAX);
// https://github.com/0xMiden/compiler/issues/1285
// test_int_op!(rem, %, i64, i64::MIN..=i64::MAX, 1..=i64::MAX);
test_int_op!(rem, %, u32, 0..=u32::MAX, 1..=u32::MAX);
test_int_op!(rem, %, i32, i32::MIN..=i32::MAX, 1..=i32::MAX);
test_int_op!(rem, %, u16, 0..=u16::MAX, 1..=u16::MAX);
test_int_op!(rem, %, i16, i16::MIN..=i16::MAX, 1..=i16::MAX);
test_int_op!(rem, %, u8, 0..=u8::MAX, 1..=u8::MAX);
test_int_op!(rem, %, i8, i8::MIN..=i8::MAX, 1..=i8::MAX);
test_wide_bin_op!(rem, %, u128, u128, 0..=u128::MAX, 1..=u128::MAX);
test_wide_bin_op!(rem, %, i128, i128, i128::MIN..=i128::MAX, 1..=i128::MAX);

test_unary_op!(neg, -, i64, (i64::MIN + 1)..=i64::MAX);

// Comparison ops

// enable when https://github.com/0xMiden/compiler/issues/56 is fixed
test_func_two_arg!(min, core::cmp::min, i32, i32, i32);
test_func_two_arg!(min, core::cmp::min, u32, u32, u32);
test_func_two_arg!(min, core::cmp::min, u8, u8, u8);
test_func_two_arg!(max, core::cmp::max, u8, u8, u8);

test_overflowing_arith!(overflowing_add, add_unsigned, u8, u16, u32, u64, u128);
test_overflowing_arith!(overflowing_add, add_signed, i8, i16, i32, i64, i128);
test_overflowing_arith!(overflowing_sub, sub_unsigned, u8, u16, u32, u64, u128);
test_overflowing_arith!(overflowing_sub, sub_signed, i8, i16, i32, i64, i128);
test_overflowing_arith!(overflowing_mul, mul_unsigned, u8, u16, u32, u64, u128);
test_overflowing_arith!(overflowing_mul, mul_signed, i8, i16, i32, i128);
test_overflowing_arith!(overflowing_div, div_unsigned_overflowing, u8, u16, u32, u64, u128);
test_overflowing_arith!(overflowing_div, div_signed_overflowing, i8, i16, i32, i64, i128);
test_overflowing_arith!(overflowing_rem, rem_unsigned_overflowing, u8, u16, u32, u64, u128);
test_overflowing_arith!(
    overflowing_rem,
    rem_signed_overflowing,
    i8,
    i16,
    i32,
    #[ignore = "https://github.com/0xMiden/compiler/issues/1000"]
    i64,
    i128
);

test_checked_arith!(checked_add, add_unsigned, u8, u16, u32, u64, u128);
test_checked_arith!(checked_add, add_signed, i8, i16, i32, i64, i128);
test_checked_arith!(checked_sub, sub_unsigned, u8, u16, u32, u64, u128);
test_checked_arith!(checked_sub, sub_signed, i8, i16, i32, i64, i128);
test_checked_arith!(checked_mul, mul_unsigned, u8, u16, u32, u64, u128);
test_checked_arith!(
    checked_mul,
    mul_signed,
    i8,
    i16,
    i32,
    #[ignore = "https://github.com/0xMiden/compiler/issues/1144"]
    i64,
    i128
);
test_checked_arith!(checked_div, div_unsigned_checked, u8, u16, u32, u64, u128);
test_checked_arith!(checked_div, div_signed_checked, i8, i16, i32, i64, i128);
test_checked_arith!(checked_rem, rem_unsigned_checked, u8, u16, u32, u64, u128);
test_checked_arith!(
    checked_rem,
    rem_signed_checked,
    i8,
    i16,
    i32,
    #[ignore = "https://github.com/0xMiden/compiler/issues/1000"]
    i64,
    i128
);

test_saturating_arith!(
    saturating_add,
    add_unsigned,
    u8,
    u16,
    u32,
    u64,
    #[ignore = "https://github.com/0xMiden/compiler/issues/1355"]
    u128
);
test_saturating_arith!(saturating_add, add_signed, i8, i16, i32, i64, i128);
test_saturating_arith!(
    saturating_sub,
    sub_unsigned,
    u8,
    u16,
    u32,
    u64,
    #[ignore = "https://github.com/0xMiden/compiler/issues/1355"]
    u128
);
test_saturating_arith!(saturating_sub, sub_signed, i8, i16, i32, i64, i128);
test_saturating_arith!(saturating_mul, mul_unsigned, u8, u16, u32, u64, u128);
test_saturating_arith!(
    saturating_mul,
    mul_signed,
    i8,
    i16,
    i32,
    #[ignore = "https://github.com/0xMiden/compiler/issues/1144"]
    i64,
    i128
);
test_saturating_arith!(saturating_div, div_unsigned_overflowing, u8, u16, u32, u64, u128);
test_saturating_arith!(saturating_div, div_signed_overflowing, i8, i16, i32, i64, i128);

fn test_overflowing_arith<T>(
    op: fn(T, T) -> (T, bool),
    fn_name: &str,
    strategy: impl Strategy<Value = (T, T)>,
) where
    T: ToBytes + ToMidenRepr + FromMidenRepr + PrimInt + Arbitrary + 'static,
{
    // The return value of `type_name` isn't stable, but it's good enough for this test.
    let ty_name = type_name::<T>();
    let main_fn = format!(
        r#"(a: {ty_name}, b: {ty_name}, addr: *mut {ty_name}) -> bool {{
        let (value, flag) = a.{fn_name}(b);
        unsafe {{ *addr = value; }}
        flag
    }}"#
    );
    let mut test = CompilerTest::rust_fn_body(&main_fn, None);
    let package = test.compile_package();

    let res = NumericStrategy::<T>::test_runner().run(&strategy, move |(a, b)| {
        let rust_out = op(a, b);

        // Write the operation result to 20 * PAGE_SIZE.
        let out_addr = 20u32 * 65536;

        let mut args = Vec::<midenc_hir::Felt>::default();
        push_wasm_ty_to_operand_stack(a, &mut args);
        push_wasm_ty_to_operand_stack(b, &mut args);
        out_addr.push_to_operand_stack(&mut args);

        eval_package::<Felt, _, _>(package.clone(), None, &args, &test.session, |trace| {
            let success = trace
                .parse_result::<bool>()
                .expect("expected a boolean value on the operand stack");
            prop_assert_eq!(
                success,
                rust_out.1,
                "the Miden VM and Rust disagree on the outcome of this operation"
            );
            let x = trace
                .read_from_rust_memory::<T>(out_addr)
                .expect("expected valid value of input type");
            prop_assert_eq!(
                x,
                rust_out.0,
                "the Miden VM and Rust disagree on the value produced by this operation"
            );
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
    T: ToBytes + ToMidenRepr + FromMidenRepr + PrimInt + Arbitrary + 'static,
{
    // The return value of `type_name` isn't stable, but it's good enough for this test.
    let ty_name = type_name::<T>();
    let source_code = format!(
        r#"
#![no_std]
#![no_main]
#![feature(alloc_error_handler)]

#[panic_handler]
fn my_panic(_info: &core::panic::PanicInfo) -> ! {{
    core::arch::wasm32::unreachable()
}}

#[alloc_error_handler]
fn my_alloc_error(_info: core::alloc::Layout) -> ! {{
    core::arch::wasm32::unreachable()
}}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(a: {ty_name}, b: {ty_name}, addr: *mut {ty_name}) -> bool {{
    match a.{fn_name}(b) {{
        Some(value) => {{
            unsafe {{ *addr = value; }}
            true
        }}
        None => false,
    }}
}}
"#
    );
    let mut test = CompilerTest::rust_source_program_with_entrypoint(source_code, "entrypoint");
    let package = test.compile_package();

    let res = NumericStrategy::<T>::test_runner().run(&strategy, move |(a, b)| {
        let rust_out = match op(a, b) {
            Some(value) => (value, true),
            None => (T::zero(), false),
        };

        // Write the operation result to 20 * PAGE_SIZE.
        let out_addr = 20u32 * 65536;

        let mut args = Vec::<midenc_hir::Felt>::default();
        push_wasm_ty_to_operand_stack(a, &mut args);
        push_wasm_ty_to_operand_stack(b, &mut args);
        out_addr.push_to_operand_stack(&mut args);

        eval_package::<Felt, _, _>(package.clone(), None, &args, &test.session, |trace| {
            let success = trace
                .parse_result::<bool>()
                .expect("expected a boolean value on the operand stack");
            prop_assert_eq!(
                success,
                rust_out.1,
                "the Miden VM and Rust disagree on the outcome of this operation"
            );
            if success {
                let x = trace
                    .read_from_rust_memory::<T>(out_addr)
                    .expect("expected valid value of input type");
                prop_assert_eq!(
                    x,
                    rust_out.0,
                    "the Miden VM and Rust disagree on the value produced by this operation"
                );
            }
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
    T: ToBytes + ToMidenRepr + FromMidenRepr + PrimInt + Arbitrary + std::fmt::Debug + 'static,
    U: ToMidenRepr + PrimInt + Arbitrary,
{
    // The return value of `type_name` isn't stable, but it's good enough for this test.
    let lhs_ty_name = type_name::<T>();
    let rhs_ty_name = type_name::<U>();

    // Write the result to memory to handle all integer widths with one `main_fn`.
    // If the result were to be returned, it would be written to memory for 128 bit wide ints
    // and returned on the stack for smaller ints.
    let main_fn = format!(
        r#"(a: {lhs_ty_name}, b: {rhs_ty_name}, addr: *mut {lhs_ty_name}) {{
        unsafe {{ *addr = a.{fn_name}(b); }}
    }}"#
    );
    let mut test = CompilerTest::rust_fn_body(&main_fn, None);
    let package = test.compile_package();

    let res = TestRunner::default().run(&strategy, move |(a, b)| {
        let rust_out = op(a, b);

        // Write the operation result to 20 * PAGE_SIZE.
        let out_addr = 20u32 * 65536;
        let mut args = Vec::<midenc_hir::Felt>::default();
        push_wasm_ty_to_operand_stack(a, &mut args);
        push_wasm_ty_to_operand_stack(b, &mut args);
        out_addr.push_to_operand_stack(&mut args);

        eval_package::<u32, _, _>(package.clone(), None, &args, &test.session, |trace| {
            let x = trace
                .read_from_rust_memory::<T>(out_addr)
                .expect("expected valid value of input type");
            prop_assert_eq!(
                x,
                rust_out,
                "the Miden VM and Rust disagree on the value produced by this operation"
            );
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

test_shift!(wrapping_shl, u8, i8, u16, i16, u32, i32, u64, i64, u128, i128);
test_shift!(wrapping_shr, u8, i8, u16, i16, u32, i32, u64, i64, u128, i128);

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
