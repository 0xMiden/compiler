use crate::{
    BlockRef, FxHashMap, FxHashSet, Operation, OperationRef, Region, RegionBranchOpInterface,
    RegionKindInterface, RegionRef, SmallVec, cfg::Graph,
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

/// A lazily-populated cache of forward block reachability, for callers issuing many
/// [Operation::reachability_cached] queries over one body of IR.
///
/// The first query from a given block computes and stores that block's full forward closure;
/// subsequent queries from the same block are set lookups. The cache assumes the block
/// structure of the IR does not change between queries: discard it after splitting, erasing,
/// or rewiring blocks.
#[derive(Default)]
pub struct ReachabilityCache {
    forward: FxHashMap<BlockRef, FxHashSet<BlockRef>>,
}

impl ReachabilityCache {
    /// Returns true if control leaving the end of `from` can reach the start of `to` by
    /// following block successors.
    ///
    /// The walk is not reflexive: `from == to` returns true only when a cycle leads back into
    /// the block, which is what [region_can_re_execute] relies on for cycle detection.
    fn leads_to(&mut self, from: BlockRef, to: BlockRef) -> bool {
        self.forward
            .entry(from)
            .or_insert_with(|| {
                let mut reachable = FxHashSet::default();
                let mut worklist = SmallVec::<[BlockRef; 8]>::from_iter(BlockRef::children(from));
                while let Some(block) = worklist.pop() {
                    if reachable.insert(block) {
                        worklist.extend(BlockRef::children(block));
                    }
                }
                reachable
            })
            .contains(&to)
    }
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
        Self::reachability_cached(from, to, &mut ReachabilityCache::default())
    }

    /// Like [Operation::reachability], reusing `cache` across queries.
    ///
    /// Use this when issuing many queries over one body of IR (e.g. pairing spills with
    /// reloads), so that block-reachability walks are shared between them.
    pub fn reachability_cached(
        from: OperationRef,
        to: OperationRef,
        cache: &mut ReachabilityCache,
    ) -> Reachability {
        // One operation enclosing the other relates the two positions through the chain of
        // regions between them: control entering the enclosing op can reach the nested
        // position, and control leaving the nested position flows back through the enclosing
        // op — unless the chain crosses a graph-like region (e.g. a module enclosing a
        // function), where no control-flow order is defined. This must precede the scope
        // comparison, which would otherwise misclassify enclosure by an op residing in a
        // graph-like region as interprocedural.
        if let Some(result) =
            enclosure_reachability(from, to).or_else(|| enclosure_reachability(to, from))
        {
            return result;
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

        // Both operations normalize to the same ancestor op, i.e. they sit in different
        // sub-regions of it (enclosure was handled above). Whether control can transfer from
        // one sub-region to a sibling is decided by the op's own region graph, or by the op
        // executing more than once.
        if from_ancestor == to_ancestor {
            return sibling_region_reachability(from_ancestor, from, to, cache);
        }

        let (Some(from_block), Some(to_block)) =
            (from_ancestor.borrow().parent(), to_ancestor.borrow().parent())
        else {
            return Reachability::Maybe;
        };

        // Within one block an earlier operation may flow into a later one, but order alone does
        // not prove total fallthrough: the source or an intervening operation may loop
        // indefinitely, return from the function, or abort. Positions that were normalized are
        // likewise conditional on their ancestor op's semantics.
        if from_block == to_block && from_ancestor.borrow().is_before_in_block(&to_ancestor) {
            return Reachability::Maybe;
        }

        // A forward path may exist through block successors; earlier positions are only
        // reachable through a cycle, either via block successors, or by re-entry of the common
        // region itself.
        if cache.leads_to(from_block, to_block) {
            return Reachability::Maybe;
        }
        if region_can_re_execute(common_region_ref, cache) {
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

/// If `ancestor` properly encloses `descendant`, classifies the enclosure:
/// [Reachability::Maybe] when every region crossed between them defines control flow, or
/// [Reachability::Indeterminate] when the chain crosses a graph-like region (e.g. a module
/// enclosing a function). Returns `None` when `ancestor` does not enclose `descendant`.
fn enclosure_reachability(
    ancestor: OperationRef,
    descendant: OperationRef,
) -> Option<Reachability> {
    if !ancestor.borrow().is_proper_ancestor_of(&descendant.borrow()) {
        return None;
    }
    // Walk the regions from `descendant` up to (and including) the region owned by `ancestor`.
    let mut region = descendant.borrow().parent_region();
    while let Some(r) = region {
        if !region_has_ssa_dominance(r) {
            return Some(Reachability::Indeterminate);
        }
        let Some(owner) = r.parent() else {
            break;
        };
        if owner == ancestor {
            return Some(Reachability::Maybe);
        }
        region = owner.borrow().parent_region();
    }
    // Unreachable given the ancestry check above; defensively report plain enclosure.
    Some(Reachability::Maybe)
}

/// Classifies reachability between two positions in different sub-regions of one `owner` op.
///
/// The sibling region is reachable when the region graph of `owner` can transfer control from
/// the region holding `from` to the region holding `to` within one execution of `owner` (e.g.
/// from the `before` region of a while to its `after` region), or when `owner` itself can
/// execute more than once (e.g. the arms of an if that an enclosing loop re-enters); otherwise
/// it is provably unreachable (e.g. the arms of an if outside any loop).
fn sibling_region_reachability(
    owner: OperationRef,
    from: OperationRef,
    to: OperationRef,
    cache: &mut ReachabilityCache,
) -> Reachability {
    if !owner.borrow().implements::<dyn RegionBranchOpInterface>() {
        // Unknown region semantics: conservatively treat transfer as possible.
        return Reachability::Maybe;
    }
    let (Some(from_region), Some(to_region)) =
        (child_region_containing(owner, from), child_region_containing(owner, to))
    else {
        // Unreachable given the normalization above; defensively treat as reachable.
        return Reachability::Maybe;
    };
    if to_region.borrow().is_reachable_from(&from_region.borrow()) {
        return Reachability::Maybe;
    }
    if op_can_re_execute(owner, cache) {
        return Reachability::Maybe;
    }
    Reachability::Impossible
}

/// Returns the direct child region of `owner` that contains `op`.
fn child_region_containing(owner: OperationRef, op: OperationRef) -> Option<RegionRef> {
    let mut region = op.borrow().parent_region();
    while let Some(r) = region {
        let parent = r.parent()?;
        if parent == owner {
            return Some(r);
        }
        region = parent.borrow().parent_region();
    }
    None
}

/// Returns true if `op` can execute more than once: its block lies on a CFG cycle, or the
/// region containing it can re-execute.
fn op_can_re_execute(op: OperationRef, cache: &mut ReachabilityCache) -> bool {
    let (block, region) = {
        let op = op.borrow();
        (op.parent(), op.parent_region())
    };
    if let Some(block) = block
        && cache.leads_to(block, block)
    {
        return true;
    }
    region.is_some_and(|region| region_can_re_execute(region, cache))
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

/// Returns true if `region` can execute more than once within a single execution of the
/// operation bounding control-flow reasoning about it (see [control_flow_scope]).
///
/// That is the case when an enclosing region is repetitive (e.g. the regions of an `scf.while`,
/// whose back edges are expressed in the region graph of the owning op rather than as block
/// successors), or when an enclosing op itself sits on a CFG cycle in its parent region.
fn region_can_re_execute(region: RegionRef, cache: &mut ReachabilityCache) -> bool {
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
        if cache.leads_to(owner_block, owner_block) {
            return true;
        }
        current = owner_op.parent_region();
    }
    false
}
