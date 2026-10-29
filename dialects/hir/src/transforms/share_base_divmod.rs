//! The `share-base-divmod` pass: divisions of one base plus constant offsets share one division.

use alloc::{rc::Rc, vec::Vec};

use midenc_dialect_arith::{Add, ArithOpBuilder, Divmod};
use midenc_hir::{
    BlockRef, Builder, ConstantIntRange, EntityMut, Forward, FxHashMap, Immediate, Op,
    OperationName, OperationRef, RawWalk, Report, Rewriter, Spanned, Symbol, TraceTarget, Type,
    ValueRef,
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

use crate::{Bitcast, LoadLocal, LocalAddress, StoreLocal};

/// Shares one `u32` `arith.divmod` among the divisions of one base plus constant offsets in a
/// block.
///
/// A group is formed, within a block, by the divisions by the same constant divisor `d` of the same
/// base `b`, either directly (`arith.divmod(b, d)`) or plus a constant offset `K` that is a multiple
/// of `d` (`arith.divmod(arith.add(b, K) <checked>, d)`, the addition being in the same block and
/// having no other use). A group of at least two divisions is rewritten to divide `b + Kmax` once,
/// where `Kmax` is the largest offset of the group, at the first operation of the group. Every
/// division then takes the shared remainder, and the shared quotient minus `(Kmax - K) / d`.
///
/// The addition of `Kmax` traps when `b + Kmax` overflows, like the addition it replaces, unless
/// the integer range analysis proves that it cannot overflow. That trap may now happen earlier in
/// the block than the access to `b + Kmax` would have: any execution that reaches the first
/// operation of the group and then the access still fails, but with the overflow instead of a
/// failure between them, and without the host events emitted between them.
///
/// Bases are the same when they are the same SSA value, or when they are loaded from the same
/// procedure local in the block, all directly or all through a `hir.bitcast`, with no store to that
/// local between the loads, including stores nested in regions. A local is assumed to be written
/// only by `hir.store_local`: a function that takes the address of any local with
/// `hir.local_address` gets no such groups, since a memory write may then modify a local.
///
/// Memory accesses with element-aligned offsets benefit: all accesses through one base share one
/// conversion to the element address, and `remove-repeated-assertz` then drops the repeated
/// alignment checks of the shared remainder.
#[derive(Default)]
pub struct ShareBaseDivmod;

midenc_hir::inventory::submit!(::midenc_hir::pass::registry::PassInfo::new::<ShareBaseDivmod>(
    "share-base-divmod",
    "share one divmod of a base value among divmods of that base plus constant offsets"
));

/// A `divmod` of `base + offset` by the divisor of its group.
struct Candidate {
    /// The division.
    divmod: OperationRef,
    /// The checked addition of `offset` producing the dividend, when `offset` is not zero.
    add: Option<OperationRef>,
    /// The operation and operand index holding the base: the addition, or the division itself.
    // The base is read when needed rather than stored: rewriting a group replaces the quotient it
    // produces, which may be the base of another group.
    base_operand: (OperationRef, usize),
    /// The constant added to the base, a multiple of the divisor.
    offset: u32,
}

impl Candidate {
    /// The base of this candidate.
    fn base(&self) -> ValueRef {
        let (op, index) = self.base_operand;
        op.borrow().operands()[index].borrow().as_value_ref()
    }
}

/// Identifies the runtime value of a base within a block.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum BaseKey {
    /// The base is this SSA value.
    Value(ValueRef),
    /// The base is loaded from a local, and loads with equal keys read the same value.
    Local {
        /// The local the base is loaded from.
        local: LocalVariable,
        /// The number of stores to `local` in the block before the load, including stores nested
        /// in regions.
        epoch: u32,
        /// Whether the loaded value is reinterpreted by a `hir.bitcast`.
        bitcast: bool,
    },
}

/// The candidates of one block that divide the same base by the same divisor, in block order.
struct Group {
    /// The identity of the base.
    key: BaseKey,
    /// The constant divisor, not zero.
    divisor: u32,
    /// The divisions, in block order.
    candidates: Vec<Candidate>,
}

impl Group {
    /// Returns true if the group shares a division among several divisions.
    fn is_profitable(&self) -> bool {
        self.candidates.len() > 1
    }

    /// Returns true if the range analysis may prove that the base plus the largest offset fits in 32
    /// bits: there is an offset, and the base is not loaded from a local, whose range is unknown.
    fn may_be_proven(&self) -> bool {
        matches!(self.key, BaseKey::Value(_)) && self.max_offset() != 0
    }

    /// The largest offset of the group.
    fn max_offset(&self) -> u32 {
        self.candidates.iter().map(|candidate| candidate.offset).max().unwrap_or(0)
    }

    /// The first operation of the group in block order, and the candidate it belongs to.
    fn first_op(&self) -> (OperationRef, &Candidate) {
        // A candidate's addition precedes its division in the same block (see `match_candidate`),
        // and may precede the divisions of the earlier candidates too.
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

    /// Returns true if the range analysis proves that the base plus the largest offset fits in 32
    /// bits.
    fn is_proven(&self, solver: &DataFlowSolver) -> bool {
        let offset = ConstantIntRange::constant(Immediate::U32(self.max_offset()));
        // The bases of the group hold the same value, so a proof for any of them holds for all.
        self.candidates.iter().any(|candidate| {
            value_range(solver, candidate.base()).is_some_and(|range| {
                range.is_proven()
                    && range
                        .range()
                        .and_then(|range| range.add_unsigned_without_overflow(&offset))
                        .is_some()
            })
        })
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

        let groups = if function.is_declaration() {
            Vec::new()
        } else {
            collect_groups(function.as_operation_ref())
        };
        let groups = groups.into_iter().filter(Group::is_profitable).collect::<Vec<_>>();
        if groups.is_empty() {
            state.preserved_analyses_mut().preserve_all();
            state.set_post_pass_status(PostPassStatus::Unchanged);
            return Ok(());
        }

        // Decide every proof before the first rewrite: the solver only knows the original values.
        let proven = if groups.iter().any(Group::may_be_proven) {
            let mut solver = DataFlowSolver::default();
            solver.load::<IntegerRangeAnalysis>();
            solver.initialize_and_run(function.as_operation(), state.analysis_manager().clone())?;
            groups
                .iter()
                .map(|group| group.may_be_proven() && group.is_proven(&solver))
                .collect::<Vec<_>>()
        } else {
            alloc::vec![false; groups.len()]
        };

        let context = function.as_operation().context_rc();
        drop(function);

        let mut rewriter = RewriterImpl::<TracingRewriterListener>::new(context)
            .with_listener(TracingRewriterListener);
        for (group, proven) in groups.iter().zip(proven) {
            log::debug!(
                target: &trace_target,
                sym = trace_target.relevant_symbol();
                "sharing the division of {} by {} among {} divisions (proven: {proven})",
                group.candidates[0].base(),
                group.divisor,
                group.candidates.len()
            );
            rewrite_group(&mut rewriter, group, proven)?;
        }

        state.set_post_pass_status(PostPassStatus::Changed);
        Ok(())
    }
}

/// Collects the candidate divisions of `function`, grouped by block, base and divisor.
fn collect_groups(function: OperationRef) -> Vec<Group> {
    // An address of one local reaches the others too (they are adjacent in the frame), so a memory
    // write may then modify any local.
    let mut takes_local_address = false;
    let mut blocks = Vec::<BlockRef>::new();
    function.raw_prewalk_all::<Forward, _>(|op: OperationRef| {
        let op = op.borrow();
        takes_local_address |= op.is::<LocalAddress>();
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
                if !takes_local_address {
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

            let Some((divisor, candidate)) = match_candidate(op) else {
                continue;
            };
            let key = base_key(candidate.base(), &load_epochs);
            let index = *group_index.entry((key, divisor)).or_insert_with(|| {
                groups.push(Group {
                    key,
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

/// Returns the key of `base`: the local and epoch of the load in the block that produces it,
/// directly or through a `hir.bitcast`, if any, otherwise the value itself.
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
/// that is a multiple of the divisor, defined in the same block and used only by the division.
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
        // An addition with other uses stays, so splitting it off would not remove it.
        if !add.has_exactly_one_use() {
            return None;
        }
        let add_ref = add.downcast_ref::<Add>()?;
        // The shared addition is placed before the group's first operation in this block, which
        // must stay on the paths through the original addition.
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

/// Rewrites the divisions of `group` to share one division of the base plus the largest offset,
/// whose addition is checked unless `proven`.
fn rewrite_group(
    rewriter: &mut RewriterImpl<TracingRewriterListener>,
    group: &Group,
    proven: bool,
) -> Result<(), Report> {
    // Every base of the group holds the same value; the one of the first operation is defined
    // before it.
    let (first_op, first_candidate) = group.first_op();
    let base = first_candidate.base();
    let max_offset = group.max_offset();
    // The shared operations report the location of the access whose overflow they check.
    let source = group
        .candidates
        .iter()
        .find(|candidate| candidate.offset == max_offset)
        .map(|candidate| candidate.add.unwrap_or(candidate.divmod))
        .expect("the largest offset belongs to a candidate");
    let span = source.borrow().span();
    let (remainder, quotient) = {
        let mut rewriter = rewriter.with_inline_call_chain(source);
        rewriter.set_insertion_point_before(first_op);
        let dividend = if max_offset == 0 {
            base
        } else {
            let offset = rewriter.imm(Immediate::U32(max_offset), span);
            if proven {
                rewriter.add_unchecked(base, offset, span)?
            } else {
                rewriter.add(base, offset, span)?
            }
        };
        let divisor = rewriter.imm(Immediate::U32(group.divisor), span);
        let (quotient, remainder) = rewriter.divmod(dividend, divisor, span)?;
        (remainder, quotient)
    };

    for candidate in &group.candidates {
        let (old_remainder, old_quotient, span) = {
            let op = candidate.divmod.borrow();
            let divmod = op.downcast_ref::<Divmod>().expect("candidates are divmods");
            (divmod.remainder().as_value_ref(), divmod.quotient().as_value_ref(), op.span())
        };
        let new_quotient = if candidate.offset == max_offset {
            quotient
        } else {
            let mut rewriter = rewriter.with_inline_call_chain(candidate.divmod);
            rewriter.set_insertion_point_before(candidate.divmod);
            // Both offsets are multiples of the divisor, so this is exactly
            // `(base + offset) / divisor`, and it cannot underflow.
            let difference = (max_offset - candidate.offset) / group.divisor;
            let difference = rewriter.imm(Immediate::U32(difference), span);
            rewriter.sub_unchecked(quotient, difference, span)?
        };
        rewriter.replace_all_uses_of_value_with(old_remainder, remainder);
        rewriter.replace_all_uses_of_value_with(old_quotient, new_quotient);
        rewriter.erase_op(candidate.divmod);
        if let Some(add) = candidate.add {
            rewriter.erase_op(add);
        }
    }
    Ok(())
}
