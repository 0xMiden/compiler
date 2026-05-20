mod lattice;

use midenc_hir::{
    AttributeRef, Forward, Immediate, Operation, ProgramPoint, Report, SmallVec, ValueRef,
    attributes::IntegerLikeAttr,
    traits::{InferIntRangeInterface, operation_result_value_range_refinement},
};
pub use midenc_hir::{ConstantIntRange, IntegerRangeConfidence, IntegerValueRange};

pub use self::lattice::IntegerRangeLattice;
use super::{DeadCodeAnalysis, SparseConstantPropagation, constant_propagation::ConstantValue};
use crate::{
    AnalysisState, AnalysisStateGuard, AnalysisStateGuardMut, BuildableDataFlowAnalysis,
    DataFlowSolver, Lattice, LatticeLike, SparseForwardDataFlowAnalysis, SparseLattice,
    sparse::SparseDataFlowAnalysis,
};

impl LatticeLike for IntegerValueRange {
    fn join(&self, other: &Self) -> Self {
        match (self.range().copied(), other.range().copied()) {
            (None, None) | (Some(_), None) => self.clone(),
            (None, Some(_)) => other.clone(),
            (Some(lhs), Some(rhs)) => Self::with_confidence(
                lhs.union(&rhs),
                join_confidence(self.confidence(), other.confidence()),
            ),
        }
    }

    fn meet(&self, other: &Self) -> Self {
        match (self.range().copied(), other.range().copied()) {
            (None, _) | (_, None) => Self::uninitialized(),
            (Some(lhs), Some(rhs)) => Self::with_confidence(
                lhs.intersection(&rhs),
                join_confidence(self.confidence(), other.confidence()),
            ),
        }
    }
}

fn join_confidence(
    lhs: Option<IntegerRangeConfidence>,
    rhs: Option<IntegerRangeConfidence>,
) -> IntegerRangeConfidence {
    match (lhs, rhs) {
        (Some(IntegerRangeConfidence::Proven), Some(IntegerRangeConfidence::Proven)) => {
            IntegerRangeConfidence::Proven
        }
        _ => IntegerRangeConfidence::Assumed,
    }
}

/// Sparse forward integer range analysis.
#[derive(Default)]
pub struct IntegerRangeAnalysis;

impl BuildableDataFlowAnalysis for IntegerRangeAnalysis {
    type Strategy = SparseDataFlowAnalysis<Self, Forward>;

    fn new(solver: &mut DataFlowSolver) -> Self {
        solver.load::<DeadCodeAnalysis>();
        solver.load::<SparseConstantPropagation>();
        Self
    }
}

impl SparseForwardDataFlowAnalysis for IntegerRangeAnalysis {
    type Lattice = IntegerRangeLattice;

    fn debug_name(&self) -> &'static str {
        "integer-range-analysis"
    }

    fn visit_operation(
        &self,
        op: &Operation,
        operands: &[AnalysisStateGuard<'_, Self::Lattice>],
        results: &mut [AnalysisStateGuardMut<'_, Self::Lattice>],
        solver: &mut DataFlowSolver,
    ) -> Result<(), Report> {
        log::debug!(target: self.debug_name(), "visiting operation {op}");

        if let Some(inferrable) = op.as_trait::<dyn InferIntRangeInterface>() {
            return self
                .visit_range_inferrable_operation(op, inferrable, operands, results, solver);
        }

        for (index, result_lattice) in results.iter_mut().enumerate() {
            let result = op.results()[index].borrow().as_value_ref();

            if let Some(constant_range) = constant_range_from_solver(result, op, solver) {
                result_lattice.join(&constant_range);
                continue;
            }

            if let Some(refined_range) = refined_result_range(op, result, operands) {
                result_lattice.join(&refined_range);
                continue;
            }

            self.set_to_entry_state(result_lattice);
        }

        Ok(())
    }

    fn set_to_entry_state(&self, lattice: &mut AnalysisStateGuardMut<'_, Self::Lattice>) {
        let Some(value) = lattice.anchor().as_value() else {
            return;
        };
        let ty = value.borrow().ty().clone();
        let entry_state = IntegerValueRange::max_for_type(&ty);
        log::trace!(
            target: self.debug_name(),
            "setting integer range for {} to entry state {entry_state}",
            lattice.anchor()
        );
        lattice.join(&entry_state);
    }
}

impl IntegerRangeAnalysis {
    fn visit_range_inferrable_operation(
        &self,
        op: &Operation,
        inferrable: &dyn InferIntRangeInterface,
        operands: &[AnalysisStateGuard<'_, <Self as SparseForwardDataFlowAnalysis>::Lattice>],
        results: &mut [AnalysisStateGuardMut<
            '_,
            <Self as SparseForwardDataFlowAnalysis>::Lattice,
        >],
        solver: &mut DataFlowSolver,
    ) -> Result<(), Report> {
        let operand_ranges = operands
            .iter()
            .map(|operand| operand.value().clone())
            .collect::<SmallVec<[_; 4]>>();
        let has_uninitialized_operand =
            operand_ranges.iter().any(IntegerValueRange::is_uninitialized);
        let mut inferred = SmallVec::<[bool; 4]>::with_capacity(results.len());
        inferred.resize(results.len(), false);
        let mut join_result_range = |value: ValueRef, range: IntegerValueRange| {
            let Some(index) = op
                .results()
                .all()
                .iter()
                .position(|result| result.borrow().as_value_ref() == value)
            else {
                return;
            };
            inferred[index] = true;
            results[index].join(&range);
        };

        inferrable.infer_result_ranges(&operand_ranges, &mut join_result_range);

        for (index, result_lattice) in results.iter_mut().enumerate() {
            if inferred[index] {
                continue;
            }

            let result = op.results()[index].borrow().as_value_ref();
            if let Some(constant_range) = constant_range_from_solver(result, op, solver) {
                result_lattice.join(&constant_range);
            } else if !has_uninitialized_operand {
                self.set_to_entry_state(result_lattice);
            }
        }

        Ok(())
    }
}

/// Return the currently known integer range for `value`, if available.
pub fn value_range(solver: &DataFlowSolver, value: ValueRef) -> Option<IntegerValueRange> {
    solver
        .get::<IntegerRangeLattice, _>(&value)
        .map(|range| range.value().clone())
        .filter(|range| !range.is_uninitialized())
}

/// Return true if `value` is known to be non-negative from a signed perspective.
pub fn statically_non_negative(solver: &DataFlowSolver, value: ValueRef) -> bool {
    value_range(solver, value)
        .and_then(|range| range.range().copied())
        .is_some_and(|range| range.is_statically_non_negative())
}

fn constant_range_from_solver(
    result: ValueRef,
    op: &Operation,
    solver: &mut DataFlowSolver,
) -> Option<IntegerValueRange> {
    let constant = solver.require::<Lattice<ConstantValue>, _>(result, ProgramPoint::after(op));
    if constant.value().is_uninitialized() {
        return None;
    }

    constant
        .value()
        .constant_value()
        .and_then(immediate_from_attribute)
        .map(IntegerValueRange::constant)
}

fn immediate_from_attribute(attr: AttributeRef) -> Option<Immediate> {
    let attr = attr.borrow();
    attr.as_attr()
        .as_trait::<dyn IntegerLikeAttr>()
        .map(|integer| integer.as_immediate())
}

fn refined_result_range(
    op: &Operation,
    result: ValueRef,
    operands: &[AnalysisStateGuard<'_, IntegerRangeLattice>],
) -> Option<IntegerValueRange> {
    let refinement = operation_result_value_range_refinement(op, result)?;
    let operand_range = op
        .operands()
        .iter()
        .position(|operand| operand.borrow().as_value_ref() == refinement.input)
        .and_then(|index| operands.get(index))
        .map(|range| range.value().clone())
        .unwrap_or_else(IntegerValueRange::uninitialized);

    Some(operand_range.constrained_by(&refinement.constraint))
}

#[cfg(test)]
mod tests {
    use midenc_dialect_arith::ArithOpBuilder;
    use midenc_dialect_hir::HirOpBuilder;
    use midenc_dialect_scf::StructuredControlFlowOpBuilder;
    use midenc_hir::{
        Felt, Op, SourceSpan, Type, dialects::builtin::BuiltinOpBuilder, pass::AnalysisManager,
        testing::Test,
    };

    use super::*;
    use crate::{DataFlowConfig, analyses::dce::DeadCodeAnalysis};

    #[test]
    fn constants_get_exact_ranges() -> Result<(), Report> {
        let mut test = Test::new("constants_get_exact_ranges", &[], &[Type::U32]);
        let value = {
            let mut builder = test.function_builder();
            let value = builder.u32(42, SourceSpan::UNKNOWN);
            builder.ret([value], SourceSpan::UNKNOWN)?;
            value
        };

        let solver = run_integer_range_analysis(&test)?;
        let range = range_for(&solver, value);
        assert_eq!(range.umin(), 42);
        assert_eq!(range.umax(), 42);
        assert_eq!(range.smin(), 42);
        assert_eq!(range.smax(), 42);
        assert!(statically_non_negative(&solver, value));
        assert!(value_range(&solver, value).expect("expected integer range").is_proven());

        Ok(())
    }

    #[test]
    fn checked_assertion_refines_to_u32() -> Result<(), Report> {
        let mut test = Test::new("checked_assertion_refines_to_u32", &[Type::Felt], &[Type::U32]);
        let asserted = {
            let raw = test.entry_block().borrow().arguments()[0].borrow().as_value_ref();
            let mut builder = test.function_builder();
            let asserted = builder.assert_u32(raw, SourceSpan::UNKNOWN)?;
            builder.ret([asserted], SourceSpan::UNKNOWN)?;
            asserted
        };

        let solver = run_integer_range_analysis(&test)?;
        let range = range_for(&solver, asserted);
        assert_eq!(range.umin(), 0);
        assert_eq!(range.umax(), u32::MAX as u128);
        assert_eq!(range.smin(), i32::MIN as i128);
        assert_eq!(range.smax(), i32::MAX as i128);
        assert!(value_range(&solver, asserted).expect("expected integer range").is_proven());

        Ok(())
    }

    #[test]
    fn unrealized_conversion_cast_preserves_input_range() -> Result<(), Report> {
        let mut test =
            Test::new("unrealized_conversion_cast_preserves_input_range", &[], &[Type::U32]);
        let cast = {
            let mut builder = test.function_builder();
            let raw =
                builder.felt(Felt::new(u64::from(u32::MAX) + 1).unwrap(), SourceSpan::UNKNOWN);
            let cast = builder.unrealized_conversion_cast(raw, Type::U32, SourceSpan::UNKNOWN)?;
            builder.ret([cast], SourceSpan::UNKNOWN)?;
            cast
        };

        let solver = run_integer_range_analysis(&test)?;
        let range = range_for(&solver, cast);
        assert_eq!(range.umin(), u64::from(u32::MAX) as u128 + 1);
        assert_eq!(range.umax(), u64::from(u32::MAX) as u128 + 1);

        Ok(())
    }

    #[test]
    fn inferrable_add_uses_operand_ranges() -> Result<(), Report> {
        let mut test = Test::new("inferrable_add_uses_operand_ranges", &[], &[Type::U32]);
        let sum = {
            let mut builder = test.function_builder();
            let lhs = builder.u32(1, SourceSpan::UNKNOWN);
            let rhs = builder.u32(2, SourceSpan::UNKNOWN);
            let sum = builder.add(lhs, rhs, SourceSpan::UNKNOWN)?;
            builder.ret([sum], SourceSpan::UNKNOWN)?;
            sum
        };

        let solver = run_integer_range_analysis(&test)?;
        let range = range_for(&solver, sum);
        assert_eq!(range.umin(), 3);
        assert_eq!(range.umax(), 3);

        Ok(())
    }

    #[test]
    fn comparisons_infer_exact_true_values() -> Result<(), Report> {
        let mut test = Test::new(
            "comparisons_infer_exact_true_values",
            &[],
            &[Type::I1, Type::I1, Type::I1, Type::I1, Type::I1, Type::I1],
        );
        let comparisons = {
            let mut builder = test.function_builder();
            let lt_lhs = builder.u32(1, SourceSpan::UNKNOWN);
            let lt_rhs = builder.u32(2, SourceSpan::UNKNOWN);
            let lt = builder.lt(lt_lhs, lt_rhs, SourceSpan::UNKNOWN)?;
            let lte_lhs = builder.u32(2, SourceSpan::UNKNOWN);
            let lte_rhs = builder.u32(2, SourceSpan::UNKNOWN);
            let lte = builder.lte(lte_lhs, lte_rhs, SourceSpan::UNKNOWN)?;
            let gt_lhs = builder.u32(3, SourceSpan::UNKNOWN);
            let gt_rhs = builder.u32(2, SourceSpan::UNKNOWN);
            let gt = builder.gt(gt_lhs, gt_rhs, SourceSpan::UNKNOWN)?;
            let gte_lhs = builder.u32(2, SourceSpan::UNKNOWN);
            let gte_rhs = builder.u32(2, SourceSpan::UNKNOWN);
            let gte = builder.gte(gte_lhs, gte_rhs, SourceSpan::UNKNOWN)?;
            let eq_lhs = builder.u32(5, SourceSpan::UNKNOWN);
            let eq_rhs = builder.u32(5, SourceSpan::UNKNOWN);
            let eq = builder.eq(eq_lhs, eq_rhs, SourceSpan::UNKNOWN)?;
            let neq_lhs = builder.u32(5, SourceSpan::UNKNOWN);
            let neq_rhs = builder.u32(6, SourceSpan::UNKNOWN);
            let neq = builder.neq(neq_lhs, neq_rhs, SourceSpan::UNKNOWN)?;
            let comparisons = [lt, lte, gt, gte, eq, neq];
            builder.ret(comparisons, SourceSpan::UNKNOWN)?;
            comparisons
        };

        let solver = run_integer_range_analysis(&test)?;
        for comparison in comparisons {
            assert_bool_range(&solver, comparison, true);
        }

        Ok(())
    }

    #[test]
    fn comparisons_infer_exact_false_values() -> Result<(), Report> {
        let mut test = Test::new(
            "comparisons_infer_exact_false_values",
            &[],
            &[Type::I1, Type::I1, Type::I1, Type::I1, Type::I1, Type::I1],
        );
        let comparisons = {
            let mut builder = test.function_builder();
            let lt_lhs = builder.u32(2, SourceSpan::UNKNOWN);
            let lt_rhs = builder.u32(1, SourceSpan::UNKNOWN);
            let lt = builder.lt(lt_lhs, lt_rhs, SourceSpan::UNKNOWN)?;
            let lte_lhs = builder.u32(3, SourceSpan::UNKNOWN);
            let lte_rhs = builder.u32(2, SourceSpan::UNKNOWN);
            let lte = builder.lte(lte_lhs, lte_rhs, SourceSpan::UNKNOWN)?;
            let gt_lhs = builder.u32(2, SourceSpan::UNKNOWN);
            let gt_rhs = builder.u32(3, SourceSpan::UNKNOWN);
            let gt = builder.gt(gt_lhs, gt_rhs, SourceSpan::UNKNOWN)?;
            let gte_lhs = builder.u32(2, SourceSpan::UNKNOWN);
            let gte_rhs = builder.u32(3, SourceSpan::UNKNOWN);
            let gte = builder.gte(gte_lhs, gte_rhs, SourceSpan::UNKNOWN)?;
            let eq_lhs = builder.u32(5, SourceSpan::UNKNOWN);
            let eq_rhs = builder.u32(6, SourceSpan::UNKNOWN);
            let eq = builder.eq(eq_lhs, eq_rhs, SourceSpan::UNKNOWN)?;
            let neq_lhs = builder.u32(5, SourceSpan::UNKNOWN);
            let neq_rhs = builder.u32(5, SourceSpan::UNKNOWN);
            let neq = builder.neq(neq_lhs, neq_rhs, SourceSpan::UNKNOWN)?;
            let comparisons = [lt, lte, gt, gte, eq, neq];
            builder.ret(comparisons, SourceSpan::UNKNOWN)?;
            comparisons
        };

        let solver = run_integer_range_analysis(&test)?;
        for comparison in comparisons {
            assert_bool_range(&solver, comparison, false);
        }

        Ok(())
    }

    #[test]
    fn comparisons_use_signed_bounds_for_signed_types() -> Result<(), Report> {
        let mut test =
            Test::new("comparisons_use_signed_bounds_for_signed_types", &[], &[Type::I1, Type::I1]);
        let (lt, gt) = {
            let mut builder = test.function_builder();
            let minus_one = builder.i32(-1, SourceSpan::UNKNOWN);
            let one = builder.i32(1, SourceSpan::UNKNOWN);
            let lt = builder.lt(minus_one, one, SourceSpan::UNKNOWN)?;
            let minus_one = builder.i32(-1, SourceSpan::UNKNOWN);
            let one = builder.i32(1, SourceSpan::UNKNOWN);
            let gt = builder.gt(minus_one, one, SourceSpan::UNKNOWN)?;
            builder.ret([lt, gt], SourceSpan::UNKNOWN)?;
            (lt, gt)
        };

        let solver = run_integer_range_analysis(&test)?;
        assert_bool_range(&solver, lt, true);
        assert_bool_range(&solver, gt, false);

        Ok(())
    }

    #[test]
    fn comparisons_remain_unknown_when_ranges_overlap() -> Result<(), Report> {
        let mut test =
            Test::new("comparisons_remain_unknown_when_ranges_overlap", &[Type::U32], &[Type::I1]);
        let comparison = {
            let arg = test.entry_block().borrow().arguments()[0].borrow().as_value_ref();
            let mut builder = test.function_builder();
            let limit = builder.u32(10, SourceSpan::UNKNOWN);
            let comparison = builder.lt(arg, limit, SourceSpan::UNKNOWN)?;
            builder.ret([comparison], SourceSpan::UNKNOWN)?;
            comparison
        };

        let solver = run_integer_range_analysis(&test)?;
        let range = range_for(&solver, comparison);
        assert_eq!(range.umin(), 0);
        assert_eq!(range.umax(), 1);
        assert!(value_range(&solver, comparison).expect("expected integer range").is_proven());

        Ok(())
    }

    #[test]
    fn unknown_ops_fall_back_to_type_range() -> Result<(), Report> {
        let mut test = Test::new("unknown_ops_fall_back_to_type_range", &[], &[Type::Felt]);
        let value = {
            let mut builder = test.function_builder();
            let base = builder.felt(Felt::new(2).unwrap(), SourceSpan::UNKNOWN);
            let exp = builder.felt(Felt::new(3).unwrap(), SourceSpan::UNKNOWN);
            let value = builder.exp(base, exp, SourceSpan::UNKNOWN)?;
            builder.ret([value], SourceSpan::UNKNOWN)?;
            value
        };

        let solver = run_integer_range_analysis(&test)?;
        let range = range_for(&solver, value);
        assert_eq!(range.umin(), 0);
        assert_eq!(range.umax(), (Felt::ORDER - 1) as u128);
        assert!(!value_range(&solver, value).expect("expected integer range").is_proven());

        Ok(())
    }

    #[test]
    fn region_branch_results_join_ranges() -> Result<(), Report> {
        let mut test = Test::new("region_branch_results_join_ranges", &[Type::I1], &[Type::U32]);
        let if_result = {
            let cond = test.entry_block().borrow().arguments()[0].borrow().as_value_ref();
            let mut builder = test.function_builder();
            let entry = builder.current_block();
            let if_op = builder.r#if(cond, &[Type::U32], SourceSpan::UNKNOWN)?;
            let if_result = if_op.as_operation_ref().borrow().results()[0] as ValueRef;

            let (then_region, else_region) = {
                let if_op = if_op.borrow();
                (if_op.then_body().as_region_ref(), if_op.else_body().as_region_ref())
            };

            let then_block = builder.create_block_in_region(then_region);
            builder.switch_to_block(then_block);
            let one = builder.u32(1, SourceSpan::UNKNOWN);
            builder.r#yield([one], SourceSpan::UNKNOWN)?;

            let else_block = builder.create_block_in_region(else_region);
            builder.switch_to_block(else_block);
            let five = builder.u32(5, SourceSpan::UNKNOWN);
            builder.r#yield([five], SourceSpan::UNKNOWN)?;

            builder.switch_to_block(entry);
            builder.ret([if_result], SourceSpan::UNKNOWN)?;
            if_result
        };

        let solver = run_integer_range_analysis(&test)?;
        let range = range_for(&solver, if_result);
        assert_eq!(range.umin(), 1);
        assert_eq!(range.umax(), 5);
        assert_eq!(range.smin(), 1);
        assert_eq!(range.smax(), 5);

        Ok(())
    }

    #[test]
    fn regression_unchecked_overflow_does_not_prove_the_result_type() -> Result<(), Report> {
        let mut test = Test::new("unchecked_overflow", &[], &[Type::U32]);
        let sum = {
            let span = SourceSpan::UNKNOWN;
            let mut builder = test.function_builder();
            let lhs = builder.u32(u32::MAX, span);
            let rhs = builder.u32(1, span);
            let sum = builder.add_unchecked(lhs, rhs, span)?;
            builder.ret([sum], span)?;
            sum
        };
        let solver = run_integer_range_analysis(&test)?;
        assert!(!value_range(&solver, sum).expect("expected integer range").is_proven());
        Ok(())
    }

    #[test]
    fn regression_signed_comparison_does_not_reuse_wider_signed_bounds() -> Result<(), Report> {
        let mut test = Test::new("signed_cast_comparison", &[], &[Type::I1]);
        let comparison = {
            let span = SourceSpan::UNKNOWN;
            let mut builder = test.function_builder();
            let lhs = builder.felt(Felt::new(u64::from(u32::MAX)).unwrap(), span);
            let rhs = builder.felt(Felt::new(1).unwrap(), span);
            let lhs = builder.unrealized_conversion_cast(lhs, Type::I32, span)?;
            let rhs = builder.unrealized_conversion_cast(rhs, Type::I32, span)?;
            let comparison = builder.lt(lhs, rhs, span)?;
            builder.ret([comparison], span)?;
            comparison
        };
        let solver = run_integer_range_analysis(&test)?;
        // In i32, 0xffffffff is -1, so a false-only range is unsound.
        assert_eq!(range_for(&solver, comparison).umax(), 1);
        Ok(())
    }

    #[test]
    fn regression_loop_ranges_converge_within_solver_budget() -> Result<(), Report> {
        let span = SourceSpan::UNKNOWN;
        let mut test = Test::new("incrementing_loop", &[Type::I1], &[Type::U32]);
        let loop_result = {
            let cond = test.entry_block().borrow().arguments()[0].borrow().as_value_ref();
            let mut builder = test.function_builder();
            let entry = builder.current_block();
            let zero = builder.u32(0, span);
            let loop_op = builder.r#while([zero], &[Type::U32], span)?;
            let before = loop_op.borrow().before().entry().as_block_ref();
            let after = loop_op.borrow().after().entry().as_block_ref();
            builder.switch_to_block(before);
            let current = before.borrow().arguments()[0].borrow().as_value_ref();
            builder.condition(cond, [current], span)?;
            builder.switch_to_block(after);
            let current = after.borrow().arguments()[0].borrow().as_value_ref();
            let one = builder.u32(1, span);
            let next = builder.add(current, one, span)?;
            builder.r#yield([next], span)?;
            builder.switch_to_block(entry);
            let result = loop_op.borrow().results()[0].borrow().as_value_ref();
            builder.ret([result], span)?;
            result
        };
        let solver = run_integer_range_analysis(&test)?;
        assert!(range_for(&solver, loop_result).umax() >= 100);
        Ok(())
    }

    fn run_integer_range_analysis(test: &Test) -> Result<DataFlowSolver, Report> {
        let mut config = DataFlowConfig::new();
        config.set_interprocedural(true);
        config.set_max_worklist_iterations(Some(1024));
        let mut solver = DataFlowSolver::new(config);
        solver.load::<DeadCodeAnalysis>();
        solver.load::<IntegerRangeAnalysis>();
        let op = test.function().as_operation_ref();
        solver.initialize_and_run(&op.borrow(), AnalysisManager::new(op, None))?;
        Ok(solver)
    }

    fn range_for(solver: &DataFlowSolver, value: ValueRef) -> ConstantIntRange {
        value_range(solver, value)
            .and_then(|range| range.range().copied())
            .expect("expected integer range")
    }

    fn assert_bool_range(solver: &DataFlowSolver, value: ValueRef, expected: bool) {
        let value_range = value_range(solver, value).expect("expected integer range");
        assert!(value_range.is_proven());
        let range = value_range.range().copied().expect("expected integer range");
        let expected = u128::from(expected);
        assert_eq!(range.umin(), expected);
        assert_eq!(range.umax(), expected);
    }
}
