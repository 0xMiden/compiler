use alloc::{boxed::Box, rc::Rc};

use midenc_dialect_scf::If;
use midenc_hir::{
    Context, Report,
    diagnostics::Uri,
    dialects::builtin::{Function, FunctionRef},
    parse::{self, ParserConfig},
    pass::{Nesting, PassManager},
};

use super::CommonSubexpressionElimination;

fn parse_function(context: &Rc<Context>, source: &str) -> Result<FunctionRef, Report> {
    parse::parse::<Function>(ParserConfig::new(context.clone()), Uri::new("regions.hir"), source)
}

#[test]
fn cse_eliminates_equivalent_regions() -> Result<(), Report> {
    let context = Rc::new(Context::default());
    let function = parse_function(
        &context,
        r#"
builtin.function public extern("C") @regions(%c: i1, %a: u32, %b: u32) -> (u32, u32) {
    %x = scf.if %c then {
        %sum = arith.add %a, %b <{ overflow = #builtin.overflow<unchecked> }>;
        scf.yield %sum : (u32);
    } else {
        scf.yield %b : (u32);
    } : (i1) -> (u32);
    %y = scf.if %c then {
        %sum2 = arith.add %b, %a <{ overflow = #builtin.overflow<unchecked> }>;
        scf.yield %sum2 : (u32);
    } else {
        scf.yield %b : (u32);
    } : (i1) -> (u32);
    builtin.ret %x, %y : (u32, u32);
};
"#,
    )?;
    function.as_operation_ref().borrow().recursively_verify()?;
    let mut passes = PassManager::on::<Function>(context, Nesting::Implicit);
    passes.add_pass(Box::<CommonSubexpressionElimination>::default());
    passes.run(function.as_operation_ref())?;
    function.as_operation_ref().borrow().recursively_verify()?;
    let function = function.borrow();
    let body = function.body();
    let entry = body.entry();
    assert_eq!(entry.body().iter().filter(|op| op.is::<If>()).count(), 1);
    let ret = entry.terminator().unwrap();
    let ret = ret.borrow();
    let operands = ret.operands().all();
    assert_eq!(operands[0].borrow().as_value_ref(), operands[1].borrow().as_value_ref());
    Ok(())
}

#[test]
fn region_equivalence_maps_nested_values_and_preserves_captures() -> Result<(), Report> {
    use midenc_hir::equivalence::OperationEquivalenceFlags;

    let context = Rc::new(Context::default());
    let source = r#"
builtin.function public extern("C") @nested(%c: i1, %a: u32, %b: u32) -> u32 {
    %first = arith.add %a, %b <{ overflow = #builtin.overflow<unchecked> }>;
    %second = arith.add %a, %a <{ overflow = #builtin.overflow<unchecked> }>;
    %x = scf.if %c then {
        %sum = arith.add %first, %second <{ overflow = #builtin.overflow<unchecked> }>;
        scf.yield %sum : (u32);
    } else {
        scf.yield %b : (u32);
    } : (i1) -> (u32);
    builtin.ret %x : (u32);
};
"#;
    let lhs = parse_function(&context, source)?;
    let equivalent = source
        .replace("%first, %second", "%second, %first")
        .replace("%first", "%renamed");
    let rhs = parse_function(&context, &equivalent)?;
    let different_capture =
        parse_function(&context, &source.replace("scf.yield %b", "scf.yield %a"))?;
    let different_body = parse_function(&context, &source.replace("%a, %a", "%b, %b"))?;
    let lhs = lhs.as_operation_ref();
    let flags = OperationEquivalenceFlags::IGNORE_LOCATIONS;
    assert!(lhs.borrow().is_equivalent(&rhs.as_operation_ref().borrow(), flags));
    assert!(
        !lhs.borrow()
            .is_equivalent(&different_capture.as_operation_ref().borrow(), flags)
    );
    assert!(!lhs.borrow().is_equivalent(&different_body.as_operation_ref().borrow(), flags));
    Ok(())
}

#[test]
fn region_equivalence_preserves_branch_targets_and_case_keys() -> Result<(), Report> {
    use midenc_hir::equivalence::OperationEquivalenceFlags;

    let context = Rc::new(Context::default());
    let source = r#"
builtin.function public extern("C") @select(%sel: u32, %a: u32, %b: u32) -> u32 {
    cf.switch %sel [#builtin.u32<1> -> ^one(%a : u32)], ^fallback(%b : u32) : (u32);
^one(%x: u32):
    builtin.ret %x : (u32);
^fallback(%y: u32):
    builtin.ret %y : (u32);
};
"#;
    let lhs = parse_function(&context, source)?;
    let rhs =
        parse_function(&context, &source.replace("^one", "^renamed").replace("%x", "%renamed"))?;
    let different_key =
        parse_function(&context, &source.replace("#builtin.u32<1>", "#builtin.u32<2>"))?;
    let different_target =
        parse_function(&context, &source.replace("^one(%a : u32)", "^fallback(%a : u32)"))?;
    let flags = OperationEquivalenceFlags::IGNORE_LOCATIONS;
    let lhs = lhs.as_operation_ref();
    assert!(lhs.borrow().is_equivalent(&rhs.as_operation_ref().borrow(), flags));
    assert!(!lhs.borrow().is_equivalent(&different_key.as_operation_ref().borrow(), flags));
    assert!(!lhs.borrow().is_equivalent(&different_target.as_operation_ref().borrow(), flags));
    Ok(())
}

#[test]
fn region_equivalence_handles_graph_forward_references() {
    use midenc_dialect_arith::ArithOpBuilder;
    use midenc_hir::{Builder, SourceSpan, equivalence::OperationEquivalenceFlags, testing::Test};

    let mut lhs = Test::named("forward_refs").in_module("forward_refs");
    let mut rhs = Test::named("forward_refs").in_module("forward_refs");
    for (test, reverse_allocation) in [(&mut lhs, false), (&mut rhs, true)] {
        let block = test.module().borrow().body().entry_block_ref().unwrap();
        let builder = test.builder_mut();
        builder.set_insertion_point_to_end(block);
        // Opposite allocation order ensures address sorting cannot substitute for the SSA map.
        let (seven, nine) = if reverse_allocation {
            let nine = builder.u32(9, SourceSpan::UNKNOWN);
            (builder.u32(7, SourceSpan::UNKNOWN), nine)
        } else {
            let seven = builder.u32(7, SourceSpan::UNKNOWN);
            (seven, builder.u32(9, SourceSpan::UNKNOWN))
        };
        let sum = builder.add_unchecked(seven, nine, SourceSpan::UNKNOWN).unwrap();
        for value in [sum, seven, nine] {
            let mut op = value.borrow().get_defining_op().unwrap();
            op.borrow_mut().remove();
            op.insert_at_end(block);
        }
    }
    assert!(lhs.module().as_operation_ref().borrow().is_equivalent(
        &rhs.module().as_operation_ref().borrow(),
        OperationEquivalenceFlags::IGNORE_LOCATIONS,
    ));
}
