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
    rebuild::{IterArg, rebuild_if, rebuild_index_switch, rebuild_while},
};
use midenc_hir::{
    BlockRef, Builder, EntityMut, Op, Operation, OperationName, OperationRef, ProgramPoint, Report,
    Rewriter, SmallVec, SourceSpan, Spanned, Symbol, ValueRef,
    dialects::builtin::{Function, FunctionRef, attributes::LocalVariable},
    pass::{Pass, PassExecutionState, PostPassStatus},
    patterns::{RewriterImpl, TracingRewriterListener},
};
use midenc_hir_analysis::analyses::{LivenessAnalysis, spills::K};

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

        let liveness = state.analysis_manager().get_analysis::<LivenessAnalysis>()?;
        let context = function.as_operation().context_rc();
        let mut narrower = Narrower {
            function: function.as_function_ref(),
            liveness,
            rewriter: RewriterImpl::<TracingRewriterListener>::new(context)
                .with_listener(TracingRewriterListener),
        };
        drop(function);

        for op in candidates {
            narrower.narrow(op)?;
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
/// An `scf.while` must fit its inits (yielded again by the after region) and, together with the
/// condition flag, the values its `scf.condition` forwards; the MASM lowering schedules all of
/// them at once. An `scf.if` or `scf.index_switch` must fit its results.
fn fits(op: &Operation) -> bool {
    if let Some(while_op) = op.downcast_ref::<scf::While>() {
        let inits = felts(while_op.inits().into_iter().map(|o| o.borrow().as_value_ref()));
        let forwarded = felts(while_op.results().iter().map(|r| *r as ValueRef));
        // The condition flag is scheduled together with the forwarded values.
        inits <= K && forwarded < K
    } else {
        felts(op.results().iter().map(|r| *r as ValueRef)) <= K
    }
}

/// The number of operand stack elements the given values occupy together.
fn felts(values: impl Iterator<Item = ValueRef>) -> usize {
    values.map(|v| v.borrow().ty().size_in_felts()).sum()
}

/// The rewriting state of one run of the pass over a function.
struct Narrower {
    function: FunctionRef,
    liveness: Rc<LivenessAnalysis>,
    rewriter: RewriterImpl<TracingRewriterListener>,
}

/// One value an op forwards, as a candidate for demotion.
#[derive(Debug, Clone, Copy)]
struct Column {
    /// The position of the value among the forwarded values of its group.
    index: usize,
    /// Its size on the operand stack.
    size: usize,
    /// Whether demoting it costs nothing inside the regions: the region-side value it would
    /// replace has no real uses there.
    unused_inside: bool,
    /// The distance to the next use of the region-side value at the entry of the region, or
    /// `u32::MAX` when it is not used there.
    next_use: u32,
}

impl Narrower {
    fn narrow(&mut self, op: OperationRef) -> Result<(), Report> {
        if op.borrow().is::<scf::While>() {
            self.narrow_while(op)
        } else {
            self.narrow_results(op)
        }
    }

    /// Demotes forwarded values of an `scf.while` until both of its groups fit the window.
    ///
    /// The result group (the values `scf.condition` forwards, the after block arguments and the
    /// results) and the iter group (the inits, the before block arguments and the yielded values)
    /// are kept consistent separately by the verifier, so they are narrowed independently.
    fn narrow_while(&mut self, op: OperationRef) -> Result<(), Report> {
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

        // The result group: live-out-only columns first (their after block argument is unused),
        // then the columns used latest in the after region.
        let mut demoted_results = Vec::new();
        let mut forwarded_size = 1 + felts(forwarded.iter().copied());
        if forwarded_size > K {
            let columns = self.columns(&after_args, after_block, |_| false);
            for column in columns {
                if forwarded_size <= K {
                    break;
                }
                demoted_results.push(column.index);
                forwarded_size -= column.size;
            }
        }

        // The iter group: columns whose before block argument feeds a demoted result column
        // first (so the pair moves together), then the columns used latest in the before region.
        let mut demoted_iters = Vec::new();
        let mut inits_size = felts(inits.iter().copied());
        if inits_size > K {
            let paired = |arg: ValueRef| demoted_results.iter().any(|j| forwarded[*j] == arg);
            let columns = self.columns(&before_args, before_block, paired);
            for column in columns {
                if inits_size <= K {
                    break;
                }
                demoted_iters.push(column.index);
                inits_size -= column.size;
            }
        }

        // Result columns: store before the condition, load at the start of the after region and
        // after the loop.
        for j in demoted_results.iter().copied() {
            let local = self.alloc_local(forwarded[j]);
            let value = operand(cond_ref, 1 + j);
            self.rewriter.set_insertion_point_before(cond_ref);
            self.rewriter.store_local(local, value, span)?;
            self.replace_with_load(
                after_args[j],
                local,
                ProgramPoint::at_start_of(after_block),
                span,
            )?;
            self.replace_with_load(results[j], local, ProgramPoint::after(op), span)?;
        }

        // Iter columns: store before the loop and before the yield, load at the start of the
        // before region.
        for i in demoted_iters.iter().copied() {
            let local = self.alloc_local(inits[i]);
            self.rewriter.set_insertion_point_before(op);
            self.rewriter.store_local(local, inits[i], span)?;
            self.replace_with_load(
                before_args[i],
                local,
                ProgramPoint::at_start_of(before_block),
                span,
            )?;
            // The yielded value may itself have been replaced by a load above.
            let yielded = operand(yield_ref, i);
            self.rewriter.set_insertion_point_before(yield_ref);
            self.rewriter.store_local(local, yielded, span)?;
        }

        let iter_args = (0..inits.len())
            .map(|i| {
                if demoted_iters.contains(&i) {
                    IterArg::Remove(None)
                } else {
                    IterArg::Keep
                }
            })
            .collect::<SmallVec<[IterArg; 4]>>();
        let results = result_map(forwarded.len(), &demoted_results);
        rebuild_while(&mut self.rewriter, op, &iter_args, &results)?;

        Ok(())
    }

    /// Demotes results of an `scf.if` or `scf.index_switch` until they fit the window.
    fn narrow_results(&mut self, op: OperationRef) -> Result<(), Report> {
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

        // Every result is only used after the op, so the ones used latest go first.
        let mut demoted = Vec::new();
        let mut size = felts(results.iter().copied());
        let columns = {
            let next_uses = self.liveness.next_uses_at(&ProgramPoint::after(op));
            let mut columns = results
                .iter()
                .enumerate()
                .map(|(index, result)| Column {
                    index,
                    size: result.borrow().ty().size_in_felts(),
                    unused_inside: !result.borrow().has_real_uses(),
                    next_use: next_uses.as_ref().map(|nu| nu.distance(*result)).unwrap_or(u32::MAX),
                })
                .collect::<Vec<_>>();
            sort_columns(&mut columns);
            columns
        };
        for column in columns {
            if size <= K {
                break;
            }
            demoted.push(column.index);
            size -= column.size;
        }

        for j in demoted.iter().copied() {
            let local = self.alloc_local(results[j]);
            for entry in entry_blocks.iter() {
                let yield_ref = entry.borrow().terminator().unwrap();
                let value = operand(yield_ref, j);
                self.rewriter.set_insertion_point_before(yield_ref);
                self.rewriter.store_local(local, value, span)?;
            }
            self.replace_with_load(results[j], local, ProgramPoint::after(op), span)?;
        }

        let results = result_map(results.len(), &demoted);
        if !rebuild_if(&mut self.rewriter, op, &results)? {
            rebuild_index_switch(&mut self.rewriter, op, &results)?;
        }

        Ok(())
    }

    /// The columns of a region entry block, in the order they should be demoted: the ones the
    /// caller marks as preferred first, then the ones whose argument is unused in the region,
    /// then by decreasing distance to the next use, larger values first on ties.
    fn columns(
        &self,
        args: &[ValueRef],
        block: BlockRef,
        preferred: impl Fn(ValueRef) -> bool,
    ) -> Vec<Column> {
        let next_uses = self.liveness.next_uses_at(&ProgramPoint::at_start_of(block));
        let mut columns = args
            .iter()
            .enumerate()
            .map(|(index, arg)| Column {
                index,
                size: arg.borrow().ty().size_in_felts(),
                unused_inside: !arg.borrow().has_real_uses(),
                next_use: if preferred(*arg) {
                    u32::MAX
                } else {
                    next_uses.as_ref().map(|nu| nu.distance(*arg)).unwrap_or(u32::MAX)
                },
            })
            .collect::<Vec<_>>();
        sort_columns(&mut columns);
        columns
    }

    /// Allocates a procedure local of the type of `value`.
    fn alloc_local(&mut self, value: ValueRef) -> LocalVariable {
        let ty = value.borrow().ty().clone();
        self.function.borrow_mut().alloc_local(ty)
    }

    /// Replaces every use of `value` with a load of `local` inserted at `point`, if it has any.
    fn replace_with_load(
        &mut self,
        value: ValueRef,
        local: LocalVariable,
        point: ProgramPoint,
        span: SourceSpan,
    ) -> Result<(), Report> {
        if !value.borrow().has_real_uses() {
            return Ok(());
        }
        self.rewriter.set_insertion_point(point);
        let loaded = self.rewriter.load_local(local, span)?;
        self.rewriter.replace_all_uses_of_value_with(value, loaded);
        Ok(())
    }
}

/// The arguments of `block` as values.
fn block_args(block: BlockRef) -> SmallVec<[ValueRef; 4]> {
    block.borrow().arguments().iter().map(|arg| *arg as ValueRef).collect()
}

/// The current value of operand `index` of `op`.
fn operand(op: OperationRef, index: usize) -> ValueRef {
    op.borrow().operands()[index].borrow().as_value_ref()
}

/// The result map for the rebuild helpers: kept results are renumbered in order, demoted ones
/// are dropped.
fn result_map(num_results: usize, demoted: &[usize]) -> SmallVec<[Option<usize>; 4]> {
    let mut next = 0;
    (0..num_results)
        .map(|index| {
            if demoted.contains(&index) {
                None
            } else {
                next += 1;
                Some(next - 1)
            }
        })
        .collect()
}

/// Orders columns for demotion: unused ones first, then the furthest next use, then the largest.
fn sort_columns(columns: &mut [Column]) {
    columns.sort_by(|a, b| {
        b.unused_inside
            .cmp(&a.unused_inside)
            .then(b.next_use.cmp(&a.next_use))
            .then(b.size.cmp(&a.size))
            .then(a.index.cmp(&b.index))
    });
}
