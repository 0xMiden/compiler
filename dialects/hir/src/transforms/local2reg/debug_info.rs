//! Program-point recovery for nonescaping locals which executable promotion has already selected.
use alloc::{rc::Rc, vec::Vec};

use midenc_hir::{
    Block, BlockRef, CallOpInterface, Forward, FxHashMap, FxHashSet, Operation, OperationRef,
    ProgramPoint, RawWalk, RegionBranchOpInterface, RegionBranchPoint,
    RegionBranchTerminatorOpInterface, RegionRef, Report, Rewriter, RewriterExt, Spanned,
    StorableEntity, SuccessorOperands, ValueRef,
    dialects::{
        builtin::{Function, attributes::LocalVariable},
        debuginfo::{
            DIBuilder, DebugInfoMutation, DebugInfoUpdate,
            attributes::{Expression, ExpressionOp, FrameBase},
            bindings::{RecoveryIdentity, VisibilitySlot},
            debug_record_variable,
            transform::invalidate_debug_record_with,
        },
    },
    dominance::{DominanceInfo, Dominates},
    pass::AnalysisManager,
    traits::BranchOpInterface,
};
use midenc_hir_analysis::{
    AnalysisState, AnalysisStateGuardMut, BuildableDataFlowAnalysis, CallControlFlowAction,
    DataFlowSolver, DenseForwardDataFlowAnalysis, DenseLattice, Lattice, LatticeLike,
    analyses::{DeadCodeAnalysis, SparseConstantPropagation, dce::PredecessorState},
    dense::DenseDataFlowAnalysis,
};

use crate::StoreLocal;

// Dense facts exist at every program point. Bound their local width instead of allocating an
// unbounded function-wide product; each batch still shares DCE/SCCP and dominance across locals.
const RECOVERY_BATCH_SIZE: usize = 64;

/// Bottom is an unreachable program point. At reachable points, aliases are SSA values known
/// equal to the current slot; an empty set means unknown. Binding bits record possible inactive
/// (1) and active (2) states, so a join cannot invent an unconditional source binding.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Recovery(Option<Facts>);

#[derive(Clone, Debug, PartialEq, Eq)]
struct Facts {
    aliases: Vec<Vec<ValueRef>>,
    bindings: Vec<u8>,
}

impl LatticeLike for Recovery {
    fn join(&self, other: &Self) -> Self {
        match (&self.0, &other.0) {
            (None, _) => other.clone(),
            (_, None) => self.clone(),
            (Some(a), Some(b)) => Self(Some(Facts {
                aliases: a
                    .aliases
                    .iter()
                    .zip(&b.aliases)
                    .map(|(a, b)| a.iter().copied().filter(|v| b.contains(v)).collect())
                    .collect(),
                bindings: a.bindings.iter().zip(&b.bindings).map(|(a, b)| a | b).collect(),
            })),
        }
    }

    fn meet(&self, _: &Self) -> Self {
        unreachable!("recovery is a forward join analysis")
    }
}

struct LocalRecovery {
    locals: FxHashMap<LocalVariable, usize>,
    entry: BlockRef,
    references: Vec<OperationRef>,
    record_updates: FxHashMap<OperationRef, Vec<(usize, u8)>>,
    dominance: Rc<DominanceInfo>,
}

impl BuildableDataFlowAnalysis for LocalRecovery {
    type Strategy = DenseDataFlowAnalysis<Self, Forward>;

    fn new(_: &mut DataFlowSolver) -> Self {
        unreachable!("local recovery requires a selected local")
    }
}

fn same_binding(a: &Operation, b: &Operation) -> bool {
    RecoveryIdentity::of(a) == RecoveryIdentity::of(b)
}

impl DenseForwardDataFlowAnalysis for LocalRecovery {
    type Lattice = Lattice<Recovery>;

    fn set_to_entry_state(
        &self,
        lattice: &mut AnalysisStateGuardMut<'_, Self::Lattice>,
        _: &mut DataFlowSolver,
    ) {
        let point = lattice.anchor().as_any().downcast_ref::<ProgramPoint>().unwrap();
        let at_entry = point.block() == Some(self.entry);
        lattice.join(&Recovery(Some(Facts {
            aliases: alloc::vec![Vec::new(); self.locals.len()],
            bindings: alloc::vec![if at_entry { 1 } else { 3 }; self.references.len()],
        })));
    }

    fn visit_operation(
        &self,
        op: &Operation,
        before: &Self::Lattice,
        after: &mut AnalysisStateGuardMut<'_, Self::Lattice>,
        _: &mut DataFlowSolver,
    ) -> Result<(), Report> {
        let mut state = before.value().clone();
        if let Some(facts) = state.0.as_mut() {
            if let Some(store) = op.downcast_ref::<StoreLocal>()
                && let Some(index) = self.locals.get(&*store.get_local())
            {
                facts.aliases[*index] = alloc::vec![store.value().as_value_ref()];
            }
            if op.has_regions() {
                // RegionBranchOpInterface owners are dispatched separately. An opaque owner
                // may execute its regions in an unknown order, so it cannot preserve a reaching
                // value or promise that a previously active binding remains active.
                facts.aliases.iter_mut().for_each(Vec::clear);
                for (reference, active) in self.references.iter().zip(&mut facts.bindings) {
                    if *active != 1
                        || self.references.iter().any(|nested| {
                            op.is_ancestor_of(&nested.borrow())
                                && same_binding(&reference.borrow(), &nested.borrow())
                        })
                    {
                        *active = 3;
                    }
                }
            }
            if let Some(updates) = self.record_updates.get(&op.as_operation_ref()) {
                for (index, active) in updates {
                    facts.bindings[*index] = *active;
                }
            }
        }
        after.join(&state);
        Ok(())
    }

    fn visit_call_control_flow_transfer(
        &self,
        _: &dyn CallOpInterface,
        _: CallControlFlowAction,
        before: &Self::Lattice,
        after: &mut AnalysisStateGuardMut<'_, Self::Lattice>,
        _: &mut DataFlowSolver,
    ) {
        // The caller's local has no LocalAddress uses. A call cannot access its slot, and its
        // source bindings belong to a separate frame. This also applies to external calls.
        after.join(before.value());
    }

    fn visit_branch_control_flow_transfer(
        &self,
        from: BlockRef,
        to: &Block,
        before: &Self::Lattice,
        after: &mut AnalysisStateGuardMut<'_, Self::Lattice>,
        _: &mut DataFlowSolver,
    ) {
        for edge in to.predecessors().filter(|edge| edge.predecessor() == from) {
            let terminator = edge.owner.borrow();
            let Some(branch) = terminator.as_trait::<dyn BranchOpInterface>() else {
                after.join(&translate(
                    before.value(),
                    core::iter::empty(),
                    ProgramPoint::at_start_of(to.as_block_ref()),
                    &self.dominance,
                ));
                continue;
            };
            let operands = branch.get_successor_operands(edge.index());
            let mappings = to.arguments().iter().enumerate().filter_map(|(i, arg)| {
                Some((operands.get(i)?.into_value_ref()?, *arg as ValueRef))
            });
            after.join(&translate(
                before.value(),
                mappings,
                ProgramPoint::at_start_of(to.as_block_ref()),
                &self.dominance,
            ));
        }
    }

    fn visit_region_branch_control_flow_transfer(
        &self,
        branch: &dyn RegionBranchOpInterface,
        _: Option<RegionRef>,
        region_to: Option<RegionRef>,
        before: &Self::Lattice,
        after: &mut AnalysisStateGuardMut<'_, Self::Lattice>,
        solver: &mut DataFlowSolver,
    ) {
        let from = *before.anchor().as_any().downcast_ref::<ProgramPoint>().unwrap();
        let to = *after.anchor().as_any().downcast_ref::<ProgramPoint>().unwrap();
        let predecessor = if from == ProgramPoint::before(branch.as_operation()) {
            branch.as_operation().as_operation_ref()
        } else {
            from.prev_operation().unwrap()
        };
        let successor = region_to.map_or(RegionBranchPoint::Parent, RegionBranchPoint::Child);
        let op = predecessor.borrow();
        let operands = if predecessor == branch.as_operation().as_operation_ref() {
            branch.get_entry_successor_operands(successor)
        } else if let Some(terminator) = op.as_trait::<dyn RegionBranchTerminatorOpInterface>() {
            terminator.get_successor_operands(successor)
        } else {
            // The parent can expose an edge without the terminator exposing its operands.
            // Keep invariant names, but do not carry old region arguments across reentry.
            after.join(&translate(before.value(), core::iter::empty(), to, &self.dominance));
            return;
        };
        // The dense dispatcher already subscribes to, and holds, this predecessor state.
        let predecessors = solver.get::<PredecessorState, _>(&to).unwrap();
        let inputs = predecessors.successor_inputs(&predecessor);
        let mappings = inputs
            .iter()
            .enumerate()
            .filter_map(|(i, input)| Some((operands.get(i)?.into_value_ref()?, *input)));
        after.join(&translate(before.value(), mappings, to, &self.dominance));
    }
}

/// Keep all equivalent names, rather than eagerly replacing an outer value by a block argument.
/// This lets both identical incoming values and differing values with an existing semantic phi
/// survive joins. No executable arguments or operations are created for debug information.
fn translate(
    state: &Recovery,
    mappings: impl Iterator<Item = (ValueRef, ValueRef)>,
    target: ProgramPoint,
    dominance: &DominanceInfo,
) -> Recovery {
    let mut state = state.clone();
    if let Some(facts) = &mut state.0 {
        let mappings = mappings.collect::<Vec<_>>();
        let entry = target.is_at_block_start().then(|| target.block().unwrap());
        let anchor = if entry.is_some() {
            target.next_operation()
        } else {
            target.prev_operation()
        };
        // A loop reentry assigns a new dynamic value to its arguments and reexecutes its body.
        // Only explicit edge operands can reestablish these names. In particular, an old loop
        // argument is not the current local just because it has the same static ValueRef.
        for values in &mut facts.aliases {
            let aliases = values.clone();
            values.retain(|value| {
                let value = value.borrow();
                entry.is_none_or(|block| value.parent_block() != Some(block))
                    && anchor.is_some_and(|op| value.properly_dominates(&op.borrow(), dominance))
            });
            for (source, target) in &mappings {
                if aliases.contains(source) && !values.contains(target) {
                    values.push(*target);
                }
            }
        }
    }
    state
}

fn normalized_expression(expression: &Expression, local: u32) -> Option<Expression> {
    match expression.operations.as_slice() {
        [ExpressionOp::LocalSlot(index)]
        | [ExpressionOp::LocalSlot(index), ExpressionOp::StackValue]
            if *index == local =>
        {
            Some(Expression::default())
        }
        [
            ExpressionOp::FrameBase {
                base: FrameBase::LocalSlot(index),
                byte_offset,
            },
            tail @ ..,
        ] if *index == local
            && tail.iter().all(|op| {
                matches!(
                    op,
                    ExpressionOp::PlusUConst(_) | ExpressionOp::Deref | ExpressionOp::StackValue
                )
            }) =>
        {
            let mut operations = Vec::new();
            if *byte_offset > 0 {
                operations.push(ExpressionOp::PlusUConst(*byte_offset as u64));
            } else if *byte_offset < 0 {
                operations.push(ExpressionOp::ConstU64(byte_offset.unsigned_abs()));
                operations.push(ExpressionOp::Minus);
            }
            operations.extend_from_slice(tail);
            // FrameBase starts as an address; the replacement SSA seed starts as a value.
            // Deref preserves that kind. Spell out the implicit final read unless StackValue
            // converted the address to a scalar. Leave the final read terminal so lowering
            // preserves its typed-memory convention, including values wider than one word.
            if !tail.contains(&ExpressionOp::StackValue) {
                operations.push(ExpressionOp::Deref);
            }
            Some(Expression::with_ops(operations))
        }
        _ => None,
    }
}

/// Analyze one immutable operation stream after executable RAUW. All original references and
/// stores remain linked until every capture/update has been materialized; no solver facts survive
/// an IR mutation. Each reference is owned once, including expressions mentioning several slots.
pub(super) fn convert<R: Rewriter>(
    rewriter: &mut R,
    function: OperationRef,
    promotions: &[super::Promotion],
    analysis_manager: AnalysisManager,
) -> Result<(), Report> {
    if promotions.is_empty() {
        return Ok(());
    }
    let selected = promotions
        .iter()
        .enumerate()
        .map(|(i, p)| (p.local.as_usize() as u32, i))
        .collect::<FxHashMap<_, _>>();
    let mut references = Vec::new();
    let mut identities = Vec::new();
    let mut records = Vec::new();
    let mut references_by_local = alloc::vec![Vec::new(); promotions.len()];
    function.raw_prewalk_all::<Forward, _>(|op: OperationRef| {
        let Some(identity) = RecoveryIdentity::of(&op.borrow()) else {
            return;
        };
        let mut locals = Vec::new();
        if let Some(expression) = &identity.expression {
            for operation in &expression.operations {
                if let ExpressionOp::LocalSlot(index)
                | ExpressionOp::FrameBase {
                    base: FrameBase::LocalSlot(index),
                    ..
                } = operation
                    && let Some(local) = selected.get(index)
                    && !locals.contains(local)
                {
                    locals.push(*local);
                }
            }
        }
        if !locals.is_empty() {
            for local in locals {
                references_by_local[local].push(references.len());
            }
            references.push(op);
            identities.push(identity.clone());
        }
        records.push((op, identity));
    });
    if references.is_empty() {
        return Ok(());
    }
    // Index only locals with debug references, and only records which change a tracked visible
    // binding. Snapshot identities once; location expressions remain part of recovery equality.
    let selected = promotions
        .iter()
        .zip(references_by_local)
        .filter(|(_, refs)| !refs.is_empty())
        .collect::<Vec<_>>();
    let entry = function.borrow().downcast_ref::<Function>().unwrap().entry_block();
    // RAUW and nonterminator insertion/erasure preserve the CFG. Share the analysis manager's
    // lazy region trees with the dense dispatcher and every batch.
    let dominance = analysis_manager.get_analysis::<DominanceInfo>()?;
    let mut updates = Vec::new();
    let mut captures = Vec::new();
    let mut captured = FxHashSet::default();
    for batch in selected.chunks(RECOVERY_BATCH_SIZE) {
        let locals = batch.iter().enumerate().map(|(i, (p, _))| (p.local, i)).collect();
        let mut batch_references =
            batch.iter().flat_map(|(_, refs)| refs.iter().copied()).collect::<Vec<_>>();
        batch_references.sort_unstable();
        batch_references.dedup();
        let binding_indices = batch_references
            .iter()
            .enumerate()
            .map(|(index, global)| (*global, index))
            .collect::<FxHashMap<_, _>>();
        let mut visibility = FxHashMap::<VisibilitySlot, Vec<usize>>::default();
        for index in &batch_references {
            visibility
                .entry(identities[*index].binding.visibility(None))
                .or_default()
                .push(*index);
        }
        let record_updates = records
            .iter()
            .filter_map(|(op, identity)| {
                let indices = visibility.get(&identity.binding.visibility(None))?;
                Some((
                    *op,
                    indices
                        .iter()
                        .map(|index| {
                            (
                                binding_indices[index],
                                if identities[*index] == *identity {
                                    2
                                } else {
                                    1
                                },
                            )
                        })
                        .collect(),
                ))
            })
            .collect();
        let mut solver = DataFlowSolver::default();
        solver.load::<DeadCodeAnalysis>();
        solver.load::<SparseConstantPropagation>();
        solver.load_with_strategy(LocalRecovery {
            locals,
            entry,
            references: batch_references.iter().map(|index| references[*index]).collect(),
            record_updates,
            dominance: dominance.clone(),
        });
        solver.initialize_and_run(&function.borrow(), analysis_manager.clone())?;
        for (local_index, (promotion, indices)) in batch.iter().enumerate() {
            let index = promotion.local.as_usize() as u32;
            let mut updated = FxHashSet::default();
            for i in indices.iter().copied() {
                let reference = references[i];
                let expr = normalized_expression(identities[i].expression.as_ref().unwrap(), index);
                if captured.insert(reference) {
                    let state =
                        solver.get::<Lattice<Recovery>, _>(&ProgramPoint::before(reference));
                    let value = state.as_ref().and_then(|state| state.value().0.as_ref()).and_then(
                        |facts| {
                            facts.aliases[local_index].iter().copied().find(|value| {
                                value.borrow().properly_dominates(&reference.borrow(), &dominance)
                            })
                        },
                    );
                    captures.push((reference, value, expr.clone()));
                }
                // Path-specific declarations can share a source location, but different expressions
                // or inline owners cannot. Author at most one store update for each exact location.
                if !updated.insert(&identities[i]) {
                    continue;
                }
                for store in &promotion.stores {
                    let state = solver.get::<Lattice<Recovery>, _>(&ProgramPoint::before(*store));
                    let Some(facts) = state.as_ref().and_then(|state| state.value().0.as_ref())
                    else {
                        continue;
                    };
                    match facts.bindings[binding_indices[&i]] {
                        2 => updates.push((
                            *store,
                            reference,
                            Some(super::stored_value(*store)),
                            expr.clone(),
                        )),
                        3 => updates.push((*store, reference, None, None)),
                        _ => (),
                    }
                }
            }
        }
        // Drop every lattice and anchor before the next solve. All batches observe exactly the
        // same original records, including visibility changes owned by another batch.
        drop(solver);
    }
    // Emit all updates before erasing references, since they supply each binding's inline owner.
    for (anchor, reference, value, expr) in updates {
        let mut scope = rewriter.with_inline_call_chain(reference);
        scope.set_insertion_point_before(anchor);
        let variable = debug_record_variable(&reference.borrow()).unwrap();
        let span = anchor.borrow().span();
        if let (Some(value), Some(expr)) = (value, expr) {
            scope.debug_value_with_expr(value, variable, Some(expr), span)?;
        } else {
            scope.debug_kill(variable, span)?;
        }
    }
    for (reference, value, expr) in captures {
        if let (Some(value), Some(expr)) = (value, expr) {
            let _update = DebugInfoMutation::new(reference, DebugInfoUpdate::Salvage);
            let variable = debug_record_variable(&reference.borrow()).unwrap();
            let span = reference.borrow().span();
            {
                let mut scope = rewriter.with_inline_call_chain(reference);
                scope.set_insertion_point_before(reference);
                scope.debug_value_with_expr(value, variable, Some(expr), span)?;
            }
            rewriter.erase_op(reference);
        } else {
            invalidate_debug_record_with(reference, rewriter);
        }
    }
    Ok(())
}
