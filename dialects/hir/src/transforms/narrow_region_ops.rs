//! Narrowing structured control flow ops whose forwarded values do not fit the operand stack.
//!
//! Every value an `scf.while` forwards through its `scf.condition`, and every result an `scf.if`
//! or `scf.index_switch` yields, is on the operand stack at once at the region boundary, so the
//! MASM backend can only lower the op if they fit the addressable window. The spill analysis
//! rejects an op whose forwarded values exceed it, because spilling can only evict values the op
//! does not itself use.
//!
//! This pass runs before that analysis and moves the excess to procedure locals: a demoted value
//! is stored to a local where it would have been forwarded and loaded again where it would have
//! been used, and the op is rebuilt without it. The loads and stores are then ordinary operations
//! to the spill analysis.

use alloc::{rc::Rc, vec::Vec};

use midenc_dialect_scf::{
    self as scf,
    rebuild::{IterArg, keep_map, rebuild_if, rebuild_index_switch, rebuild_while},
};
use midenc_hir::{
    BlockRef, EntityMut, Op, Operation, OperationName, OperationRef, ProgramPoint, Report,
    Rewriter, SmallVec, SourceSpan, Spanned, Symbol, ValueRef,
    dialects::builtin::{Function, FunctionRef, attributes::LocalVariable},
    pass::{Pass, PassExecutionState, PostPassStatus},
    patterns::{RewriterExt, RewriterImpl, TracingRewriterListener},
};
use midenc_hir_analysis::analyses::{
    LivenessAnalysis,
    spills::{K, region_results_demand},
};

use crate::HirOpBuilder;

/// Moves forwarded values of structured control flow ops that do not fit the operand stack window
/// to procedure locals, so that the spill analysis can place the remaining operand stack traffic.
///
/// Scheduled right before the post-lift `transform-spills`; before the lift there are no such ops.
#[derive(Default)]
pub struct NarrowRegionOps;

midenc_hir::inventory::submit!(::midenc_hir::pass::registry::PassInfo::new::<NarrowRegionOps>(
    "narrow-region-ops",
    "move forwarded values of structured control flow ops that do not fit the operand stack \
     window to procedure locals"
));

impl Pass for NarrowRegionOps {
    type Target = Function;

    fn name(&self) -> &'static str {
        "narrow-region-ops"
    }

    fn argument(&self) -> &'static str {
        "narrow-region-ops"
    }

    fn can_schedule_on(&self, _name: &OperationName) -> bool {
        true
    }

    fn initialize(&mut self, context: Rc<midenc_hir::Context>) -> Result<(), Report> {
        context.get_or_register_dialect::<crate::HirDialect>();
        context.get_or_register_dialect::<scf::ScfDialect>();

        Ok(())
    }

    fn run_on_operation(
        &mut self,
        op: EntityMut<'_, Self::Target>,
        state: &mut PassExecutionState,
    ) -> Result<(), Report> {
        let function = op.into_entity_ref();

        if function.is_declaration() {
            state.preserved_analyses_mut().preserve_all();
            state.set_post_pass_status(PostPassStatus::Unchanged);
            return Ok(());
        }

        // Inner ops first: narrowing an op leaves the ops nested in its regions untouched, while
        // the loads it inserts change what the enclosing op has to forward.
        let mut candidates = Vec::new();
        function.as_operation().postwalk_all(|op: &Operation| {
            if is_narrowable(op) && !fits(op) {
                candidates.push(op.as_operation_ref());
            }
        });
        if candidates.is_empty() {
            state.preserved_analyses_mut().preserve_all();
            state.set_post_pass_status(PostPassStatus::Unchanged);
            return Ok(());
        }

        // The liveness computed before any rewrite serves every candidate: a rewrite replaces
        // uses of an op's own forwarded values with loads, which does not move the next use of
        // anything an enclosing op still has to decide about, and the distances only order the
        // columns.
        let liveness = state.analysis_manager().get_analysis::<LivenessAnalysis>()?;
        let context = function.as_operation().context_rc();
        let mut rewriter = RewriterImpl::<TracingRewriterListener>::new(context)
            .with_listener(TracingRewriterListener);
        // The locals are allocated through a mutable borrow of the function below.
        let function_ref = function.as_function_ref();
        drop(function);
        let function = function_ref;

        for op in candidates {
            // Everything inserted for `op` describes the same source location as `op`.
            let mut scope = rewriter.with_inline_call_chain(op);
            if op.borrow().is::<scf::While>() {
                narrow_while(&mut *scope, function, &liveness, op)?;
            } else {
                narrow_results(&mut *scope, function, &liveness, op)?;
            }
        }

        state.set_post_pass_status(PostPassStatus::Changed);

        Ok(())
    }
}

/// Whether `op` is one of the ops this pass narrows.
fn is_narrowable(op: &Operation) -> bool {
    op.is::<scf::While>() || op.is::<scf::If>() || op.is::<scf::IndexSwitch>()
}

/// Whether the values `op` forwards already fit the operand stack window.
///
/// The results, with the condition flag of a loop-like op beside them, are bounded by the same
/// demand the spill analysis checks. An `scf.while` must also fit its inits, which the after
/// region yields again.
fn fits(op: &Operation) -> bool {
    if region_results_demand(op) > K {
        return false;
    }
    match op.downcast_ref::<scf::While>() {
        Some(while_op) => {
            felts(while_op.inits().into_iter().map(|o| o.borrow().as_value_ref())) <= K
        }
        None => true,
    }
}

/// The number of operand stack elements the given values occupy together.
fn felts(values: impl Iterator<Item = ValueRef>) -> usize {
    values.map(|v| v.borrow().ty().size_in_felts()).sum()
}

/// One value an op forwards, as a candidate for demotion.
///
/// The columns are demoted in the order of [sort_columns]: the ones that cost nothing first.
#[derive(Debug, Clone, Copy)]
struct Column {
    /// The position of the value among the forwarded values of its group.
    index: usize,
    /// Its size on the operand stack.
    size: usize,
    /// Neither the region-side value nor the result has real uses: the column is dropped without
    /// a store or a load.
    dead: bool,
    /// The region-side value it would replace has no real uses there, so no load is needed inside
    /// the region.
    unused_inside: bool,
    /// Demoting it keeps a pair together (its before block argument feeds a result column that is
    /// already demoted), so the two share one local.
    paired: bool,
    /// The distance to the next use of the region-side value at the entry of the region, or
    /// `u32::MAX` when it is not used there.
    next_use: u32,
}

/// Demotes forwarded values of an `scf.while` until both of its groups fit the window.
///
/// The result group (the values `scf.condition` forwards, the after block arguments and the
/// results) and the iter group (the inits, the before block arguments and the yielded values) are
/// kept consistent separately by the verifier, so they are narrowed independently.
fn narrow_while(
    rewriter: &mut dyn Rewriter,
    function: FunctionRef,
    liveness: &LivenessAnalysis,
    op: OperationRef,
) -> Result<(), Report> {
    let (before_block, after_block, cond_ref, yield_ref, inits, forwarded, span) = {
        let operation = op.borrow();
        let while_op = operation.downcast_ref::<scf::While>().unwrap();
        (
            while_op.before().entry_block_ref().unwrap(),
            while_op.after().entry_block_ref().unwrap(),
            while_op.condition_op().as_operation_ref(),
            while_op.yield_op().as_operation_ref(),
            while_op
                .inits()
                .into_iter()
                .map(|o| o.borrow().as_value_ref())
                .collect::<SmallVec<[ValueRef; 4]>>(),
            while_op
                .condition_op()
                .borrow()
                .forwarded()
                .into_iter()
                .map(|o| o.borrow().as_value_ref())
                .collect::<SmallVec<[ValueRef; 4]>>(),
            while_op.span(),
        )
    };
    let before_args = block_args(before_block);
    let after_args = block_args(after_block);
    let results = op.borrow().results().iter().map(|r| *r as ValueRef).collect::<Vec<_>>();

    // The result group, bounded by the demand of the results and the condition flag together.
    let mut demoted_results = Vec::new();
    let mut demand = region_results_demand(&op.borrow());
    if demand > K {
        let columns = columns(liveness, after_block, &after_args, |j, after_arg| {
            let dead = !after_arg.borrow().has_real_uses() && !results[j].borrow().has_real_uses();
            (dead, false)
        });
        for column in columns {
            if demand <= K {
                break;
            }
            demoted_results.push(column);
            demand -= column.size;
        }
    }

    // The iter group, bounded by the inits. A column whose before block argument feeds a demoted
    // result column is preferred, so the pair shares one local.
    let mut demoted_iters = Vec::new();
    let mut inits_size = felts(inits.iter().copied());
    if inits_size > K {
        let columns = columns(liveness, before_block, &before_args, |_, before_arg| {
            let paired = demoted_results
                .iter()
                .any(|column| !column.dead && forwarded[column.index] == before_arg);
            (false, paired)
        });
        for column in columns {
            if inits_size <= K {
                break;
            }
            demoted_iters.push(column);
            inits_size -= column.size;
        }
    }

    // Result columns: load at the start of the after region and after the loop. A dead column
    // needs no local at all.
    let mut slots = Vec::with_capacity(demoted_results.len());
    for column in demoted_results.iter() {
        let j = column.index;
        if column.dead {
            slots.push(None);
            continue;
        }
        let local = alloc_local(function, forwarded[j]);
        slots.push(Some(local));
        replace_with_load(
            rewriter,
            after_args[j],
            local,
            ProgramPoint::at_start_of(after_block),
            span,
        )?;
        replace_with_load(rewriter, results[j], local, ProgramPoint::after(op), span)?;
    }

    // Iter columns: store before the loop and before the yield, load at the start of the before
    // region. A paired column reuses the local of its result column, which then needs no store
    // before the condition: the slot already holds the value the init or the yield stored.
    let mut stored_by_pair = Vec::new();
    for column in demoted_iters.iter() {
        let i = column.index;
        let pair = demoted_results
            .iter()
            .zip(slots.iter())
            .find(|(result, _)| forwarded[result.index] == before_args[i])
            .and_then(|(_, slot)| *slot);
        let local = match pair {
            Some(local) => {
                stored_by_pair.push(local);
                local
            }
            None => alloc_local(function, inits[i]),
        };
        rewriter.set_insertion_point_before(op);
        rewriter.store_local(local, inits[i], span)?;
        replace_with_load(
            rewriter,
            before_args[i],
            local,
            ProgramPoint::at_start_of(before_block),
            span,
        )?;
        // The yielded value may itself have been replaced by a load above.
        let yielded = operand(yield_ref, i);
        rewriter.set_insertion_point_before(yield_ref);
        rewriter.store_local(local, yielded, span)?;
    }
    // The remaining result columns are stored before the condition.
    for (column, slot) in demoted_results.iter().zip(slots.iter()) {
        let Some(local) = slot else {
            continue;
        };
        if stored_by_pair.contains(local) {
            continue;
        }
        // The forwarded value may have been replaced by a load of an iter column's local above.
        let value = operand(cond_ref, 1 + column.index);
        rewriter.set_insertion_point_before(cond_ref);
        rewriter.store_local(*local, value, span)?;
    }

    let iter_args = (0..inits.len())
        .map(|i| {
            if demoted_iters.iter().any(|column| column.index == i) {
                IterArg::Remove(None)
            } else {
                IterArg::Keep
            }
        })
        .collect::<SmallVec<[IterArg; 4]>>();
    let results =
        keep_map(forwarded.len(), |j| !demoted_results.iter().any(|column| column.index == j));
    rebuild_while(rewriter, op, &iter_args, &results)?;

    Ok(())
}

/// Demotes results of an `scf.if` or `scf.index_switch` until they fit the window.
fn narrow_results(
    rewriter: &mut dyn Rewriter,
    function: FunctionRef,
    liveness: &LivenessAnalysis,
    op: OperationRef,
) -> Result<(), Report> {
    let (entry_blocks, results, span) = {
        let operation = op.borrow();
        (
            operation
                .regions()
                .iter()
                .map(|region| region.entry_block_ref().unwrap())
                .collect::<SmallVec<[BlockRef; 4]>>(),
            operation.results().iter().map(|r| *r as ValueRef).collect::<Vec<_>>(),
            operation.span(),
        )
    };

    // Every result is only used after the op: a dead one is dropped, the ones used latest go
    // first among the rest.
    let mut demoted = Vec::new();
    let mut demand = region_results_demand(&op.borrow());
    let mut columns = {
        let next_uses = liveness.next_uses_at(&ProgramPoint::after(op));
        results
            .iter()
            .enumerate()
            .map(|(index, result)| Column {
                index,
                size: result.borrow().ty().size_in_felts(),
                dead: !result.borrow().has_real_uses(),
                unused_inside: true,
                paired: false,
                next_use: next_uses.as_ref().map(|nu| nu.distance(*result)).unwrap_or(u32::MAX),
            })
            .collect::<Vec<_>>()
    };
    sort_columns(&mut columns);
    for column in columns {
        if demand <= K {
            break;
        }
        demoted.push(column);
        demand -= column.size;
    }

    for column in demoted.iter() {
        if column.dead {
            continue;
        }
        let j = column.index;
        let local = alloc_local(function, results[j]);
        for entry in entry_blocks.iter() {
            let terminator = entry.borrow().terminator().unwrap();
            if !terminator.borrow().is::<scf::Yield>() {
                // An `ub.unreachable` arm forwards nothing.
                continue;
            }
            let value = operand(terminator, j);
            rewriter.set_insertion_point_before(terminator);
            rewriter.store_local(local, value, span)?;
        }
        replace_with_load(rewriter, results[j], local, ProgramPoint::after(op), span)?;
    }

    let results = keep_map(results.len(), |j| !demoted.iter().any(|column| column.index == j));
    if !rebuild_if(rewriter, op, &results)? {
        rebuild_index_switch(rewriter, op, &results)?;
    }

    Ok(())
}

/// The columns of a region entry block, sorted for demotion (see [sort_columns]).
///
/// `classify` returns whether a column is dead and whether it is paired, see [Column]; the rest
/// is read from the argument and the liveness at the start of `block`.
fn columns(
    liveness: &LivenessAnalysis,
    block: BlockRef,
    args: &[ValueRef],
    classify: impl Fn(usize, ValueRef) -> (bool, bool),
) -> Vec<Column> {
    let next_uses = liveness.next_uses_at(&ProgramPoint::at_start_of(block));
    let mut columns = args
        .iter()
        .enumerate()
        .map(|(index, arg)| {
            let (dead, paired) = classify(index, *arg);
            Column {
                index,
                size: arg.borrow().ty().size_in_felts(),
                dead,
                unused_inside: !arg.borrow().has_real_uses(),
                paired,
                next_use: next_uses.as_ref().map(|nu| nu.distance(*arg)).unwrap_or(u32::MAX),
            }
        })
        .collect::<Vec<_>>();
    sort_columns(&mut columns);
    columns
}

/// Orders columns for demotion, cheapest first: dead columns (no store, no load), then the ones
/// unused inside the region (no load there), then the ones that complete a pair (one local for
/// both), then by decreasing distance to the next use, larger values first, lowest index first.
///
/// A value wider than the window cannot be made to fit by any choice; it is demoted like any
/// other and the spill analysis reports the op if the rest still does not fit.
fn sort_columns(columns: &mut [Column]) {
    columns.sort_by(|a, b| {
        b.dead
            .cmp(&a.dead)
            .then(b.unused_inside.cmp(&a.unused_inside))
            .then(b.paired.cmp(&a.paired))
            .then(b.next_use.cmp(&a.next_use))
            .then(b.size.cmp(&a.size))
            .then(a.index.cmp(&b.index))
    });
}

/// Allocates a procedure local of the type of `value`.
fn alloc_local(mut function: FunctionRef, value: ValueRef) -> LocalVariable {
    let ty = value.borrow().ty().clone();
    function.borrow_mut().alloc_local(ty)
}

/// Replaces every real use of `value` with a load of `local` inserted at `point`, if it has any;
/// the debug uses of a dropped value are converted when the op is rebuilt.
fn replace_with_load(
    rewriter: &mut dyn Rewriter,
    value: ValueRef,
    local: LocalVariable,
    point: ProgramPoint,
    span: SourceSpan,
) -> Result<(), Report> {
    if !value.borrow().has_real_uses() {
        return Ok(());
    }
    rewriter.set_insertion_point(point);
    let loaded = rewriter.load_local(local, span)?;
    rewriter.replace_all_uses_of_value_with(value, loaded);
    Ok(())
}

/// The arguments of `block` as values.
fn block_args(block: BlockRef) -> SmallVec<[ValueRef; 4]> {
    block.borrow().arguments().iter().map(|arg| *arg as ValueRef).collect()
}

/// The current value of operand `index` of `op`.
fn operand(op: OperationRef, index: usize) -> ValueRef {
    op.borrow().operands()[index].borrow().as_value_ref()
}
