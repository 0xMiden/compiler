//! Regression test for https://github.com/0xMiden/compiler/issues/1243.
//!
//! LLVM normalizes a `switch` selector by subtracting the smallest case value with wrapping
//! arithmetic, relying on `br_table`'s unsigned out-of-range -> default rule. The frontend used a
//! checked I32 -> U32 cast for the selector, which the MASM backend lowers to an assertion that
//! rejects any value with the high bit set (e.g. `0 - 1 = 0xFFFFFFFF`), trapping with
//! "value does not fit in i32" instead of branching to the default arm.

use miden_core::Felt;
use midenc_frontend_wasm::WasmTranslationConfig;

use crate::{
    CompilerTest,
    testing::{eval_package, setup},
};

/// A `br_table` selector that is out of range as an unsigned value must branch to the default arm,
/// whether it got there by wrapping subtraction (`0 - 1`) or already had its high bit set.
#[test]
fn br_table_default_arm_for_out_of_range_selector() {
    // The panicking `assert!` arm plus `black_box` is what makes LLVM at `opt-level = "z"` merge
    // the two tests into a `switch` (lowered to `br_table`) with selector `v - 1`.
    let main_fn = r#"(x: u32) -> u32 {
        let v = core::hint::black_box(x);
        assert!(v != 1, "must not be one");
        if v != 2 { 10 } else { 20 }
    }"#;

    setup::enable_compiler_instrumentation();
    let config = WasmTranslationConfig::default();
    let mut test = CompilerTest::rust_fn_body_with_stdlib_sys("issue1243", main_fn, config, []);

    let package = test.compile_package();

    // `v = 0` makes the normalized selector wrap to `0xFFFFFFFF`, which must take the default arm.
    let out =
        eval_package::<u32, _, _>(package.clone(), [], &[Felt::from(0u32)], &test.session, |_| {
            Ok(())
        })
        .unwrap();
    assert_eq!(out, 10);

    // A selector with the high bit set without wrapping must also take the default arm.
    let out = eval_package::<u32, _, _>(
        package,
        [],
        &[Felt::from(0x9000_0000u32)],
        &test.session,
        |_| Ok(()),
    )
    .unwrap();
    assert_eq!(out, 10);
}
