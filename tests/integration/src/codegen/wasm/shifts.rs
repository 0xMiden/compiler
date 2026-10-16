//! Tests for `i32` shifts by constant counts and of constant values, compiled from Wasm.
//!
//! A constant count below 32 is an immediate of the shift instruction rather than an operand on
//! the stack; a constant shifted value is still an operand.

use super::*;

/// Compile an entrypoint with the given `params`, whose body is `body`, returning an `i32`.
fn compile(params: &str, body: &str) -> (CompilerTest, Arc<Package>, String) {
    compile_wat(&format!(
        r#"(func $entrypoint (export "entrypoint") (param {params}) (result i32)
    {body})"#
    ))
}

/// Run `package` on the `i32` arguments `args`, the first on top of the operand stack.
fn run(test: &CompilerTest, package: &Arc<Package>, args: &[u32]) -> u32 {
    let args: Vec<Felt> = args.iter().map(|&arg| Felt::from(arg)).collect();
    eval_package::<u32, _, _>(package.clone(), None, &args, &test.session, |_| Ok(())).unwrap()
}

/// A constant shifted by a constant count is computed, the shifted value pushed as an operand.
#[test]
fn shl_of_a_constant_by_a_constant_count() {
    let (test, package, masm) = compile_wat(
        r#"(func $entrypoint (export "entrypoint") (result i32)
    i32.const 7
    i32.const 2
    i32.shl)"#,
    );
    // The shift is not folded: it reaches codegen with both operands constant.
    assert!(masm.contains("u32shl.2"), "{masm}");
    assert_eq!(run(&test, &package, &[]), 28, "{masm}");
}

/// A constant shifted by a variable count takes the count modulo 32.
#[test]
fn shl_of_a_constant_by_a_variable_count() {
    let (test, package, masm) = compile(
        "i32",
        "i32.const 7
    local.get 0
    i32.shl",
    );
    for count in [0u32, 1, 31, 33] {
        assert_eq!(
            run(&test, &package, &[count]),
            7u32.wrapping_shl(count),
            "7 << {count}\n{masm}"
        );
    }
}

/// A constant used both as a shift count and as an operand of another operation is right in
/// both uses.
#[test]
fn constant_shared_by_a_shift_count_and_an_add() {
    let (test, package, masm) = compile(
        "i32",
        "local.get 0
    i32.const 3
    i32.shl
    i32.const 3
    i32.add",
    );
    for arg in [0u32, 1, 5, 0x2000_0000, u32::MAX] {
        let expected = (arg << 3).wrapping_add(3);
        assert_eq!(run(&test, &package, &[arg]), expected, "({arg} << 3) + 3\n{masm}");
    }
}

/// A logical right shift by a constant count lowers to the immediate form, without masking the
/// count at run time.
#[test]
fn shr_u_by_a_constant_count_uses_the_immediate_form() {
    let (test, package, masm) = compile(
        "i32",
        "local.get 0
    i32.const 3
    i32.shr_u",
    );
    assert!(masm.contains("u32shr.3"), "{masm}");
    assert!(!masm.contains("u32and"), "{masm}");
    for arg in [0u32, 7, 8, 0x1234_5678, 0x8000_0000, u32::MAX] {
        assert_eq!(run(&test, &package, &[arg]), arg / 8, "{arg} >> 3\n{masm}");
    }
}

/// A left shift by a constant count of 32 or more takes the count modulo 32.
#[test]
fn shl_by_a_constant_count_of_32_or_more() {
    for (count, effective) in [(32u32, 0u32), (33, 1)] {
        let (test, package, masm) = compile(
            "i32",
            &format!(
                "local.get 0
    i32.const {count}
    i32.shl"
            ),
        );
        for arg in [0u32, 1, 0x1234_5678, 0x8000_0001, u32::MAX] {
            assert_eq!(run(&test, &package, &[arg]), arg << effective, "{arg} << {count}\n{masm}");
        }
    }
}

/// A logical right shift by a variable count takes the count modulo 32.
#[test]
fn shr_u_by_a_variable_count_masks_the_count() {
    let (test, package, masm) = compile(
        "i32 i32",
        "local.get 0
    local.get 1
    i32.shr_u",
    );
    for (value, count) in
        [(0xf0u32, 35u32), (0xf0, 3), (u32::MAX, 0), (u32::MAX, 31), (u32::MAX, 32)]
    {
        assert_eq!(
            run(&test, &package, &[value, count]),
            value.wrapping_shr(count),
            "{value} >> {count}\n{masm}"
        );
    }
}
