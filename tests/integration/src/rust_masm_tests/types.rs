use midenc_expect_test::expect_file;

use crate::CompilerTest;

#[test]
fn test_enum() {
    let _ = CompilerTest::rust_source_program(include_str!("types_src/enum.rs")).compile_package();
}

#[test]
fn test_array() {
    let _ = CompilerTest::rust_source_program(include_str!("types_src/array.rs")).compile_package();
}

#[test]
fn test_static_mut() {
    let _ = CompilerTest::rust_source_program(include_str!("types_src/static_mut.rs"))
        .compile_package();
}
