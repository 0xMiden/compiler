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
                for (result_index, (result_value, result)) in
                    call.as_operation().results().all().iter().zip(after).enumerate()
                {
                    let result_value = result_value.borrow();
                    let value =
                        if external_call_result_has_unconstrained_advice_effect(call, result_index)
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
    use alloc::{
        string::{String, ToString},
        vec::Vec,
    };

    use midenc_dialect_arith::ArithOpBuilder;
    use midenc_hir::{
        SourceSpan, Type, dialects::builtin::BuiltinOpBuilder, pass::AnalysisManager, testing::Test,
    };

    use super::super::{AdviceTaintAnalysis, AdviceTaintFinding};
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

    fn advice_taint_findings(test: &Test) -> Result<Vec<AdviceTaintFinding>, midenc_hir::Report> {
        let analysis_manager = AnalysisManager::new(test.function().as_operation_ref(), None);
        let analysis = analysis_manager.get_analysis::<AdviceTaintAnalysis>()?;
        Ok(analysis.findings().to_vec())
    }

    fn sink_names(findings: &[AdviceTaintFinding]) -> Vec<String> {
        findings.iter().map(|finding| finding.sink.to_string()).collect()
    }
}
