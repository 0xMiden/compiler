use alloc::{
    rc::Rc,
    string::{String, ToString},
};

use midenc_hir::{
    ConstantIntRange, FxHashMap, Operation, Report, ValueRef,
    pass::AnalysisManager,
    traits::{ValueRangeConstraint, operation_operand_range_requirement},
};
use midenc_hir_analysis::{
    DataFlowConfig, DataFlowSolver,
    analyses::{IntegerRangeAnalysis, integer_range::value_range},
};

/// Whether optional integer range inference completed before advice taint propagation.
///
/// Exhaustion reduces precision: static checks still cover every constrained use. It does not
/// imply incomplete taint coverage, which is reported separately by the taint solver.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum RangeAnalysisStatus {
    #[default]
    NotRun,
    Complete,
    BudgetExhausted {
        reason: String,
    },
}

/// Immutable proven bounds shared by taint propagation and diagnostic collection.
///
/// Only a completed range solve can discharge a requirement. Partial facts are discarded;
/// successful solves retain just the proven ranges of values used by the analysis root.
#[derive(Clone, Default)]
pub(super) struct RangeFacts {
    proven: Rc<FxHashMap<ValueRef, ConstantIntRange>>,
    status: RangeAnalysisStatus,
}

impl RangeFacts {
    pub fn analyze(
        op: &Operation,
        analysis_manager: AnalysisManager,
        config: DataFlowConfig,
    ) -> Result<Self, Report> {
        let mut solver = DataFlowSolver::new(config);
        solver.load::<IntegerRangeAnalysis>();
        match solver.initialize_and_run(op, analysis_manager) {
            Ok(()) => {
                let mut proven = FxHashMap::default();
                op.prewalk_all(|operation| {
                    for operand in operation.operands().iter() {
                        let value = operand.borrow().as_value_ref();
                        if let Some(range) =
                            value_range(&solver, value).filter(|range| range.is_proven())
                            && let Some(range) = range.range().copied()
                        {
                            proven.insert(value, range);
                        }
                    }
                });
                Ok(Self {
                    proven: Rc::new(proven),
                    status: RangeAnalysisStatus::Complete,
                })
            }
            Err(err) if super::is_dataflow_budget_exhaustion(&err) => Ok(Self {
                status: RangeAnalysisStatus::BudgetExhausted {
                    reason: err.to_string(),
                },
                ..Self::default()
            }),
            Err(err) => Err(err),
        }
    }

    pub fn status(&self) -> &RangeAnalysisStatus {
        &self.status
    }

    pub fn satisfies(&self, value: ValueRef, constraint: &ValueRangeConstraint) -> bool {
        self.proven.get(&value).is_some_and(|range| range.satisfies(constraint))
    }

    pub fn unsatisfied_operands<'a>(
        &'a self,
        op: &'a Operation,
    ) -> impl Iterator<Item = usize> + 'a {
        op.operands().iter().enumerate().filter_map(move |(index, operand)| {
            let constraint = operation_operand_range_requirement(op, index)?;
            (!self.satisfies(operand.borrow().as_value_ref(), &constraint)).then_some(index)
        })
    }
}
