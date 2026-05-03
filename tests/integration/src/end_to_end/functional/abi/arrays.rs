use crate::CompilerTest;

#[test]
fn arrays_and_slices() {
    let _ = CompilerTest::rust_source_program(include_str!(
        "../../../../../fixtures/standalone/array.rs"
    ))
    .compile_package();
}
