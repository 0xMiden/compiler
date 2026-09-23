use core::ops::{Deref, DerefMut};

use midenc_dialect_arith::ArithOpBuilder;
use midenc_dialect_cf::ControlFlowOpBuilder;
use midenc_dialect_hir::HirOpBuilder;
use midenc_dialect_scf::StructuredControlFlowOpBuilder;
use midenc_dialect_wasm::WasmOpBuilder;
use midenc_hir::{
    Builder, Op, PointerType, Report, SourceSpan, SymbolName, SymbolTable, Type,
    UnsafeIntrusiveEntityRef, ValueRef,
    diagnostics::Uri,
    dialects::builtin::{BuiltinOpBuilder, FunctionBuilder, Module},
    parse::{ParserConfig, parse},
    testing::Test,
};

use crate::*;

struct EvalTest {
    test: Test,
    evaluator: HirEvaluator,
}

impl Default for EvalTest {
    fn default() -> Self {
        let test = Test::default();
        let evaluator = HirEvaluator::new(test.context_rc());
        Self { test, evaluator }
    }
}

impl EvalTest {
    pub fn named(name: &'static str) -> Self {
        let test = Test::named(name);
        let evaluator = HirEvaluator::new(test.context_rc());
        Self { test, evaluator }
    }

    pub fn with_function(&mut self, params: &[Type], results: &[Type]) {
        let name = self.test.name();
        self.test.with_function(name, params, results);
    }
}

impl Deref for EvalTest {
    type Target = Test;

    fn deref(&self) -> &Self::Target {
        &self.test
    }
}

impl DerefMut for EvalTest {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.test
    }
}

const ALIAS_TEST_SOURCE: &str = r#"
builtin.module public @test {
    builtin.function private extern("C") @body(%x: u32) -> u32 { builtin.ret %x : (u32); };
    builtin.function_alias private @first -> @body;
    builtin.function_alias public @api -> @first;
    builtin.function public extern("C") @caller(%x: u32) -> u32 {
        %result = hir.exec @api(%x) : extern("C") (u32) -> u32;
        builtin.ret %result : (u32);
    };
};
"#;

fn parse_alias_test_source() -> Result<(HirEvaluator, UnsafeIntrusiveEntityRef<Module>), Report> {
    let test = Test::default();
    test.context().get_or_register_dialect::<midenc_dialect_hir::HirDialect>();
    let evaluator = HirEvaluator::new(test.context_rc());
    let module = parse::<Module>(
        ParserConfig::new(test.context_rc()),
        Uri::new("eval_alias.hir"),
        ALIAS_TEST_SOURCE,
    )?;
    Ok((evaluator, module))
}

/// Test that we can evaluate a standalone operation, not just callables
///
/// This verifies ControlFlowEffect::None and ControlFlowEffect::Yield.
#[test]
fn eval_test() -> Result<(), Report> {
    let mut test = EvalTest::default();

    let op = {
        let builder = test.builder_mut();
        let block = builder.context_rc().create_block_with_params([Type::I1]);
        let cond = block.borrow().arguments()[0] as ValueRef;
        let conditional = builder.r#if(cond, &[Type::U32], SourceSpan::default())?;

        let then_region = conditional.borrow().then_body().as_region_ref();
        builder.create_block(then_region, None, &[]);
        let is_true = builder.u32(1, SourceSpan::default());
        builder.r#yield([is_true], SourceSpan::default())?;

        let else_region = conditional.borrow().else_body().as_region_ref();
        builder.create_block(else_region, None, &[]);
        let is_false = builder.u32(0, SourceSpan::default());
        builder.r#yield([is_false], SourceSpan::default())?;
        conditional.as_operation_ref()
    };

    let op = op.borrow();
    let results = test.evaluator.eval(&op, [true.into()])?;
    assert_eq!(results.len(), 1);
    assert_eq!(results[0], Value::Immediate(1u32.into()));

    let results = test.evaluator.eval(&op, [false.into()])?;
    assert_eq!(results.len(), 1);
    assert_eq!(results[0], Value::Immediate(0u32.into()));

    Ok(())
}

/// Test evaluation of a callable operation
///
/// This verifies the interaction between ControlFlowEffect::Yield and ControlFlowEffect::Return
#[test]
fn eval_callable_test() -> Result<(), Report> {
    let mut test = EvalTest::named("callable");
    test.with_function(&[Type::I1], &[Type::U32]);

    {
        let mut builder = test.function_builder();
        let cond = builder.current_block().borrow().arguments()[0] as ValueRef;
        let conditional = builder.r#if(cond, &[Type::U32], SourceSpan::default())?;
        let result = conditional.borrow().results()[0] as ValueRef;
        builder.ret(Some(result), SourceSpan::default())?;

        let then_region = conditional.borrow().then_body().as_region_ref();
        let then_block = builder.create_block_in_region(then_region);
        builder.switch_to_block(then_block);
        let is_true = builder.u32(1, SourceSpan::default());
        builder.r#yield([is_true], SourceSpan::default())?;

        let else_region = conditional.borrow().else_body().as_region_ref();
        let else_block = builder.create_block_in_region(else_region);
        builder.switch_to_block(else_block);
        let is_false = builder.u32(0, SourceSpan::default());
        builder.r#yield([is_false], SourceSpan::default())?;
    }

    let function = test.function();
    let callable = function.borrow();
    let results = test.evaluator.eval_callable(&*callable, [true.into()])?;
    assert_eq!(results.len(), 1);
    assert_eq!(results[0], Value::Immediate(1u32.into()));

    let results = test.evaluator.eval_callable(&*callable, [false.into()])?;
    assert_eq!(results.len(), 1);
    assert_eq!(results[0], Value::Immediate(0u32.into()));

    Ok(())
}

#[test]
fn alias_entrypoint_and_nested_call_execute_the_canonical_body() -> Result<(), Report> {
    let (mut evaluator, module) = parse_alias_test_source()?;
    let module = module.borrow();

    for name in ["api", "caller"] {
        let path = module.get(SymbolName::intern(name)).unwrap().borrow().path();
        let results = evaluator.call(module.as_operation(), &path, [42u32.into()])?;
        assert_eq!(results.as_slice(), &[Value::Immediate(42u32.into())]);
    }

    Ok(())
}

#[test]
fn alias_call_with_wrong_signature_fails() -> Result<(), Report> {
    let (mut evaluator, module) = parse_alias_test_source()?;
    let module = module.borrow();

    let alias = module.get(SymbolName::intern("api")).unwrap();
    let path = alias.borrow().path();
    let err = evaluator
        .call(module.as_operation(), &path, [])
        .expect_err("calling the alias without its required argument should fail");
    let expected = alloc::format!("entrypoint '{path}' expects 1 arguments, but 0 were given");
    assert!(
        err.labels()
            .expect("argument-count mismatch should have a diagnostic label")
            .any(|label| label.label() == Some(expected.as_str())),
        "unexpected diagnostic: {err:?}"
    );
    Ok(())
}

#[test]
fn eval_on_alias_executes_the_canonical_body() -> Result<(), Report> {
    let (mut evaluator, module) = parse_alias_test_source()?;
    let module = module.borrow();

    for name in ["first", "api"] {
        let alias = module.get(SymbolName::intern(name)).unwrap();
        let alias = alias.borrow();
        let results = evaluator.eval(alias.as_symbol_operation(), [42u32.into()])?;
        assert_eq!(results.as_slice(), &[Value::Immediate(42u32.into())]);
    }

    Ok(())
}

#[test]
fn eval_on_alias_with_broken_target_reports_resolution_error() -> Result<(), Report> {
    let (mut evaluator, mut module) = parse_alias_test_source()?;
    module.borrow_mut().remove(SymbolName::intern("first"));
    let module = module.borrow();

    let alias = module.get(SymbolName::intern("api")).unwrap();
    let alias = alias.borrow();
    let err = evaluator
        .eval(alias.as_symbol_operation(), [42u32.into()])
        .expect_err("evaluating an alias whose target is missing should fail");
    let label = err
        .labels()
        .expect("unresolvable alias should have a diagnostic label")
        .find_map(|label| label.label().map(alloc::string::ToString::to_string))
        .expect("diagnostic label should have text");
    assert!(label.contains("function alias 'api'"), "unexpected diagnostic: {err:?}");
    assert!(label.contains("does not resolve"), "unexpected diagnostic: {err:?}");

    Ok(())
}

/// Test evaluation of a callable that calls another callable.
///
/// This verifies the handling of ControlFlowEffect::Call and ControlFlowEffect::Return, and their
/// interaction with ControlFlowEffect::Yield
#[test]
fn call_handling_test() -> Result<(), Report> {
    let test = Test::named("call_handling").in_module("test");
    let evaluator = HirEvaluator::new(test.context_rc());
    let mut test = EvalTest { test, evaluator };

    test.with_function(&[Type::I1], &[Type::U32]);

    // Define callee
    let callee = test.define_function("callee", &[Type::I1], &[Type::I1]);

    {
        let callee_signature = callee.borrow().get_signature().clone();
        let mut builder = test.function_builder();
        let input = builder.current_block().borrow().arguments()[0] as ValueRef;
        let call = builder.exec(callee, callee_signature, [input], SourceSpan::default())?;
        let cond = call.borrow().results()[0] as ValueRef;
        {
            let call = call.borrow();
            let callee = call.callee();
            assert_eq!(callee.path().name().as_str(), "callee");
        }
        let conditional = builder.r#if(cond, &[Type::U32], SourceSpan::default())?;
        let result = conditional.borrow().results()[0] as ValueRef;
        builder.ret(Some(result), SourceSpan::default())?;

        let then_region = conditional.borrow().then_body().as_region_ref();
        let then_block = builder.create_block_in_region(then_region);
        builder.switch_to_block(then_block);
        let is_true = builder.u32(1, SourceSpan::default());
        builder.r#yield([is_true], SourceSpan::default())?;

        let else_region = conditional.borrow().else_body().as_region_ref();
        let else_block = builder.create_block_in_region(else_region);
        builder.switch_to_block(else_block);
        let is_false = builder.u32(0, SourceSpan::default());
        builder.r#yield([is_false], SourceSpan::default())?;
    }

    // This function inverts the boolean value it receives and returns it
    {
        let mut builder = FunctionBuilder::new(callee, test.builder_mut());
        let cond = builder.current_block().borrow().arguments()[0] as ValueRef;
        let truthy = builder.i1(true, SourceSpan::default());
        let falsey = builder.i1(false, SourceSpan::default());
        let result = builder.select(cond, falsey, truthy, SourceSpan::default())?;
        builder.ret(Some(result), SourceSpan::default())?;
    }

    let callable = test.function().borrow();
    let results = test.evaluator.eval_callable(&*callable, [true.into()])?;
    assert_eq!(results.len(), 1);
    assert_eq!(results[0], Value::Immediate(0u32.into()));

    let results = test.evaluator.eval_callable(&*callable, [false.into()])?;
    assert_eq!(results.len(), 1);
    assert_eq!(results[0], Value::Immediate(1u32.into()));

    Ok(())
}

#[test]
fn inv_zero_reports_error() -> Result<(), Report> {
    let mut test = EvalTest::named("inv_zero");
    test.with_function(&[], &[Type::Felt]);

    {
        let mut builder = test.function_builder();
        let zero = builder.felt(midenc_hir::Felt::ZERO, SourceSpan::default());
        let inverse = builder.inv(zero, SourceSpan::default())?;
        builder.ret(Some(inverse), SourceSpan::default())?;
    }

    let callable = test.function().borrow();
    let _err = test
        .evaluator
        .eval_callable(&*callable, [])
        .expect_err("zero inverse should produce an evaluation error");

    Ok(())
}

#[test]
fn println_collects_printed_lines() -> Result<(), Report> {
    let mut test = EvalTest::named("println_collects_printed_lines");
    test.with_function(&[], &[]);

    {
        let span = SourceSpan::default();
        let mut builder = test.function_builder();
        let ptr_ty = Type::from(PointerType::new(Type::U8));
        let base_addr = 64u32;

        for (offset, byte) in b"hello".iter().enumerate() {
            let addr = builder.u32(base_addr + offset as u32, span);
            let ptr = builder.inttoptr(addr, ptr_ty.clone(), span)?;
            let value = builder.u8(*byte, span);
            builder.store(ptr, value, span)?;
        }

        let addr = builder.u32(base_addr, span);
        let ptr = builder.inttoptr(addr, ptr_ty, span)?;
        let len = builder.u32(5, span);
        builder.println(ptr, len, span)?;
        builder.ret(None, span)?;
    }

    let callable = test.function().borrow();
    let results = test.evaluator.eval_callable(&*callable, [])?;
    assert!(results.is_empty());
    assert_eq!(test.evaluator.printed_lines(), ["hello"]);

    Ok(())
}

#[test]
fn println_reports_invalid_utf8() -> Result<(), Report> {
    let mut test = EvalTest::named("println_reports_invalid_utf8");
    test.with_function(&[], &[]);

    {
        let span = SourceSpan::default();
        let mut builder = test.function_builder();
        let ptr_ty = Type::from(PointerType::new(Type::U8));
        let addr = builder.u32(96, span);
        let ptr = builder.inttoptr(addr, ptr_ty.clone(), span)?;
        let invalid_utf8 = builder.u8(0xff, span);
        builder.store(ptr, invalid_utf8, span)?;

        let ptr = builder.inttoptr(addr, ptr_ty, span)?;
        let len = builder.u32(1, span);
        builder.println(ptr, len, span)?;
        builder.ret(None, span)?;
    }

    let callable = test.function().borrow();
    test.evaluator
        .eval_callable(&*callable, [])
        .expect_err("invalid UTF-8 should produce an evaluation error");
    assert!(test.evaluator.printed_lines().is_empty());

    Ok(())
}

#[test]
fn wasm_i64_remainder() -> Result<(), Report> {
    let mut test = EvalTest::named("wasm_i64_remainder");
    test.with_function(&[Type::I64, Type::I64], &[Type::I64]);
    {
        let mut builder = test.function_builder();
        let block = builder.current_block();
        let lhs = block.borrow().arguments()[0] as ValueRef;
        let rhs = block.borrow().arguments()[1] as ValueRef;
        let result = builder.i64_rem_s(lhs, rhs, SourceSpan::default())?;
        builder.ret(Some(result), SourceSpan::default())?;
    }
    let function = test.function();
    let callable = function.borrow();
    for (lhs, rhs, expected) in [
        (-7i64, 3i64, -1i64),
        (7, -3, 1),
        (-7, -3, -1),
        (i64::MIN, -1, 0),
        (i64::MAX, i64::MIN, i64::MAX),
    ] {
        let results = test.evaluator.eval_callable(&*callable, [lhs.into(), rhs.into()])?;
        assert_eq!(results.as_slice(), &[Value::Immediate(expected.into())]);
    }
    assert!(test.evaluator.eval_callable(&*callable, [1i64.into(), 0i64.into()]).is_err());
    Ok(())
}
