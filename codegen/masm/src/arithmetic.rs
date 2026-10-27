use alloc::{rc::Rc, vec::Vec};

use midenc_dialect_arith::ArithOpBuilder;
use midenc_dialect_hir::HirOpBuilder;
use midenc_hir::{
    Context, EntityMut, Forward, Operation, OperationName, OperationRef, Overflow, RawWalk, Report,
    Rewriter, Spanned, Type, ValueRef,
    pass::{Pass, PassExecutionState, PostPassStatus},
    patterns::{
        FrozenRewritePatternSet, NoopRewriterListener, Pattern, PatternApplicationError,
        PatternApplicator, PatternBenefit, PatternInfo, PatternKind, RewritePattern,
        RewritePatternSet, RewriterImpl,
    },
};

use crate::{arith, cost::MasmArithmeticCostModel};

/// Select cheaper MASM arithmetic lowerings without changing overflow semantics.
#[derive(Default)]
pub struct OptimizeMasmArithmetic {
    costs: Option<MasmArithmeticCostModel>,
}

midenc_hir::inventory::submit!(
    ::midenc_hir::pass::registry::PassInfo::new::<OptimizeMasmArithmetic>(
        "masm-arithmetic",
        "select arithmetic using the Miden VM cost model"
    )
);

impl Pass for OptimizeMasmArithmetic {
    type Target = Operation;

    fn name(&self) -> &'static str {
        "masm-arithmetic"
    }

    fn can_schedule_on(&self, _name: &OperationName) -> bool {
        true
    }

    fn run_on_operation(
        &mut self,
        operation: EntityMut<'_, Operation>,
        state: &mut PassExecutionState,
    ) -> Result<(), Report> {
        let root = operation.as_operation_ref();
        let context = operation.context_rc();
        drop(operation);
        let mut candidates = Vec::new();
        root.raw_prewalk_all::<Forward, _>(|operation: OperationRef| {
            if wrapping_i32_mul(&operation.borrow()).is_some() {
                candidates.push(operation);
            }
        });
        if candidates.is_empty() {
            state.preserved_analyses_mut().preserve_all();
            state.set_post_pass_status(PostPassStatus::Unchanged);
            return Ok(());
        }
        let costs = match self.costs {
            Some(costs) => Some(costs),
            None => MasmArithmeticCostModel::load(context.clone())?,
        };
        self.costs = costs;
        let Some(costs) = costs else {
            state.preserved_analyses_mut().preserve_all();
            state.set_post_pass_status(PostPassStatus::Unchanged);
            return Ok(());
        };
        context.get_or_register_dialect::<midenc_dialect_hir::HirDialect>();
        let mut patterns = RewritePatternSet::new(context.clone());
        patterns.push(NormalizeWrappingMul::new(context.clone(), costs));
        let mut applicator =
            PatternApplicator::new(Rc::new(FrozenRewritePatternSet::new(patterns)));
        applicator.apply_default_cost_model();
        let mut rewriter = RewriterImpl::<NoopRewriterListener>::new(context);
        let mut changed = false;
        for candidate in candidates {
            match applicator.match_and_rewrite(
                candidate,
                &mut rewriter,
                |_| true,
                |_| {},
                |_| Ok(()),
            ) {
                Ok(()) => changed = true,
                Err(PatternApplicationError::NoMatchesFound) => (),
                Err(PatternApplicationError::Report(error)) => return Err(error),
            }
        }
        state.set_post_pass_status(PostPassStatus::from(changed));
        if !changed {
            state.preserved_analyses_mut().preserve_all();
        }
        Ok(())
    }
}

struct NormalizeWrappingMul {
    info: PatternInfo,
    costs: MasmArithmeticCostModel,
}

impl NormalizeWrappingMul {
    fn new(context: Rc<Context>, costs: MasmArithmeticCostModel) -> Self {
        let dialect = context.get_or_register_dialect::<arith::ArithDialect>();
        let operation = dialect.registered_name::<arith::Mul>().unwrap();
        let savings = costs.wrapping_mul_savings().unwrap_or(0);
        let benefit = u16::try_from(savings).unwrap_or(u16::MAX - 1).min(u16::MAX - 1);
        Self {
            info: PatternInfo::new(
                context,
                "normalize-wrapping-i32-mul",
                PatternKind::Operation(operation),
                PatternBenefit::new(benefit),
            ),
            costs,
        }
    }
}

impl Pattern for NormalizeWrappingMul {
    fn info(&self) -> &PatternInfo {
        &self.info
    }
}

impl RewritePattern for NormalizeWrappingMul {
    fn match_and_rewrite(
        &self,
        operation: OperationRef,
        rewriter: &mut dyn Rewriter,
    ) -> Result<bool, Report> {
        let Some((lhs, rhs)) = wrapping_i32_mul(&operation.borrow()) else {
            return Ok(false);
        };
        if self.costs.wrapping_mul_savings().is_none() {
            return Ok(false);
        }
        let span = operation.span();
        let lhs = rewriter.bitcast(lhs, Type::U32, span)?;
        let rhs = rewriter.bitcast(rhs, Type::U32, span)?;
        let product = rewriter.mul_wrapping(lhs, rhs, span)?;
        let product = rewriter.bitcast(product, Type::I32, span)?;
        rewriter.replace_op_with_values(operation, &[Some(product)]);
        Ok(true)
    }
}

fn wrapping_i32_mul(operation: &Operation) -> Option<(ValueRef, ValueRef)> {
    let multiply = operation.downcast_ref::<arith::Mul>()?;
    if *multiply.get_overflow() != Overflow::Wrapping || multiply.lhs().ty() != Type::I32 {
        return None;
    }
    let lhs = multiply.lhs().as_value_ref();
    let rhs = multiply.rhs().as_value_ref();
    Some((lhs, rhs))
}
