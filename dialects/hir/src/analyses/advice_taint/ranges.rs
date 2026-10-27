use alloc::{rc::Rc, vec::Vec};

use midenc_hir::{
    Operation, Report, ValueRef,
    pass::AnalysisManager,
    traits::{ValueRangeConstraint, operation_operand_range_requirement},
};
use midenc_hir_analysis::{
    DataFlowConfig, DataFlowSolver,
    analyses::{IntegerRangeAnalysis, integer_range::value_range},
};

/// Immutable range facts shared by taint propagation and diagnostic collection.
///
/// A partially solved range can still widen when another predecessor arrives. Using it to
/// decide whether an origin has been reported would make taint transfer non-monotone.
/// Only a completed range solve can discharge a requirement; absent facts keep every check.
#[derive(Clone, Default)]
pub(super) struct RangeFacts(Option<Rc<DataFlowSolver>>);

impl RangeFacts {
    pub fn analyze(
        op: &Operation,
        analysis_manager: AnalysisManager,
        config: DataFlowConfig,
    ) -> Result<Self, Report> {
        let mut solver = DataFlowSolver::new(config);
        solver.load::<IntegerRangeAnalysis>();
        match solver.initialize_and_run(op, analysis_manager) {
            Ok(()) => Ok(Self(Some(Rc::new(solver)))),
            Err(err) if super::is_dataflow_budget_exhaustion(&err) => {
                log::debug!(
                    "integer range budget exhausted; retaining static advice checks: {err}"
                );
                Ok(Self::default())
            }
            Err(err) => Err(err),
        }
    }

    pub fn satisfies(&self, value: ValueRef, constraint: &ValueRangeConstraint) -> bool {
        self.0
            .as_ref()
            .and_then(|solver| value_range(solver, value))
            .is_some_and(|range| {
                range.is_proven() && range.range().is_some_and(|range| range.satisfies(constraint))
            })
    }

    pub fn unsatisfied_operands(&self, op: &Operation) -> Vec<usize> {
        op.operands()
            .iter()
            .enumerate()
            .filter_map(|(index, operand)| {
                let constraint = operation_operand_range_requirement(op, index)?;
                (!self.satisfies(operand.borrow().as_value_ref(), &constraint)).then_some(index)
            })
            .collect()
    }
}
