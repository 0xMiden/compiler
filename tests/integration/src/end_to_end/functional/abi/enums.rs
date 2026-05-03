use crate::CompilerTest;

#[test]
fn enum_matching() {
    let _ = CompilerTest::rust_source_program(include_str!(
        "../../../../../fixtures/standalone/enum.rs"
    ))
    .compile_package();
}
