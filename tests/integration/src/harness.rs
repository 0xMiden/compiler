//! Regression tests for the compiler test harness's artifact and lifetime contracts.

use std::{panic::AssertUnwindSafe, rc::Rc, sync::Arc};

use midenc_hir::{FunctionIdent, Ident, Op, interner::Symbol};

use crate::{CompilerTest, CompilerTestBuilder};

fn fixture() -> CompilerTest {
    let wasm = wat::parse_str(
        r#"(module (func $entrypoint (export "entrypoint") (result i32) i32.const 42))"#,
    )
    .unwrap();
    let mut builder = CompilerTestBuilder::from_wasm("capture_test", wasm, []);
    builder.with_entrypoint(FunctionIdent {
        module: Ident::with_empty_span(Symbol::intern("capture_test")),
        function: Ident::with_empty_span(Symbol::intern("entrypoint")),
    });
    builder.build()
}

#[test]
fn package_only_does_not_capture_intermediate_artifacts() {
    let mut test = fixture();
    let package = test.compile_package();
    assert!(Arc::ptr_eq(&package, &test.compile_package()));
    assert!(std::panic::catch_unwind(AssertUnwindSafe(|| test.hir())).is_err());
    assert!(std::panic::catch_unwind(AssertUnwindSafe(|| test.masm_src())).is_err());
}

#[test]
fn requested_hir_survives_package_compilation() {
    let mut test = fixture();
    let hir = test.hir();
    let context = Rc::downgrade(&hir.borrow().as_operation().context_rc());
    let package = test.compile_package();
    assert!(context.upgrade().is_some());
    assert!(!hir.borrow().as_operation().to_string().is_empty());
    assert!(hir == test.hir());
    assert!(Arc::ptr_eq(&package, &test.compile_package()));
    drop(test);
    assert!(context.upgrade().is_none(), "the harness must release its captured HIR arena");
}

#[test]
fn requested_masm_survives_package_compilation() {
    let mut test = fixture();
    let masm = test.masm_src();
    assert!(!masm.is_empty());
    let package = test.compile_package();
    assert_eq!(masm, test.masm_src());
    assert!(Arc::ptr_eq(&package, &test.compile_package()));
}
