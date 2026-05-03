use crate::CompilerTest;

#[test]
fn test_enum() {
    let _ = CompilerTest::rust_source_program(include_str!("types_src/enum.rs")).compile_package();
}
