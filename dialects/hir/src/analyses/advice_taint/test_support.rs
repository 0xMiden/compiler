//! Shared scaffolding for advice-taint tests: modules holding function tables, indirect-call
//! dispatchers, and advice sources, built through the public builder API.

use alloc::{
    string::{String, ToString},
    sync::Arc,
    vec::Vec,
};

use midenc_dialect_arith::ArithOpBuilder;
use midenc_hir::{
    AddressSpace, Ident, Op, OpBuilder, PointerType, SourceSpan, Type, UnsafeIntrusiveEntityRef,
    ValueRef, Visibility,
    dialects::builtin::{
        BuiltinOpBuilder, FunctionBuilder, FunctionRef, FunctionTableRef, ModuleBuilder,
        attributes::Signature,
    },
    pass::AnalysisManager,
    testing::Test,
};

use super::{AdviceTaintAnalysis, AdviceTaintFinding};
use crate::HirOpBuilder;

/// Define a function returning a raw advice value laundered through an unrealized cast, the
/// shape the taint analysis must flag when it reaches a sink.
pub(super) fn define_raw_advice_source(test: &mut Test) -> FunctionRef {
    let span = SourceSpan::UNKNOWN;
    let function = test.define_function("raw_source", &[], &[Type::U32]);
    let mut builder = FunctionBuilder::new(function, test.builder_mut());
    let advice = builder.advice_pop(span).unwrap();
    let cast = builder.unrealized_conversion_cast(advice, Type::U32, span).unwrap();
    builder.ret([cast], span).unwrap();
    function
}

/// Define a function returning a constant, the clean counterpart of
/// [define_raw_advice_source].
pub(super) fn define_clean_source(test: &mut Test) -> FunctionRef {
    let span = SourceSpan::UNKNOWN;
    let function = test.define_function("clean_source", &[], &[Type::U32]);
    let mut builder = FunctionBuilder::new(function, test.builder_mut());
    let zero = builder.u32(0, span);
    builder.ret([zero], span).unwrap();
    function
}

/// Define a four-slot function table with the given `(slot, callee, type_tag)` entries, in
/// application order (a later entry overwrites an earlier one at the same slot).
pub(super) fn define_table(
    test: &mut Test,
    entries: &[(u32, FunctionRef, u32)],
) -> FunctionTableRef {
    let span = SourceSpan::UNKNOWN;
    let mut module_builder = ModuleBuilder::new(test.module());
    let table = module_builder
        .define_function_table(Ident::from("tbl"), Visibility::Private, 4)
        .unwrap();
    for (slot, callee, type_tag) in entries {
        module_builder
            .append_function_table_entry(table, *slot, *type_tag, *callee, span)
            .unwrap();
    }
    table
}

/// Build an element-space felt pointer to the constant address `addr`, in a form the storage
/// analysis resolves to a static memory address.
pub(super) fn felt_ptr(
    builder: &mut FunctionBuilder<'_, OpBuilder>,
    addr: u32,
    span: SourceSpan,
) -> ValueRef {
    let addr = builder.u32(addr, span);
    let ty =
        Type::Ptr(Arc::new(PointerType::new_with_address_space(Type::Felt, AddressSpace::Element)));
    builder.inttoptr(addr, ty, span).unwrap()
}

/// Define a public dispatcher that calls through `table` expecting `type_tag` and sinks the
/// u32 call result into range-constrained arithmetic; returns the call op.
pub(super) fn define_dispatcher_sinking_result(
    test: &mut Test,
    table: FunctionTableRef,
    type_tag: u32,
) -> UnsafeIntrusiveEntityRef<crate::ops::ExecIndirect> {
    let span = SourceSpan::UNKNOWN;
    let signature = Signature::new(&test.context_rc(), [], [Type::U32]);
    let dispatch = test.define_function("dispatch", &[Type::U32], &[Type::U32]);
    let mut builder = FunctionBuilder::new(dispatch, test.builder_mut());
    let index = builder.entry_block().borrow().arguments()[0] as ValueRef;
    let call = builder.exec_indirect(table, signature, type_tag, index, [], span).unwrap();
    let result = {
        let call = call.borrow();
        let results = call.results();
        let result = results.iter().next().unwrap();
        result.borrow().as_value_ref()
    };
    let one = builder.u32(1, span);
    let sum = builder.add(result, one, span).unwrap();
    builder.ret([sum], span).unwrap();
    call
}

/// Run the advice-taint analysis over the test's module and return its sink findings.
pub(super) fn module_advice_taint_findings(
    test: &Test,
) -> Result<Vec<AdviceTaintFinding>, midenc_hir::Report> {
    let analysis_manager = AnalysisManager::new(test.module().as_operation_ref(), None);
    let analysis = analysis_manager.get_analysis::<AdviceTaintAnalysis>()?;
    Ok(analysis.findings().to_vec())
}

/// The sink operation names of `findings`, in finding order.
pub(super) fn sink_names(findings: &[AdviceTaintFinding]) -> Vec<String> {
    findings.iter().map(|finding| finding.sink.to_string()).collect()
}
