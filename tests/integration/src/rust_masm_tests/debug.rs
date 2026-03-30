use std::borrow::Cow;

use midenc_expect_test::expect_file;

use crate::{CompilerTestBuilder, testing::setup};

#[test]
fn variable_locations_schedule() {
    setup::enable_compiler_instrumentation();

    let source = r#"
        (n: u32) -> u32 {
            let mut sum = 0u32;
            let mut i = 0u32;
            while i <= n {
                sum += i;
                i += 1;
            }
            sum
        }
    "#;

    let mut builder = CompilerTestBuilder::rust_fn_body(source, []);
    builder.with_rustflags([Cow::Borrowed("-C"), Cow::Borrowed("debuginfo=2")]);
    let mut test = builder.build();
    test.expect_ir_unoptimized(expect_file!["../../expected/debug_variable_locations.hir"]);
}

#[test]
fn debug_simple_params() {
    setup::enable_compiler_instrumentation();

    let source = r#"
        (a: u32, b: u32) -> u32 {
            a + b
        }
    "#;

    let mut builder = CompilerTestBuilder::rust_fn_body(source, []);
    builder.with_rustflags([Cow::Borrowed("-C"), Cow::Borrowed("debuginfo=2")]);
    let mut test = builder.build();
    test.expect_ir_unoptimized(expect_file!["../../expected/debug_simple_params.hir"]);
}

#[test]
fn debug_conditional_assignment() {
    setup::enable_compiler_instrumentation();

    let source = r#"
        (x: u32) -> u32 {
            let result = if x > 10 { x * 2 } else { x + 1 };
            result
        }
    "#;

    let mut builder = CompilerTestBuilder::rust_fn_body(source, []);
    builder.with_rustflags([Cow::Borrowed("-C"), Cow::Borrowed("debuginfo=2")]);
    let mut test = builder.build();
    test.expect_ir_unoptimized(expect_file!["../../expected/debug_conditional_assignment.hir"]);
}

#[test]
fn debug_multiple_locals() {
    setup::enable_compiler_instrumentation();

    let source = r#"
        (n: u32) -> u32 {
            let a: u32 = n + 1;
            let b: u32 = n * 2;
            let c: u32 = a + b;
            c
        }
    "#;

    let mut builder = CompilerTestBuilder::rust_fn_body(source, []);
    builder.with_rustflags([Cow::Borrowed("-C"), Cow::Borrowed("debuginfo=2")]);
    let mut test = builder.build();
    test.expect_ir_unoptimized(expect_file!["../../expected/debug_multiple_locals.hir"]);
}

#[test]
fn debug_nested_loops() {
    setup::enable_compiler_instrumentation();

    let source = r#"
        (n: u32) -> u32 {
            let mut total = 0u32;
            let mut i = 0u32;
            while i < n {
                let mut j = 0u32;
                while j < i {
                    total += 1;
                    j += 1;
                }
                i += 1;
            }
            total
        }
    "#;

    let mut builder = CompilerTestBuilder::rust_fn_body(source, []);
    builder.with_rustflags([Cow::Borrowed("-C"), Cow::Borrowed("debuginfo=2")]);
    let mut test = builder.build();
    test.expect_ir_unoptimized(expect_file!["../../expected/debug_nested_loops.hir"]);
}
