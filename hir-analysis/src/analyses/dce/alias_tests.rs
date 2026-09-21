// TODO move this into `../dce.rs`

use alloc::{format, string::ToString, vec::Vec};

use midenc_hir::{
    CallOpInterface, ImmediateAttr, Op, OperationRef, ProgramPoint, Symbol, Usable, ValueRef,
    diagnostics::Uri,
    dialects::builtin::{Function, FunctionAlias, ModuleBuilder},
    parse::{self, ParserConfig},
    pass::AnalysisManager,
    testing::Test,
};

use super::{DeadCodeAnalysis, Executable, PredecessorState};
use crate::{
    DataFlowConfig, DataFlowSolver, Lattice,
    analyses::{SparseConstantPropagation, constant_propagation::ConstantValue},
};

fn analyze(source: &str) -> (Test, OperationRef, DataFlowSolver) {
    let test = Test::default();
    test.context().get_or_register_dialect::<midenc_dialect_hir::HirDialect>();
    test.context().get_or_register_dialect::<midenc_dialect_arith::ArithDialect>();
    let module = parse::parse_any(
        ParserConfig::new(test.context_rc()),
        Uri::new("function_alias.hir"),
        source,
    )
    .expect("fixture must parse and verify");
    let mut config = DataFlowConfig::default();
    config.set_interprocedural(true);
    let mut solver = DataFlowSolver::new(config);
    solver.load::<DeadCodeAnalysis>();
    solver.load::<SparseConstantPropagation>();
    solver
        .initialize_and_run(&module.borrow(), AnalysisManager::new(module, None))
        .expect("analysis must converge");
    (test, module, solver)
}

fn alias_module(visibility: &str, extra_use: &str) -> alloc::string::String {
    format!(
        r#"
builtin.module public @test {{
    builtin.function private extern("C") @target(%x: u32) -> u32 {{
        builtin.ret %x : (u32);
    }};
    builtin.function_alias private @first -> @target;
    builtin.function_alias {visibility} @alias -> @first;
    builtin.function public extern("C") @caller() -> u32 {{
        {extra_use}
        %arg = arith.constant 42 : u32;
        %result = hir.exec @alias(%arg) : extern("C") (u32) -> u32;
        builtin.ret %result : (u32);
    }};
}};
"#
    )
}

/// Get the constant-propagated value of `value` or `None` if it is not a known constant.
fn constant(solver: &DataFlowSolver, value: ValueRef) -> Option<u32> {
    let lattice = solver.get::<Lattice<ConstantValue>, _>(&value).expect("value has a lattice");
    assert!(!lattice.value().is_uninitialized(), "call flow must initialize the value");
    lattice
        .value()
        .constant_value()
        .map(|attr| attr.borrow().downcast_ref::<ImmediateAttr>().unwrap().as_u32().unwrap())
}

/// A chain of private aliases is fully resolved by the analysis: call/return flow and constant
/// propagation work as if called directly, without rewriting any symbol references in the IR
#[test]
fn private_alias_chain_resolves_callsites_and_propagates_constants() {
    let (_test, module, solver) = analyze(&alias_module("private", ""));
    let mb = ModuleBuilder::new(
        module.try_downcast_op::<midenc_hir::dialects::builtin::Module>().unwrap(),
    );
    let target = mb.get_function("target").unwrap();
    let alias = mb.get_function_alias("alias").unwrap();
    let first = mb.get_function_alias("first").unwrap();
    let caller = mb.get_function("caller").unwrap();
    let call = caller
        .borrow()
        .entry_block()
        .borrow()
        .body()
        .iter()
        .find_map(|op| op.downcast_ref::<midenc_dialect_hir::Exec>().map(Op::as_operation_ref))
        .unwrap();

    let callsites = solver
        .get::<PredecessorState, _>(&ProgramPoint::after(target.as_operation_ref()))
        .unwrap();
    assert!(callsites.all_predecessors_known(), "private aliases do not escape");
    assert_eq!(callsites.known_predecessors(), &[call]);
    let returns = solver.get::<PredecessorState, _>(&ProgramPoint::after(call)).unwrap();
    assert_eq!(
        returns.known_predecessors(),
        &[target.borrow().entry_block().borrow().terminator().unwrap(),]
    );
    assert_eq!(
        constant(&solver, target.borrow().entry_block().borrow().arguments()[0] as ValueRef),
        Some(42)
    );
    assert_eq!(constant(&solver, call.borrow().results()[0] as ValueRef), Some(42));

    // Analysis resolves identity without rewriting any of the alias symbol uses.
    assert_eq!(alias.borrow().iter_uses().count(), 1);
    assert_eq!(first.borrow().iter_uses().count(), 1);
    assert_eq!(target.borrow().iter_uses().count(), 1);
    assert!(alias.borrow().visibility().is_private());
    let call = call.borrow();
    let call = call.downcast_ref::<midenc_dialect_hir::Exec>().unwrap();
    assert_eq!(call.callee().path().to_string(), "alias");
    assert_eq!(call.callee().user().borrow().owner, call.as_operation_ref());
    assert_eq!(first.borrow().target().user().borrow().owner, first.as_operation_ref());
    assert_eq!(
        call.resolve().map(|c| c.as_symbol_ref()),
        target.borrow().as_operation().as_symbol_ref()
    );
}

#[test]
fn public_alias_keeps_target_arguments_unknown() {
    let (_test, module, solver) = analyze(&alias_module("public", ""));
    let mb = ModuleBuilder::new(
        module.try_downcast_op::<midenc_hir::dialects::builtin::Module>().unwrap(),
    );
    let target = mb.get_function("target").unwrap();
    let state = solver
        .get::<PredecessorState, _>(&ProgramPoint::after(target.as_operation_ref()))
        .unwrap();
    assert!(!state.all_predecessors_known(), "external callers may use the public alias");
    assert_eq!(state.known_predecessors().len(), 1, "retain the known call through the alias");
    assert_eq!(
        constant(&solver, target.borrow().entry_block().borrow().arguments()[0] as ValueRef),
        None
    );
}

#[test]
fn taking_alias_address_keeps_target_arguments_unknown() {
    let (_test, module, solver) = analyze(&alias_module(
        "private",
        "%root0, %root1, %root2, %root3 = hir.procedure_root @alias;",
    ));
    let mb = ModuleBuilder::new(
        module.try_downcast_op::<midenc_hir::dialects::builtin::Module>().unwrap(),
    );
    let target = mb.get_function("target").unwrap();
    let state = solver
        .get::<PredecessorState, _>(&ProgramPoint::after(target.as_operation_ref()))
        .unwrap();
    assert!(
        !state.all_predecessors_known(),
        "address-taking through an alias escapes its target"
    );
    assert_eq!(
        constant(&solver, target.borrow().entry_block().borrow().arguments()[0] as ValueRef),
        None
    );
}

#[test]
fn indirect_alias_calls_share_target_and_return_flow() {
    let (_test, module, solver) = analyze(
        r#"
builtin.module public @test {
    builtin.function private extern("C") @target() -> u32 {
        %value = arith.constant 42 : u32;
        builtin.ret %value : (u32);
    };
    builtin.function_alias private @first -> @target;
    builtin.function_alias private @alias -> @first;
    builtin.function_table private @table : 2 {
        builtin.function_table_entry 0 @first tag 1;
        builtin.function_table_entry 1 @alias tag 1;
    };
    builtin.function public extern("C") @caller(%index: u32) -> u32 {
        %result = hir.exec_indirect @table[%index]() : extern("C") () -> u32 tag 1;
        builtin.ret %result : (u32);
    };
};
"#,
    );

    let mut calls = Vec::new();
    module.borrow().prewalk_all(|op| {
        if let Some(call) = op.downcast_ref::<midenc_dialect_hir::ExecIndirect>() {
            calls.push(call.as_operation_ref());
        }
    });
    assert_eq!(calls.len(), 1, "the fixture contains a single indirect call");
    let call_ref = calls[0];
    let call = call_ref.borrow();
    let call = call.downcast_ref::<midenc_dialect_hir::ExecIndirect>().unwrap();
    let callees = call.possible_callees().unwrap();
    assert_eq!(callees.len(), 1, "deduplicate after resolving aliases");

    let target_ref = callees[0].as_operation_ref();
    let target = target_ref.borrow();
    assert!(target.is::<Function>());
    assert!(!target.is::<FunctionAlias>());
    let entry = target.downcast_ref::<Function>().unwrap().entry_block();
    assert!(
        solver
            .get::<Executable, _>(&ProgramPoint::at_start_of(entry))
            .unwrap()
            .is_live()
    );

    let callsites = solver.get::<PredecessorState, _>(&ProgramPoint::after(target_ref)).unwrap();
    assert!(!callsites.all_predecessors_known(), "table entries take the aliases' address");
    assert_eq!(
        callsites.known_predecessors(),
        &[call_ref],
        "the indirect call registers as the only known call site of the shared target"
    );

    let returns = solver.get::<PredecessorState, _>(&ProgramPoint::after(call_ref)).unwrap();
    assert_eq!(
        returns.known_predecessors(),
        &[entry.borrow().terminator().unwrap()],
        "the target's return flows back to the indirect call"
    );
    assert_eq!(
        constant(&solver, call.results()[0] as ValueRef),
        Some(42),
        "constant is propagated"
    );
}

/// Dispatching through a function table takes the aliases' address, so the target may have
/// callers the analysis cannot see and its arguments stay unknown even though the call site
/// passes a constant.
#[test]
fn table_dispatch_through_alias_keeps_target_arguments_unknown() {
    let (_test, module, solver) = analyze(
        r#"
builtin.module public @test {
    builtin.function private extern("C") @target(%x: u32) -> u32 {
        builtin.ret %x : (u32);
    };
    builtin.function_alias private @first -> @target;
    builtin.function_alias private @alias -> @first;
    builtin.function_table private @table : 2 {
        builtin.function_table_entry 0 @first tag 1;
        builtin.function_table_entry 1 @alias tag 1;
    };
    builtin.function public extern("C") @caller(%index: u32) -> u32 {
        %arg = arith.constant 42 : u32;
        %result = hir.exec_indirect @table[%index](%arg) : extern("C") (u32) -> u32 tag 1;
        builtin.ret %result : (u32);
    };
};
"#,
    );

    let mb = ModuleBuilder::new(
        module.try_downcast_op::<midenc_hir::dialects::builtin::Module>().unwrap(),
    );
    let target = mb.get_function("target").unwrap();
    let state = solver
        .get::<PredecessorState, _>(&ProgramPoint::after(target.as_operation_ref()))
        .unwrap();
    assert!(!state.all_predecessors_known(), "table entries take the aliases' address");
    assert_eq!(
        state.known_predecessors().len(),
        1,
        "retain the known indirect call through the table"
    );
    assert_eq!(
        constant(&solver, target.borrow().entry_block().borrow().arguments()[0] as ValueRef),
        None,
        "unknown callers keep the target's arguments from becoming constant"
    );
}
