use crate::CompilerTest;

#[test]
fn test_static_mut() {
    let _ = CompilerTest::rust_source_program(include_str!("types_src/static_mut.rs"))
        .compile_package();
}
