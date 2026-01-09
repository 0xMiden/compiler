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
