use midenc_hir::{CallOpInterface, Forward, Operation, Report, Spanned, Value};
use midenc_hir_analysis::{
    AnalysisStateGuard, AnalysisStateGuardMut, BuildableDataFlowAnalysis, CallControlFlowAction,
    DataFlowSolver, SparseForwardDataFlowAnalysis, SparseLattice,
    analyses::{DeadCodeAnalysis, SparseConstantPropagation},
    sparse::SparseDataFlowAnalysis,
};

use super::{
    lattice::{AdviceTaintSparseLattice, CallContextFrame, ContextualAdviceTaintValue},
    sinks::{is_u32_presuming_sink, is_unconstrained_external_result_type},
};
use crate::{AdviceLoadWord, AdvicePipe, AdvicePop, AssertU32};

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
        let result_taint = transfer_taint(op, operand_taint);
        join_results(results, &result_taint)
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
                for (result_value, result) in call.as_operation().results().all().iter().zip(after)
                {
                    let result_value = result_value.borrow();
                    let value = if is_unconstrained_external_result_type(result_value.ty()) {
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

fn join_results(
    results: &mut [AnalysisStateGuardMut<'_, AdviceTaintSparseLattice>],
    value: &ContextualAdviceTaintValue,
) -> Result<(), Report> {
    for result in results {
        result.join(value);
    }
    Ok(())
}

fn join_advice_pipe_results(
    op: &Operation,
    operands: &[AnalysisStateGuard<'_, AdviceTaintSparseLattice>],
    results: &mut [AnalysisStateGuardMut<'_, AdviceTaintSparseLattice>],
) -> Result<(), Report> {
    const RAW_ADVICE_RESULTS: usize = 8;

    for (index, result) in results.iter_mut().enumerate() {
        let taint = if index < RAW_ADVICE_RESULTS {
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
) -> ContextualAdviceTaintValue {
    if op.is::<AdvicePop>() || op.is::<AdviceLoadWord>() {
        return ContextualAdviceTaintValue::raw(op.span());
    }

    if op.is::<AssertU32>() {
        return ContextualAdviceTaintValue::clean();
    }

    if is_u32_presuming_sink(op) && operand_taint.has_unreported_origin() {
        operand_taint.mark_reported()
    } else {
        operand_taint
    }
}
