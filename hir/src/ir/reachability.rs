use crate::{
    BlockRef, Operation, OperationRef, Region, RegionBranchOpInterface, RegionKindInterface,
    RegionRef, SmallVec, adt::SmallSet, cfg::Graph,
};

/// The answer to a control-flow reachability query between two operations.
///
/// See [Operation::reachability].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reachability {
    /// Provably unreachable, i.e. no control flow path exists between `a` and `b`
    Impossible,
    /// Provably reachable, i.e. there is at least one control flow path guaranteed to reach
    /// from `a` to `b`
    Guaranteed,
    /// Reachability is not proven, but there is at least one control flow path that reaches
    /// from `a` to `b`; full reachability analysis is required to prove whether the path(s) are
    /// truly executable
    Maybe,
    /// Cannot be determined without global reachability analysis, because the two ops are in
    /// different functions
    MaybeInterprocedurally,
    /// Cannot be determined because control flow between the two ops is not well-defined (i.e.
    /// both belong to a graph-like region, or their common ancestor region is graph-like)
    Indeterminate,
}

/// Queries
impl Operation {
    /// Computes whether some control-flow path from `from` can reach `to`.
    ///
    /// This is a conservative, purely structural query over the CFG and region graph: it does
    /// not consider whether paths are actually executable at runtime (that requires a proper
    /// reachability analysis performed in concert with SCCP/DCE), so paths that exist but may
    /// never execute are reported as [Reachability::Maybe]. The precise answers are reliable in
    /// both directions: [Reachability::Impossible] means no control-flow path exists at all,
    /// and [Reachability::Guaranteed] means a path exists that control cannot branch away from.
    ///
    /// Queries that relate positions in different functions, or in regions where operation
    /// order does not define control flow, are answered with
    /// [Reachability::MaybeInterprocedurally] and [Reachability::Indeterminate] respectively,
    /// leaving the interpretation of such queries to the caller.
    pub fn reachability(from: OperationRef, to: OperationRef) -> Reachability {
        // One operation enclosing the other is an intra-procedural relationship no matter where
        // the encloser resides: control entering the enclosing op can reach the nested
        // position, and control leaving the nested position flows back through the enclosing
        // op. This must precede the scope comparison, which would otherwise misclassify
        // enclosure by an op residing in a graph-like region (e.g. a function op and an op in
        // its body) as interprocedural.
        if from.borrow().is_proper_ancestor_of(&to.borrow())
            || to.borrow().is_proper_ancestor_of(&from.borrow())
        {
            return Reachability::Maybe;
        }

        // Intra-procedural reasoning is bounded by the nearest ancestor residing in a
        // graph-like region (in practice, the enclosing function): positions under different
        // such ancestors can only be related interprocedurally.
        if control_flow_scope(from) != control_flow_scope(to) {
            return Reachability::MaybeInterprocedurally;
        }

        // Without a common ancestor region no control-flow path can exist: any path from `from`
        // to `to` would itself lie in a region containing both.
        let Some(common_region) = Region::find_common_ancestor(&[from, to]) else {
            return Reachability::Impossible;
        };

        // In a graph-like region operation order does not define control flow, so positional
        // queries within it are meaningless.
        if !region_has_ssa_dominance(common_region) {
            return Reachability::Indeterminate;
        }

        // Normalize both operations to the common region: an operation nested in a sub-region
        // (e.g. structured control flow) is represented by its ancestor op in the common region.
        let common_region_ref = common_region;
        let common_region = common_region.borrow();
        let (Some(from_ancestor), Some(to_ancestor)) =
            (common_region.find_ancestor_op(from), common_region.find_ancestor_op(to))
        else {
            // Unreachable per find_common_ancestor's postcondition (the returned region
            // contains every queried op); kept as a defensive fallback.
            return Reachability::Maybe;
        };

        // Both operations normalize to the same ancestor op: either one op encloses the other
        // (and control entering it can reach the nested position), or they sit in different
        // sub-regions of that op, where transfer between the regions depends on the op's
        // semantics (e.g. it can happen across loop iterations).
        if from_ancestor == to_ancestor {
            return Reachability::Maybe;
        }

        let (Some(from_block), Some(to_block)) =
            (from_ancestor.borrow().parent(), to_ancestor.borrow().parent())
        else {
            return Reachability::Maybe;
        };

        // Within one block an earlier operation always flows into a later one; this is only a
        // guarantee when neither position was normalized, since entering a sub-region of an
        // ancestor op is generally conditional on that op's semantics.
        if from_block == to_block && from_ancestor.borrow().is_before_in_block(&to_ancestor) {
            return if from_ancestor == from && to_ancestor == to {
                Reachability::Guaranteed
            } else {
                Reachability::Maybe
            };
        }

        // A forward path may exist through block successors; earlier positions are only
        // reachable through a cycle, either via block successors, or by re-entry of the common
        // region itself.
        if block_leads_to(from_block, to_block) {
            return Reachability::Maybe;
        }
        if region_can_re_execute(common_region_ref) {
            return Reachability::Maybe;
        }

        Reachability::Impossible
    }
}

/// Returns the nearest proper ancestor of `op` that resides in a graph-like region (or has no
/// parent at all), i.e. the operation whose body bounds any intra-procedural control-flow
/// reasoning about `op`. In practice this is the enclosing function, whose parent module body
/// is a graph-like region.
fn control_flow_scope(op: OperationRef) -> Option<OperationRef> {
    let mut current = op.borrow().parent_op();
    while let Some(ancestor) = current {
        let Some(parent_block) = ancestor.borrow().parent() else {
            return Some(ancestor);
        };
        if !parent_block.borrow().has_ssa_dominance() {
            return Some(ancestor);
        }
        current = ancestor.borrow().parent_op();
    }
    None
}

/// Returns true if `region` requires SSA dominance, i.e. operation order within it defines
/// control flow. Regions of operations that do not declare a region kind default to SSA.
fn region_has_ssa_dominance(region: RegionRef) -> bool {
    region
        .parent()
        .and_then(|op| {
            op.borrow()
                .as_trait::<dyn RegionKindInterface>()
                .map(|rki| rki.has_ssa_dominance())
        })
        .unwrap_or(true)
}

/// Returns true if control leaving the end of `from` can reach the start of `to` by following
/// block successors.
///
/// The walk is not reflexive: `from == to` returns true only when a cycle leads back into the
/// block, which is what [region_can_re_execute] relies on for cycle detection.
fn block_leads_to(from: BlockRef, to: BlockRef) -> bool {
    let mut visited = SmallSet::<BlockRef, 8>::default();
    let mut worklist = SmallVec::<[BlockRef; 8]>::from_iter(BlockRef::children(from));
    while let Some(block) = worklist.pop() {
        if block == to {
            return true;
        }
        if !visited.insert(block) {
            continue;
        }
        worklist.extend(BlockRef::children(block));
    }
    false
}

/// Returns true if `region` can execute more than once within a single execution of the
/// operation bounding control-flow reasoning about it (see [control_flow_scope]).
///
/// That is the case when an enclosing region is repetitive (e.g. the regions of an `scf.while`,
/// whose back edges are expressed in the region graph of the owning op rather than as block
/// successors), or when an enclosing op itself sits on a CFG cycle in its parent region.
fn region_can_re_execute(region: RegionRef) -> bool {
    let mut current = Some(region);
    while let Some(r) = current {
        let Some(owner) = r.parent() else {
            return false;
        };
        let owner_op = owner.borrow();
        let Some(owner_block) = owner_op.parent() else {
            // A top-level owner cannot be re-entered from anywhere.
            return false;
        };
        if !owner_block.borrow().has_ssa_dominance() {
            // The owner resides in a graph-like region: control-flow reasoning stops here (in
            // practice the owner is the enclosing function, and each execution of its body is
            // a separate invocation).
            return false;
        }
        if !owner_op.implements::<dyn RegionBranchOpInterface>() {
            // Unknown region semantics: conservatively treat re-entry as possible.
            return true;
        }
        if r.borrow().is_repetitive_region() {
            return true;
        }
        if block_leads_to(owner_block, owner_block) {
            return true;
        }
        current = owner_op.parent_region();
    }
    false
}
