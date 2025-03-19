use alloc::rc::Rc;

use midenc_dialect_arith::ArithOpBuilder;
use midenc_dialect_cf::ControlFlowOpBuilder;
use midenc_dialect_hir::HirOpBuilder;
use midenc_dialect_scf::StructuredControlFlowOpBuilder;
use midenc_hir::{
    dialects::builtin::{BuiltinOpBuilder, FunctionBuilder},
    AbiParam, Builder, Context, Ident, Op, OpBuilder, ProgramPoint, Report, Signature, SourceSpan,
    SymbolTable, Type, ValueRef,
};

use crate::*;

struct TestContext {
    context: Rc<Context>,
    evaluator: HirEvaluator,
}

fn setup() -> TestContext {
    let context = Rc::new(Context::default());
    register_dialect_hooks(&context);
    let evaluator = HirEvaluator::new(context.clone());

    TestContext { context, evaluator }
}

/// Test that we can evaluate a standalone operation, not just callables
///
/// This verifies ControlFlowEffect::None and ControlFlowEffect::Yield.
#[test]
fn eval_test() -> Result<(), Report> {
    let mut test_context = setup();

    let mut builder = OpBuilder::new(test_context.context.clone());

    let op = {
        let block = builder.context().create_block_with_params([Type::I1]);
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
    let results = test_context.evaluator.eval(&op, [true.into()])?;
    assert_eq!(results.len(), 1);
    assert_eq!(results[0], Value::Immediate(1u32.into()));

    let results = test_context.evaluator.eval(&op, [false.into()])?;
    assert_eq!(results.len(), 1);
    assert_eq!(results[0], Value::Immediate(0u32.into()));

    Ok(())
}

/// Test evaluation of a callable operation
///
/// This verifies the interaction between ControlFlowEffect::Yield and ControlFlowEffect::Return
#[test]
fn eval_callable_test() -> Result<(), Report> {
    let mut test_context = setup();

    let mut builder = OpBuilder::new(test_context.context.clone());

    let mut function = builder.create_function(
        Ident::with_empty_span("test".into()),
        Signature::new([AbiParam::new(Type::I1)], [AbiParam::new(Type::U32)]),
    )?;

    {
        let mut function = function.borrow_mut();
        let mut builder = FunctionBuilder::new(&mut function, &mut builder);
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

    let callable = function.borrow();
    let results = test_context.evaluator.eval_callable(&*callable, [true.into()])?;
    assert_eq!(results.len(), 1);
    assert_eq!(results[0], Value::Immediate(1u32.into()));

    let results = test_context.evaluator.eval_callable(&*callable, [false.into()])?;
    assert_eq!(results.len(), 1);
    assert_eq!(results[0], Value::Immediate(0u32.into()));

    Ok(())
}

/// Test evaluation of a callable that calls another callable.
///
/// This verifies the handling of ControlFlowEffect::Call and ControlFlowEffect::Return, and their
/// interaction with ControlFlowEffect::Yield
#[test]
fn call_handling_test() -> Result<(), Report> {
    let mut test_context = setup();

    let mut builder = OpBuilder::new(test_context.context.clone());

    let mut module = builder.create_module(Ident::with_empty_span("test".into()))?;

    let module_body = module.borrow().body().as_region_ref();
    builder.create_block(module_body, None, &[]);

    // Define entry
    let mut entry = builder.create_function(
        Ident::with_empty_span("entrypoint".into()),
        Signature::new([AbiParam::new(Type::I1)], [AbiParam::new(Type::U32)]),
    )?;
    module
        .borrow_mut()
        .symbol_manager_mut()
        .insert_new(entry, ProgramPoint::Invalid);

    // Define callee
    let callee_signature = Signature::new([AbiParam::new(Type::I1)], [AbiParam::new(Type::I1)]);
    let mut callee = builder
        .create_function(Ident::with_empty_span("callee".into()), callee_signature.clone())?;
    module
        .borrow_mut()
        .symbol_manager_mut()
        .insert_new(callee, ProgramPoint::Invalid);

    {
        let mut function = entry.borrow_mut();
        let mut builder = FunctionBuilder::new(&mut function, &mut builder);
        let input = builder.current_block().borrow().arguments()[0] as ValueRef;
        let call = builder.exec(callee, callee_signature, [input], SourceSpan::default())?;
        let cond = call.borrow().results()[0] as ValueRef;
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
        let mut function = callee.borrow_mut();
        let mut builder = FunctionBuilder::new(&mut function, &mut builder);
        let cond = builder.current_block().borrow().arguments()[0] as ValueRef;
        let truthy = builder.i1(true, SourceSpan::default());
        let falsey = builder.i1(false, SourceSpan::default());
        let result = builder.select(cond, falsey, truthy, SourceSpan::default())?;
        builder.ret(Some(result), SourceSpan::default())?;
    }

    let callable = entry.borrow();
    let results = test_context.evaluator.eval_callable(&*callable, [true.into()])?;
    assert_eq!(results.len(), 1);
    assert_eq!(results[0], Value::Immediate(0u32.into()));

    let results = test_context.evaluator.eval_callable(&*callable, [false.into()])?;
    assert_eq!(results.len(), 1);
    assert_eq!(results[0], Value::Immediate(1u32.into()));

    Ok(())
}
