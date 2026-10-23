use super::*;

fn evaluate_debug_info_fixture(name: &str, args: &[Value], expected: u32) -> Result<(), Report> {
    let test = Test::default();
    let context = test.context_rc();
    context.get_or_register_dialect::<midenc_hir::dialects::debuginfo::DebugInfoDialect>();
    context.get_or_register_dialect::<midenc_dialect_arith::ArithDialect>();
    context.get_or_register_dialect::<midenc_dialect_ub::UndefinedBehaviorDialect>();
    context.get_or_register_dialect::<midenc_dialect_scf::ScfDialect>();
    let module = parse::<Module>(
        ParserConfig::new(context.clone()),
        Uri::new("eval_debuginfo.hir"),
        include_str!("debuginfo.hir"),
    )?;
    let module = module.borrow();
    let path = module.get(SymbolName::intern(name)).unwrap().borrow().path();
    let mut evaluator = HirEvaluator::new(context);
    let results = evaluator.call(module.as_operation(), &path, args.iter().copied())?;
    assert_eq!(results.as_slice(), &[Value::Immediate(expected.into())]);
    Ok(())
}

#[test]
fn debug_declarations_do_not_read_storage() -> Result<(), Report> {
    evaluate_debug_info_fixture("declare", &[42u32.into()], 42)
}

#[test]
fn debug_kills_do_not_change_runtime_values() -> Result<(), Report> {
    evaluate_debug_info_fixture("kill", &[42u32.into()], 42)
}

#[test]
fn debug_values_do_not_consume_poison() -> Result<(), Report> {
    evaluate_debug_info_fixture("value", &[42u32.into()], 42)
}

#[test]
fn debug_lifetimes_do_not_change_control_flow() -> Result<(), Report> {
    for condition in [true, false] {
        let expected = if condition { 42 } else { 43 };
        evaluate_debug_info_fixture("branch", &[condition.into(), 42u32.into()], expected)?;
    }
    Ok(())
}
