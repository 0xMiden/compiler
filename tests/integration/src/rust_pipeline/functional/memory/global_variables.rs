use crate::CompilerTest;

#[test]
fn global_variable_static_mut() {
    let _ = CompilerTest::rust_source_program(include_str!(
        "../../../../../fixtures/standalone/static_mut.rs"
    ))
    .compile_package();
}
