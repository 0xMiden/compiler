use expect_test::expect_file;

use crate::CompilerTest;

#[test]
fn test_enum() {
    let mut test = CompilerTest::rust_source_program(include_str!("types_src/enum.rs"));
    test.expect_wasm(expect_file!["../../expected/types/enum.wat"]);
    test.expect_ir(expect_file!["../../expected/types/enum.hir"]);
    // uncomment when https://github.com/0xPolygonMiden/compiler/issues/281 is fixed
    // test.expect_masm(expect_file!["../../expected/types/enum.masm"]);
}

#[test]
fn test_array() {
    let mut test = CompilerTest::rust_source_program(include_str!("types_src/array.rs"));
    test.expect_wasm(expect_file!["../../expected/types/array.wat"]);
    test.expect_ir(expect_file!["../../expected/types/array.hir"]);
    test.expect_masm(expect_file!["../../expected/types/array.masm"]);
}

#[test]
fn test_static_mut() {
    let mut test = CompilerTest::rust_source_program(include_str!("types_src/static_mut.rs"));
    test.expect_wasm(expect_file!["../../expected/types/static_mut.wat"]);
    test.expect_ir(expect_file!["../../expected/types/static_mut.hir"]);
    test.expect_masm(expect_file!["../../expected/types/static_mut.masm"]);
}
