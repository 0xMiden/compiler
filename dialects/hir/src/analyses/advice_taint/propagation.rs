use midenc_hir::{
    CallOpInterface, Forward, Operation, Report, Spanned, Value,
    traits::operation_result_value_range_refinement,
};
use midenc_hir_analysis::{
    AnalysisStateGuard, AnalysisStateGuardMut, BuildableDataFlowAnalysis, CallControlFlowAction,
    DataFlowSolver, SparseForwardDataFlowAnalysis, SparseLattice,
    analyses::{DeadCodeAnalysis, SparseConstantPropagation},
    sparse::SparseDataFlowAnalysis,
};

use super::{
    lattice::{AdviceTaintSparseLattice, CallContextFrame, ContextualAdviceTaintValue},
    layout::ADVICE_PIPE_RAW_RESULT_COUNT,
    sinks::{
        external_call_result_has_unconstrained_advice_effect, is_range_constrained_sink,
        is_unconstrained_external_result_type, operation_result_has_advice_read_effect,
        range_constrained_operand_indices,
    },
};
use crate::AdvicePipe;

/// Sparse propagation of unconstrained advice taint through SSA values.
#[derive(Default)]
pub struct AdviceTaintPropagation;

impl BuildableDataFlowAnalysis for AdviceTaintPropagation {
    type Strategy = SparseDataFlowAnalysis<Self, Forward>;

    fn new(solver: &mut DataFlowSolver) -> Self {
        solver.load::<DeadCodeAnalysis>();
        solver.load::<SparseConstantPropagation>();
        Self
    }
}

impl SparseForwardDataFlowAnalysis for AdviceTaintPropagation {
    type Lattice = AdviceTaintSparseLattice;

    fn debug_name(&self) -> &'static str {
        "unconstrained-advice-taint"
    }

    fn allow_unknown_predecessors(&self) -> bool {
        true
    }

    fn visit_operation(
        &self,
        op: &Operation,
        operands: &[AnalysisStateGuard<'_, Self::Lattice>],
        results: &mut [AnalysisStateGuardMut<'_, Self::Lattice>],
        _solver: &mut DataFlowSolver,
    ) -> Result<(), Report> {
        if op.is::<AdvicePipe>() {
            return join_advice_pipe_results(op, operands, results);
        }

        let operand_taint =
            ContextualAdviceTaintValue::join_all(operands.iter().map(|operand| operand.value()));
        let range_constrained_operand_taint = ContextualAdviceTaintValue::join_all(
            range_constrained_operand_indices(op)
                .into_iter()
                .filter_map(|index| operands.get(index).map(|operand| operand.value())),
        );
        transfer_results(op, operand_taint, range_constrained_operand_taint, results)
    }

    fn visit_call_control_flow_transfer(
        &self,
        call: &dyn CallOpInterface,
        action: CallControlFlowAction,
        before: &[AnalysisStateGuard<'_, Self::Lattice>],
        after: &mut [AnalysisStateGuardMut<'_, Self::Lattice>],
        _solver: &mut DataFlowSolver,
    ) {
        let frame = CallContextFrame::new(call);
        match action {
            CallControlFlowAction::Enter => {
                for (argument, parameter) in before.iter().zip(after.iter_mut()) {
                    parameter.join(&argument.value().enter_call(frame));
                }
            }
            CallControlFlowAction::Exit => {
                for (returned, result) in before.iter().zip(after.iter_mut()) {
                    result.join(&returned.value().exit_call(frame));
                }
            }
            CallControlFlowAction::External => {
                let span = call.as_operation().span();
                // A call with no resolvable callee reaches here only when its possible-callee
                // set is unknown or contains an unanalyzable callable (calls through a function
                // table with statically-known callees are handled interprocedurally instead).
                // Nothing constrains what such a callee returns, so its results are
                // conservatively treated like an advice-reading external call rather than
                // trusted clean.
                let unanalyzable_callee = call.resolve().is_none();
                for (result_index, (result_value, result)) in
                    call.as_operation().results().all().iter().zip(after).enumerate()
                {
                    let result_value = result_value.borrow();
                    let value = if (unanalyzable_callee
                        || external_call_result_has_unconstrained_advice_effect(call, result_index))
                        && is_unconstrained_external_result_type(result_value.ty())
                    {
                        ContextualAdviceTaintValue::external_call(span)
                    } else {
                        ContextualAdviceTaintValue::clean()
                    };
                    result.join(&value);
                }
            }
        }
    }

    fn set_to_entry_state(&self, lattice: &mut AnalysisStateGuardMut<'_, Self::Lattice>) {
        lattice.join(&ContextualAdviceTaintValue::clean());
    }
}

fn transfer_results(
    op: &Operation,
    operand_taint: ContextualAdviceTaintValue,
    range_constrained_operand_taint: ContextualAdviceTaintValue,
    results: &mut [AnalysisStateGuardMut<'_, AdviceTaintSparseLattice>],
) -> Result<(), Report> {
    let transferred_operand_taint =
        transfer_taint(op, operand_taint, range_constrained_operand_taint);
    let op_results = op.results().all();
    for (index, result) in results.iter_mut().enumerate() {
        let result_value = op_results[index].borrow().as_value_ref();
        let result_taint = if operation_result_has_advice_read_effect(op, result_value) {
            ContextualAdviceTaintValue::raw(op.span())
        } else if operation_result_value_range_refinement(op, result_value).is_some() {
            ContextualAdviceTaintValue::clean()
        } else {
            transferred_operand_taint.clone()
        };
        result.join(&result_taint);
    }
    Ok(())
}

fn join_advice_pipe_results(
    op: &Operation,
    operands: &[AnalysisStateGuard<'_, AdviceTaintSparseLattice>],
    results: &mut [AnalysisStateGuardMut<'_, AdviceTaintSparseLattice>],
) -> Result<(), Report> {
    for (index, result) in results.iter_mut().enumerate() {
        let taint = if index < ADVICE_PIPE_RAW_RESULT_COUNT {
            ContextualAdviceTaintValue::raw(op.span())
        } else {
            operands.get(index).map(|operand| operand.value().clone()).unwrap_or_default()
        };
        result.join(&taint);
    }

    Ok(())
}

fn transfer_taint(
    op: &Operation,
    operand_taint: ContextualAdviceTaintValue,
    range_constrained_operand_taint: ContextualAdviceTaintValue,
) -> ContextualAdviceTaintValue {
    if is_range_constrained_sink(op) && range_constrained_operand_taint.has_unreported_origin() {
        operand_taint.mark_origins_reported(range_constrained_operand_taint.unreported_origins())
    } else {
        operand_taint
    }
}

#[cfg(test)]
mod tests {
    use alloc::vec::Vec;

    use midenc_dialect_arith::ArithOpBuilder;
    use midenc_hir::{
        Op, SourceSpan, Type, ValueRef,
        dialects::builtin::{BuiltinOpBuilder, FunctionBuilder, attributes::Signature},
        pass::AnalysisManager,
        testing::Test,
    };

    use super::super::{
        AdviceTaintAnalysis, AdviceTaintFinding,
        test_support::{
            define_clean_source, define_dispatcher_sinking_result, define_raw_advice_source,
            define_table, module_advice_taint_findings, sink_names,
        },
    };
    use crate::HirOpBuilder;

    #[test]
    fn checked_cast_sanitizes_raw_advice() -> Result<(), midenc_hir::Report> {
        let mut test = Test::new("checked_cast", &[], &[Type::U32]);
        {
            let span = SourceSpan::UNKNOWN;
            let mut builder = test.function_builder();
            let advice = builder.advice_pop(span)?;
            let cast = builder.cast(advice, Type::U32, span)?;
            let one = builder.u32(1, span);
            let sum = builder.add(cast, one, span)?;
            builder.ret([sum], span)?;
        }

        let findings = advice_taint_findings(&test)?;
        assert!(findings.is_empty(), "checked cast should sanitize raw advice");

        Ok(())
    }

    #[test]
    fn unrealized_conversion_cast_propagates_raw_advice() -> Result<(), midenc_hir::Report> {
        let mut test = Test::new("unrealized_cast", &[], &[Type::U32]);
        {
            let span = SourceSpan::UNKNOWN;
            let mut builder = test.function_builder();
            let advice = builder.advice_pop(span)?;
            let cast = builder.unrealized_conversion_cast(advice, Type::U32, span)?;
            let one = builder.u32(1, span);
            let sum = builder.add(cast, one, span)?;
            builder.ret([sum], span)?;
        }

        let findings = advice_taint_findings(&test)?;
        assert_eq!(sink_names(&findings), ["arith.add"]);

        Ok(())
    }

    #[test]
    fn checked_assertion_sanitizes_unrealized_cast_result() -> Result<(), midenc_hir::Report> {
        let mut test = Test::new("checked_assertion", &[], &[Type::U32]);
        {
            let span = SourceSpan::UNKNOWN;
            let mut builder = test.function_builder();
            let advice = builder.advice_pop(span)?;
            let cast = builder.unrealized_conversion_cast(advice, Type::U32, span)?;
            let asserted = builder.assert_u32(cast, span)?;
            let one = builder.u32(1, span);
            let sum = builder.add(asserted, one, span)?;
            builder.ret([sum], span)?;
        }

        let findings = advice_taint_findings(&test)?;
        assert!(
            findings.is_empty(),
            "checked assertion should sanitize even when the input type is already u32"
        );

        Ok(())
    }

    #[test]
    fn assert_sanitizes_raw_advice_as_known_one() -> Result<(), midenc_hir::Report> {
        let mut test = Test::new("assert_one", &[], &[Type::U32]);
        {
            let span = SourceSpan::UNKNOWN;
            let mut builder = test.function_builder();
            let advice = builder.advice_pop(span)?;
            let asserted = builder.assert(advice, span)?;
            let cast = builder.unrealized_conversion_cast(asserted, Type::U32, span)?;
            let one = builder.u32(1, span);
            let sum = builder.add(cast, one, span)?;
            builder.ret([sum], span)?;
        }

        let findings = advice_taint_findings(&test)?;
        assert!(findings.is_empty(), "assert should sanitize raw advice as known one");

        Ok(())
    }

    #[test]
    fn assertz_sanitizes_raw_advice_as_known_zero() -> Result<(), midenc_hir::Report> {
        let mut test = Test::new("assert_zero", &[], &[Type::U32]);
        {
            let span = SourceSpan::UNKNOWN;
            let mut builder = test.function_builder();
            let advice = builder.advice_pop(span)?;
            let asserted = builder.assertz(advice, span)?;
            let cast = builder.unrealized_conversion_cast(asserted, Type::U32, span)?;
            let one = builder.u32(1, span);
            let sum = builder.add(cast, one, span)?;
            builder.ret([sum], span)?;
        }

        let findings = advice_taint_findings(&test)?;
        assert!(findings.is_empty(), "assertz should sanitize raw advice as known zero");

        Ok(())
    }

    /// Dispatching through a function table joins the taint of the tag-matching callee's return
    /// value into the call result, exactly like a direct call would.
    #[test]
    fn indirect_call_joins_table_callee_result_taint() -> Result<(), midenc_hir::Report> {
        let mut test = Test::named("indirect_result_taint").in_module("m");
        let raw_source = define_raw_advice_source(&mut test);
        let table = define_table(&mut test, &[(1, raw_source, 1)]);
        define_dispatcher_sinking_result(&mut test, table, /*type_tag=*/ 1);

        let findings = module_advice_taint_findings(&test)?;
        assert_eq!(sink_names(&findings), ["arith.add"]);

        Ok(())
    }

    /// A table entry whose signature tag differs from the call site's tag can only trap at
    /// runtime, never return, so its taint must not reach the call result.
    #[test]
    fn indirect_call_ignores_mismatched_signature_entries() -> Result<(), midenc_hir::Report> {
        let mut test = Test::named("indirect_tag_mismatch").in_module("m");
        let raw_source = define_raw_advice_source(&mut test);
        let table = define_table(&mut test, &[(1, raw_source, 2)]);
        define_dispatcher_sinking_result(&mut test, table, /*type_tag=*/ 1);

        let findings = module_advice_taint_findings(&test)?;
        assert!(
            findings.is_empty(),
            "a mismatched-tag entry can only trap, so its taint must not propagate: {findings:?}"
        );

        Ok(())
    }

    /// A dispatchable entry without a body cannot be analyzed, so the call result is
    /// conservatively treated as unconstrained instead of trusted clean.
    #[test]
    fn indirect_call_with_unanalyzable_entry_taints_results() -> Result<(), midenc_hir::Report> {
        let span = SourceSpan::UNKNOWN;
        let mut test = Test::named("indirect_unanalyzable").in_module("m");
        // A declaration: no body to analyze
        let extern_source = test.define_function("extern_source", &[], &[Type::Felt]);
        let table = define_table(&mut test, &[(1, extern_source, 1)]);

        let signature = Signature::new(&test.context_rc(), [], [Type::Felt]);
        let dispatch = test.define_function("dispatch", &[Type::U32], &[Type::U32]);
        {
            let mut builder = FunctionBuilder::new(dispatch, test.builder_mut());
            let index = builder.entry_block().borrow().arguments()[0] as ValueRef;
            let call = builder.exec_indirect(table, signature, 1, index, [], span)?;
            let result = {
                let call = call.borrow();
                let results = call.results();
                let result = results.iter().next().unwrap();
                result.borrow().as_value_ref()
            };
            let cast = builder.unrealized_conversion_cast(result, Type::U32, span)?;
            let one = builder.u32(1, span);
            let sum = builder.add(cast, one, span)?;
            builder.ret([sum], span)?;
        }

        let findings = module_advice_taint_findings(&test)?;
        assert_eq!(sink_names(&findings), ["arith.add"]);

        Ok(())
    }

    /// Arguments of an indirect call flow into the tag-matching callee's parameters, so a raw
    /// advice value passed through the table is flagged at the sink inside the callee.
    #[test]
    fn indirect_call_propagates_argument_taint_into_callee() -> Result<(), midenc_hir::Report> {
        let span = SourceSpan::UNKNOWN;
        let mut test = Test::named("indirect_argument_taint").in_module("m");

        // The callee sinks its parameter into range-constrained arithmetic
        let sinkhole = test.define_function("sinkhole", &[Type::U32], &[Type::U32]);
        {
            let mut builder = FunctionBuilder::new(sinkhole, test.builder_mut());
            let param = builder.entry_block().borrow().arguments()[0] as ValueRef;
            let one = builder.u32(1, span);
            let sum = builder.add(param, one, span)?;
            builder.ret([sum], span)?;
        }
        let table = define_table(&mut test, &[(1, sinkhole, 1)]);

        let signature = Signature::new(&test.context_rc(), [Type::U32], [Type::U32]);
        let dispatch = test.define_function("dispatch", &[Type::U32], &[Type::U32]);
        {
            let mut builder = FunctionBuilder::new(dispatch, test.builder_mut());
            let index = builder.entry_block().borrow().arguments()[0] as ValueRef;
            let advice = builder.advice_pop(span)?;
            let argument = builder.unrealized_conversion_cast(advice, Type::U32, span)?;
            let call = builder.exec_indirect(table, signature, 1, index, [argument], span)?;
            let result = {
                let call = call.borrow();
                let results = call.results();
                let result = results.iter().next().unwrap();
                result.borrow().as_value_ref()
            };
            builder.ret([result], span)?;
        }

        let findings = module_advice_taint_findings(&test)?;
        assert_eq!(sink_names(&findings), ["arith.add"]);

        Ok(())
    }

    /// The call result joins the taint of every tag-matching entry, so one raw-advice callee
    /// among several possible callees is enough to flag the sink.
    #[test]
    fn indirect_call_joins_taint_across_multiple_callees() -> Result<(), midenc_hir::Report> {
        let mut test = Test::named("indirect_multi_callee").in_module("m");
        let clean_source = define_clean_source(&mut test);
        let raw_source = define_raw_advice_source(&mut test);
        let table = define_table(&mut test, &[(1, clean_source, 1), (2, raw_source, 1)]);
        define_dispatcher_sinking_result(&mut test, table, /*type_tag=*/ 1);

        let findings = module_advice_taint_findings(&test)?;
        assert_eq!(sink_names(&findings), ["arith.add"]);

        Ok(())
    }

    /// A later entry overwrites an earlier one at the same slot, so only the last entry is
    /// dispatchable: the shadowed raw-advice callee must not contribute taint.
    #[test]
    fn indirect_call_dispatches_only_last_entry_per_slot() -> Result<(), midenc_hir::Report> {
        let mut test = Test::named("indirect_slot_overwrite").in_module("m");
        let raw_source = define_raw_advice_source(&mut test);
        let clean_source = define_clean_source(&mut test);
        let table = define_table(&mut test, &[(1, raw_source, 1), (1, clean_source, 1)]);
        define_dispatcher_sinking_result(&mut test, table, /*type_tag=*/ 1);

        let findings = module_advice_taint_findings(&test)?;
        assert!(
            findings.is_empty(),
            "a shadowed entry is never dispatched, so its taint must not propagate: {findings:?}"
        );

        Ok(())
    }

    /// One unanalyzable entry makes the call results conservative, but arguments still flow into
    /// the analyzable entries, so a raw advice argument is flagged at the sink inside the bodied
    /// callee.
    #[test]
    fn indirect_call_with_mixed_targets_still_flows_arguments() -> Result<(), midenc_hir::Report> {
        let span = SourceSpan::UNKNOWN;
        let mut test = Test::named("indirect_mixed_targets").in_module("m");

        // A declaration: no body to analyze
        let extern_sink = test.define_function("extern_sink", &[Type::U32], &[Type::U32]);

        // The callee sinks its parameter into range-constrained arithmetic
        let sinkhole = test.define_function("sinkhole", &[Type::U32], &[Type::U32]);
        {
            let mut builder = FunctionBuilder::new(sinkhole, test.builder_mut());
            let param = builder.entry_block().borrow().arguments()[0] as ValueRef;
            let one = builder.u32(1, span);
            let sum = builder.add(param, one, span)?;
            builder.ret([sum], span)?;
        }
        let table = define_table(&mut test, &[(1, extern_sink, 1), (2, sinkhole, 1)]);

        let signature = Signature::new(&test.context_rc(), [Type::U32], [Type::U32]);
        let dispatch = test.define_function("dispatch", &[Type::U32], &[Type::U32]);
        {
            let mut builder = FunctionBuilder::new(dispatch, test.builder_mut());
            let index = builder.entry_block().borrow().arguments()[0] as ValueRef;
            let advice = builder.advice_pop(span)?;
            let argument = builder.unrealized_conversion_cast(advice, Type::U32, span)?;
            let call = builder.exec_indirect(table, signature, 1, index, [argument], span)?;
            let result = {
                let call = call.borrow();
                let results = call.results();
                let result = results.iter().next().unwrap();
                result.borrow().as_value_ref()
            };
            builder.ret([result], span)?;
        }

        let findings = module_advice_taint_findings(&test)?;
        assert_eq!(sink_names(&findings), ["arith.add"]);

        Ok(())
    }

    fn advice_taint_findings(test: &Test) -> Result<Vec<AdviceTaintFinding>, midenc_hir::Report> {
        let analysis_manager = AnalysisManager::new(test.function().as_operation_ref(), None);
        let analysis = analysis_manager.get_analysis::<AdviceTaintAnalysis>()?;
        Ok(analysis.findings().to_vec())
    }
}
