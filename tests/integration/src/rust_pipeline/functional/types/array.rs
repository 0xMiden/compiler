use crate::CompilerTest;

#[test]
fn test_array() {
    let _ = CompilerTest::rust_source_program(include_str!("types_src/array.rs")).compile_package();
}
