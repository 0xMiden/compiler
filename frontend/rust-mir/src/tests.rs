//! Golden tests of the Rust MIR frontend.

use core::fmt::Write;
use std::{path::PathBuf, rc::Rc};

use midenc_dialect_hir::transforms::Local2Reg;
use midenc_expect_test::{ExpectFile, expect_file};
use midenc_hir::{
    Context, Op, Operation, WalkResult,
    dialects::builtin::{self, Function},
    pass::{Nesting, PassManager},
};
use midenc_hir_eval::{HirEvaluator, Value};

use crate::{RustMirTranslationConfig, translate};

/// Returns the path of a fixture file.
fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures").join(name)
}

/// Translates a fixture and returns the component it produced.
fn translate_fixture(name: &str, context: Rc<Context>) -> builtin::ComponentRef {
    let config = RustMirTranslationConfig::default();
    translate(&fixture(name), &config, context)
        .map_err(|err| midenc_session::diagnostics::PrintDiagnostic::new(err).to_string())
        .unwrap()
        .component
}

/// Collects every function of a component.
fn functions(component: &builtin::ComponentRef) -> Vec<builtin::FunctionRef> {
    let mut found = Vec::new();
    component
        .borrow()
        .as_operation()
        .prewalk(|op: &Operation| match op.downcast_ref::<Function>() {
            Some(function) => {
                found.push(function.as_function_ref());
                WalkResult::<()>::Skip
            }
            None => WalkResult::Continue(()),
        })
        .into_result()
        .unwrap();
    found
}

/// Prints every function of a component.
fn print_functions(component: &builtin::ComponentRef) -> String {
    let mut output = String::new();
    for function in functions(component) {
        writeln!(&mut output, "{}", function.borrow().as_operation()).unwrap();
    }
    output
}

/// Applies the `Local2Reg` pass to every function of a component.
fn apply_local2reg(component: &builtin::ComponentRef, context: &Rc<Context>) {
    for function in functions(component) {
        let mut pass_manager = PassManager::on::<Function>(context.clone(), Nesting::Implicit);
        pass_manager.add_pass(Box::new(Local2Reg));
        pass_manager.enable_verifier(true);
        pass_manager.run(function.as_operation_ref()).expect("invalid ir");
    }
}

/// Translates a fixture and compares the printed functions with an expect file.
fn check_fixture(name: &str, run_local2reg: bool, expected: ExpectFile) {
    let context = Rc::new(Context::default());
    let component = translate_fixture(name, context.clone());

    if run_local2reg {
        apply_local2reg(&component, &context);
    }

    expected.assert_eq(&print_functions(&component));
}

#[test]
fn add_raw() {
    check_fixture("add.rs", false, expect_file!["expected/add.hir"]);
}

#[test]
fn add_local2reg() {
    check_fixture("add.rs", true, expect_file!["expected/add_local2reg.hir"]);
}

/// Evaluates the translated `add` function with the HIR interpreter.
///
/// The interpreter receives 1 and 2 and returns the single result of the function.
fn eval_add(run_local2reg: bool) -> Value {
    let context = Rc::new(Context::default());
    let component = translate_fixture("add.rs", context.clone());

    if run_local2reg {
        apply_local2reg(&component, &context);
    }

    let function = functions(&component).pop().expect("the fixture has one function");
    let mut evaluator = HirEvaluator::new(context);
    let callable = function.borrow();
    let results = evaluator
        .eval_callable(&*callable, [1u32.into(), 2u32.into()])
        .map_err(|err| midenc_session::diagnostics::PrintDiagnostic::new(err).to_string())
        .unwrap();

    assert_eq!(results.len(), 1, "add must return one result");
    results[0]
}

#[test]
fn add_evaluates_raw() {
    assert_eq!(eval_add(false), Value::Immediate(3u32.into()));
}

#[test]
fn add_evaluates_after_local2reg() {
    assert_eq!(eval_add(true), Value::Immediate(3u32.into()));
}

#[test]
fn unsupported_parameter_type() {
    let context = Rc::new(Context::default());
    let error =
        translate(&fixture("unsupported_type.rs"), &RustMirTranslationConfig::default(), context)
            .err()
            .expect("translation of an f64 parameter must fail");

    let message = error.to_string();
    assert!(
        message.contains("unsupported type in MIR") && message.contains("Float(F64)"),
        "the error must name the unsupported type, got: {message}"
    );
}
