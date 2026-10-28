//! Tests for `i32` shifts and rotates by constant counts and of constant values, compiled from
//! Wasm.
//!
//! A constant count below 32 is an immediate of the shift or rotate instruction rather than an
//! operand on the stack; a constant shifted value is still an operand.

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

/// Whether `masm` has a line consisting of exactly the instruction `instruction`.
fn has_instruction(masm: &str, instruction: &str) -> bool {
    masm.lines().any(|line| line.trim() == instruction)
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
    assert!(masm.contains("push.7"), "{masm}");
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
    assert!(masm.contains("u32shl.3"), "{masm}");
    for arg in [0u32, 1, 5, 0x2000_0000, u32::MAX] {
        let expected = (arg << 3).wrapping_add(3);
        assert_eq!(run(&test, &package, &[arg]), expected, "({arg} << 3) + 3\n{masm}");
    }
}

/// A logical right shift by a constant count lowers to the immediate form, without masking the
/// count at run time or pushing it.
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
    assert!(!has_instruction(&masm, "push.3"), "{masm}");
    for arg in [0u32, 7, 8, 0x1234_5678, 0x8000_0000, u32::MAX] {
        assert_eq!(run(&test, &package, &[arg]), arg / 8, "{arg} >> 3\n{masm}");
    }
}

/// A constant shifted right by a constant count is computed, the shifted value pushed as an
/// operand.
#[test]
fn shr_u_of_a_constant_by_a_constant_count() {
    let (test, package, masm) = compile_wat(
        r#"(func $entrypoint (export "entrypoint") (result i32)
    i32.const 0x80
    i32.const 4
    i32.shr_u)"#,
    );
    assert!(masm.contains("u32shr.4"), "{masm}");
    assert!(masm.contains("push.128"), "{masm}");
    assert_eq!(run(&test, &package, &[]), 8, "{masm}");
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
        // The frontend reduces a constant count modulo 32, so the immediate is the reduced count.
        assert!(masm.contains(&format!("u32shl.{effective}")), "{count}\n{masm}");
        for arg in [0u32, 1, 0x1234_5678, 0x8000_0001, u32::MAX] {
            assert_eq!(run(&test, &package, &[arg]), arg << effective, "{arg} << {count}\n{masm}");
        }
    }
}

/// A left shift by a variable count takes the count modulo 32.
#[test]
fn shl_by_a_variable_count_masks_the_count() {
    let (test, package, masm) = compile(
        "i32 i32",
        "local.get 0
    local.get 1
    i32.shl",
    );
    // The count is an operand on the stack, not an immediate.
    assert!(has_instruction(&masm, "u32shl"), "{masm}");
    for (value, count) in
        [(0xf0u32, 35u32), (0xf0, 3), (u32::MAX, 0), (u32::MAX, 31), (u32::MAX, 32)]
    {
        assert_eq!(
            run(&test, &package, &[value, count]),
            value.wrapping_shl(count),
            "{value} << {count}\n{masm}"
        );
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

/// A left shift by a constant count inside a loop body uses the immediate form on every
/// iteration.
#[test]
fn shl_in_a_loop_by_a_constant_count() {
    // Doubles the accumulator, starting at 1, as many times as the first parameter says.
    let (test, package, masm) = compile(
        "i32",
        "(local i32)
    i32.const 1
    local.set 1
    block
      local.get 0
      i32.eqz
      br_if 0
      loop
        local.get 1
        i32.const 1
        i32.shl
        local.set 1
        local.get 0
        i32.const 1
        i32.sub
        local.tee 0
        br_if 0
      end
    end
    local.get 1",
    );
    assert!(masm.contains("u32shl.1"), "{masm}");
    for n in [0u32, 1, 5, 31] {
        assert_eq!(run(&test, &package, &[n]), 1u32.wrapping_shl(n), "1 << {n}\n{masm}");
    }
}

/// Rotates by a constant count lower to the immediate forms, without pushing the count.
#[test]
fn rotates_by_a_constant_count_use_the_immediate_forms() {
    for (op, instruction, rotate) in [
        ("rotl", "u32rotl.5", u32::rotate_left as fn(u32, u32) -> u32),
        ("rotr", "u32rotr.5", u32::rotate_right),
    ] {
        let (test, package, masm) = compile(
            "i32",
            &format!(
                "local.get 0
    i32.const 5
    i32.{op}"
            ),
        );
        assert!(masm.contains(instruction), "{masm}");
        assert!(!has_instruction(&masm, "push.5"), "{masm}");
        for arg in [0u32, 1, 0x8000_0000, 0x1234_5678, u32::MAX] {
            assert_eq!(run(&test, &package, &[arg]), rotate(arg, 5), "{arg} {op} 5\n{masm}");
        }
    }
}

/// A rotate by a constant count of 32 or more takes the count modulo 32.
#[test]
fn rotl_by_a_constant_count_of_32_or_more() {
    for (count, effective) in [(32u32, 0u32), (35, 3)] {
        let (test, package, masm) = compile(
            "i32",
            &format!(
                "local.get 0
    i32.const {count}
    i32.rotl"
            ),
        );
        assert!(masm.contains(&format!("u32rotl.{effective}")), "{count}\n{masm}");
        for arg in [0u32, 1, 0x8000_0000, 0x1234_5678, u32::MAX] {
            assert_eq!(
                run(&test, &package, &[arg]),
                arg.rotate_left(effective),
                "{arg} rotl {count}\n{masm}"
            );
        }
    }
}
