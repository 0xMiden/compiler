use alloc::{rc::Rc, vec::Vec};

use midenc_dialect_arith::{Add, ArithOpBuilder, Divmod};
use midenc_hir::{
    BlockRef, Builder, EntityMut, Forward, FxHashMap, FxHashSet, Immediate, Op, OperationName,
    OperationRef, RawWalk, Report, Rewriter, Spanned, Symbol, TraceTarget, Type, ValueRef,
    dialects::builtin::{
        Function,
        attributes::{LocalVariable, Overflow},
    },
    matchers::{self, Matcher},
    pass::{Pass, PassExecutionState, PostPassStatus},
    patterns::{RewriterExt, RewriterImpl, TracingRewriterListener},
};
use midenc_hir_analysis::{
    DataFlowSolver,
    analyses::{IntegerRangeAnalysis, integer_range::value_range},
};

use crate::{Bitcast, HirOpBuilder, LoadLocal, LocalAddress, StoreLocal};

/// Shares one `u32` `arith.divmod` of a base value among the divisions of that base plus constant
/// offsets in a block.
///
/// Within a block, every `arith.divmod(arith.add(b, K) <checked>, d)` whose constant offset `K` is
/// a multiple of the constant divisor `d`, and every `arith.divmod(b, d)`, is rewritten to use a
/// single `arith.divmod(b, d)` placed at the first of them: the remainder is the shared one, and
/// the quotient is the shared quotient plus `K / d`, added without a check.
///
/// Bases are the same when they are the same SSA value, or when they are loaded (optionally through
/// a `hir.bitcast`) from the same procedure local in the block with no store to that local between
/// the loads. Locals whose address is taken are excluded, since memory writes may modify them.
///
/// The checked addition traps when `b + K` overflows, and that trap is preserved. An offset is
/// applied without a check only when the integer range analysis proves that `b + K` cannot
/// overflow, or when an overflow check of `b` plus the largest unproven offset of the block,
/// emitted at the first division of the group, has passed. That check may trap earlier in the block
/// than the access it guards would have, which is unobservable: a trap aborts the whole execution.
///
/// Memory accesses with an element-aligned offset benefit: all accesses through one base share one
/// conversion to the element address, and `remove-repeated-assertz` then drops the repeated
/// alignment checks of the shared remainder.
#[derive(Default)]
pub struct ShareBaseDivmod;

midenc_hir::inventory::submit!(::midenc_hir::pass::registry::PassInfo::new::<ShareBaseDivmod>(
    "share-base-divmod",
    "share one divmod of a base value among divmods of that base plus constant offsets"
));

/// A `divmod` of `base + offset` by `divisor`.
struct Candidate {
    divmod: OperationRef,
    /// The checked addition producing the dividend, when `offset` is not zero.
    add: Option<OperationRef>,
    /// The operation and operand index that hold the base: the addition, or the division itself.
    ///
    /// The base is read from the IR when the group is rewritten, because rewriting an earlier group
    /// may have replaced it (e.g. when the base is the quotient of a shared division).
    base_operand: (OperationRef, usize),
    offset: u32,
    /// Whether the range analysis proves that `base + offset` does not overflow.
    proven: bool,
}

impl Candidate {
    /// The base of this candidate, read from the IR.
    fn base(&self) -> ValueRef {
        let (op, index) = self.base_operand;
        op.borrow().operands()[index].borrow().as_value_ref()
    }
}

/// Identifies the runtime value of a base within a block.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum BaseKey {
    Value(ValueRef),
    /// A value loaded from `local` in the block, possibly through a `hir.bitcast`. `epoch` counts
    /// the stores to `local` in the block before the load, so loads with equal keys read the same
    /// value.
    Local {
        local: LocalVariable,
        epoch: u32,
        bitcast: bool,
    },
}

/// The candidates of one block that divide the same base by the same divisor, in block order.
struct Group {
    divisor: u32,
    candidates: Vec<Candidate>,
}

impl Group {
    /// A base of the group, for diagnostics.
    fn base(&self) -> ValueRef {
        self.candidates[0].base()
    }

    /// Returns true if sharing the division saves work: it replaces at least one checked offset
    /// and either merges several divisions or needs no overflow check.
    fn is_profitable(&self) -> bool {
        let has_offset = self.candidates.iter().any(|candidate| candidate.offset != 0);
        has_offset && (self.candidates.len() > 1 || self.unproven_max_offset() == 0)
    }

    /// The largest offset whose addition is not proven to be free of overflow, or zero.
    fn unproven_max_offset(&self) -> u32 {
        self.candidates
            .iter()
            .filter(|candidate| !candidate.proven)
            .map(|candidate| candidate.offset)
            .max()
            .unwrap_or(0)
    }

    /// The first operation of the group in block order, and the candidate it belongs to.
    fn first_op(&self) -> (OperationRef, &Candidate) {
        self.candidates
            .iter()
            .flat_map(|candidate| {
                candidate
                    .add
                    .into_iter()
                    .chain([candidate.divmod])
                    .map(move |op| (op, candidate))
            })
            .reduce(|first, next| {
                if next.0.borrow().is_before_in_block(&first.0) {
                    next
                } else {
                    first
                }
            })
            .expect("groups are never empty")
    }
}

impl Pass for ShareBaseDivmod {
    type Target = Function;

    fn name(&self) -> &'static str {
        "share-base-divmod"
    }

    fn argument(&self) -> &'static str {
        "share-base-divmod"
    }

    fn can_schedule_on(&self, _name: &OperationName) -> bool {
        true
    }

    fn initialize(&mut self, context: Rc<midenc_hir::Context>) -> Result<(), Report> {
        context.get_or_register_dialect::<crate::HirDialect>();
        context.get_or_register_dialect::<midenc_dialect_arith::ArithDialect>();

        Ok(())
    }

    fn run_on_operation(
        &mut self,
        op: EntityMut<'_, Self::Target>,
        state: &mut PassExecutionState,
    ) -> Result<(), Report> {
        let function = op.into_entity_ref();
        let trace_target = TraceTarget::category("pass")
            .with_topic(self.name())
            .with_relevant_symbol(function.name().as_str());

        if function.is_declaration() {
            state.preserved_analyses_mut().preserve_all();
            state.set_post_pass_status(PostPassStatus::Unchanged);
            return Ok(());
        }

        let mut solver = DataFlowSolver::default();
        solver.load::<IntegerRangeAnalysis>();
        // Without a completed solve, ranges prove nothing and every offset gets a runtime check.
        let solver = match solver
            .initialize_and_run(function.as_operation(), state.analysis_manager().clone())
        {
            Ok(()) => Some(solver),
            Err(err) => {
                log::debug!(
                    target: &trace_target,
                    sym = trace_target.relevant_symbol();
                    "integer range analysis did not complete, nothing is proven: {err}"
                );
                None
            }
        };

        let op = function.as_operation_ref();
        let context = function.as_operation().context_rc();
        drop(function);

        let groups = collect_groups(op, solver.as_ref());
        let mut rewriter = RewriterImpl::<TracingRewriterListener>::new(context)
            .with_listener(TracingRewriterListener);
        let mut changed = false;
        for group in groups.iter().filter(|group| group.is_profitable()) {
            log::debug!(
                target: &trace_target,
                sym = trace_target.relevant_symbol();
                "sharing the division of {} by {} among {} divisions",
                group.base(),
                group.divisor,
                group.candidates.len()
            );
            rewrite_group(&mut rewriter, group)?;
            changed = true;
        }

        if changed {
            state.set_post_pass_status(PostPassStatus::Changed);
        } else {
            state.preserved_analyses_mut().preserve_all();
            state.set_post_pass_status(PostPassStatus::Unchanged);
        }
        Ok(())
    }
}

/// Collects the candidate divisions of `function`, grouped by block, base and divisor.
fn collect_groups(function: OperationRef, solver: Option<&DataFlowSolver>) -> Vec<Group> {
    let mut address_taken = FxHashSet::<LocalVariable>::default();
    let mut blocks = Vec::<BlockRef>::new();
    function.raw_prewalk_all::<Forward, _>(|op: OperationRef| {
        let op = op.borrow();
        if let Some(local_address) = op.downcast_ref::<LocalAddress>() {
            address_taken.insert(*local_address.get_local());
        }
        for region in op.regions() {
            blocks.extend(region.body().iter().map(|block| block.as_block_ref()));
        }
    });

    let mut groups = Vec::<Group>::new();
    for block in blocks {
        let mut group_index = FxHashMap::<(BaseKey, u32), usize>::default();
        let mut epochs = FxHashMap::<LocalVariable, u32>::default();
        let mut load_epochs = FxHashMap::<OperationRef, (LocalVariable, u32)>::default();
        let mut next_op = block.borrow().body().front().as_pointer();
        while let Some(op) = next_op.take() {
            next_op = op.next();
            let operation = op.borrow();
            if let Some(load) = operation.downcast_ref::<LoadLocal>() {
                let local = *load.get_local();
                if !address_taken.contains(&local) {
                    load_epochs.insert(op, (local, epochs.get(&local).copied().unwrap_or(0)));
                }
            } else if operation.has_regions() || operation.is::<StoreLocal>() {
                // A store, also one nested in a region, starts a new epoch of its local.
                op.raw_prewalk_all::<Forward, _>(|nested: OperationRef| {
                    if let Some(store) = nested.borrow().downcast_ref::<StoreLocal>() {
                        *epochs.entry(*store.get_local()).or_default() += 1;
                    }
                });
            }
            drop(operation);

            let Some((divisor, mut candidate)) = match_candidate(op) else {
                continue;
            };
            let base = candidate.base();
            candidate.proven = candidate.offset == 0
                || solver.and_then(|solver| value_range(solver, base)).is_some_and(|range| {
                    range.is_proven()
                        && range.range().is_some_and(|range| {
                            range.umax() + u128::from(candidate.offset) <= u128::from(u32::MAX)
                        })
                });
            let key = base_key(base, &load_epochs);
            let index = *group_index.entry((key, divisor)).or_insert_with(|| {
                groups.push(Group {
                    divisor,
                    candidates: Vec::new(),
                });
                groups.len() - 1
            });
            groups[index].candidates.push(candidate);
        }
    }
    groups
}

/// Returns the key of `base`: the local and epoch of the load in the block that produces it, if
/// any, otherwise the value itself.
fn base_key(
    base: ValueRef,
    load_epochs: &FxHashMap<OperationRef, (LocalVariable, u32)>,
) -> BaseKey {
    let loaded = |value: ValueRef| {
        let op = value.borrow().get_defining_op()?;
        load_epochs.get(&op).copied()
    };
    let bitcast_operand = base.borrow().get_defining_op().and_then(|op| {
        op.borrow()
            .downcast_ref::<Bitcast>()
            .map(|bitcast| bitcast.operand().as_value_ref())
    });
    if let Some((local, epoch)) = loaded(base) {
        BaseKey::Local {
            local,
            epoch,
            bitcast: false,
        }
    } else if let Some((local, epoch)) = bitcast_operand.and_then(loaded) {
        BaseKey::Local {
            local,
            epoch,
            bitcast: true,
        }
    } else {
        BaseKey::Value(base)
    }
}

/// Matches a `u32` `arith.divmod` by a non-zero constant, returning its divisor and the candidate.
///
/// The dividend is split into a base and an offset when it is a checked addition of a constant
/// that is a multiple of the divisor, defined in the same block.
fn match_candidate(op: OperationRef) -> Option<(u32, Candidate)> {
    let operation = op.borrow();
    let divmod = operation.downcast_ref::<Divmod>()?;
    let block = operation.parent()?;
    let dividend = divmod.lhs().as_value_ref();
    if dividend.borrow().ty() != &Type::U32 {
        return None;
    }
    let divisor = constant_u32(divmod.rhs().as_value_ref()).filter(|divisor| *divisor != 0)?;

    let offset_add = dividend.borrow().get_defining_op().and_then(|add_op| {
        let add = add_op.borrow();
        let add_ref = add.downcast_ref::<Add>()?;
        if *add_ref.get_overflow() != Overflow::Checked || add.parent() != Some(block) {
            return None;
        }
        let (lhs, rhs) = (add_ref.lhs().as_value_ref(), add_ref.rhs().as_value_ref());
        let (base_index, offset) = match (constant_u32(lhs), constant_u32(rhs)) {
            (_, Some(offset)) => (0, offset),
            (Some(offset), None) => (1, offset),
            (None, None) => return None,
        };
        (offset != 0 && offset.is_multiple_of(divisor)).then_some((add_op, base_index, offset))
    });
    let (add, base_operand, offset) = match offset_add {
        Some((add, base_index, offset)) => (Some(add), (add, base_index), offset),
        None => (None, (op, 0), 0),
    };
    Some((
        divisor,
        Candidate {
            divmod: op,
            add,
            base_operand,
            offset,
            proven: false,
        },
    ))
}

/// Returns the value of `value` if it is defined by a `u32` constant.
fn constant_u32(value: ValueRef) -> Option<u32> {
    let defining_op = value.borrow().get_defining_op()?;
    match matchers::constant_of::<Immediate>().matches(&*defining_op.borrow())? {
        Immediate::U32(value) => Some(value),
        _ => None,
    }
}

/// Rewrites the divisions of `group` to share one division of the base.
fn rewrite_group(
    rewriter: &mut RewriterImpl<TracingRewriterListener>,
    group: &Group,
) -> Result<(), Report> {
    // Every base of the group holds the same value; the one of the first operation is defined
    // before it.
    let (first_op, first_candidate) = group.first_op();
    let base = first_candidate.base();
    let span = first_op.borrow().span();
    let (remainder, quotient) = {
        let mut rewriter = rewriter.with_inline_call_chain(first_op);
        rewriter.set_insertion_point_before(first_op);
        let check_offset = group.unproven_max_offset();
        if check_offset != 0 {
            // `base` plus every unproven offset fits in 32 bits once `base + check_offset` does.
            let offset = rewriter.imm(Immediate::U32(check_offset), span);
            let (overflowed, _) = rewriter.add_overflowing(base, offset, span)?;
            rewriter.assertz(overflowed, span)?;
        }
        let divisor = rewriter.imm(Immediate::U32(group.divisor), span);
        let (quotient, remainder) = rewriter.divmod(base, divisor, span)?;
        (remainder, quotient)
    };

    for candidate in &group.candidates {
        let (old_remainder, old_quotient, span) = {
            let op = candidate.divmod.borrow();
            let divmod = op.downcast_ref::<Divmod>().expect("candidates are divmods");
            (divmod.remainder().as_value_ref(), divmod.quotient().as_value_ref(), op.span())
        };
        let new_quotient = if candidate.offset == 0 {
            quotient
        } else {
            let mut rewriter = rewriter.with_inline_call_chain(candidate.divmod);
            rewriter.set_insertion_point_before(candidate.divmod);
            // No overflow: the quotient is at most `(base + offset) / divisor`, which is proven or
            // checked to fit in 32 bits.
            let offset = rewriter.imm(Immediate::U32(candidate.offset / group.divisor), span);
            rewriter.add_unchecked(quotient, offset, span)?
        };
        rewriter.replace_all_uses_of_value_with(old_remainder, remainder);
        rewriter.replace_all_uses_of_value_with(old_quotient, new_quotient);
        rewriter.erase_op(candidate.divmod);
        if let Some(add) = candidate.add
            && !add.borrow().is_used()
        {
            rewriter.erase_op(add);
        }
    }
    Ok(())
}
