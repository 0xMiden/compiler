use crate::{
    AttributeRef, BlockRef, CallableOpInterface, FxHashMap, FxHashSet, Operation, OperationRef,
    Region, RegionBranchOpInterface, RegionBranchPoint, RegionBranchTerminatorOpInterface,
    RegionKindInterface, RegionRef, SmallVec,
    cfg::Graph,
    traits::{NoTerminator, ReturnLike},
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
        if let Some(result) = enclosure_reachability(from, to, EnclosureDirection::Entering, cache)
        {
            return result;
        }
        if let Some(result) = enclosure_reachability(to, from, EnclosureDirection::Exiting, cache) {
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

#[derive(Clone, Copy)]
enum EnclosureDirection {
    Entering,
    Exiting,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum BoundaryReachability {
    CanCross,
    CannotCross,
    Unknown,
}

struct EnclosureStep {
    owner: OperationRef,
    region: RegionRef,
    position: OperationRef,
}

struct RegionExitAnalysis {
    successors: SmallVec<[RegionBranchPoint; 4]>,
    has_unknown_exit: bool,
}

enum RegionGraphStart {
    Parent,
    Child { region: RegionRef, block: BlockRef },
}

/// If `ancestor` properly encloses `descendant`, classify whether control can enter or leave the
/// descendant position through every SSA region separating the two operations.
///
/// A graph-like region makes the positional query indeterminate. For SSA regions, `Impossible`
/// is returned only when a fully modeled CFG or region boundary proves that no path exists;
/// incomplete terminator or owner semantics remain conservatively `Maybe`.
fn enclosure_reachability(
    ancestor: OperationRef,
    descendant: OperationRef,
    direction: EnclosureDirection,
    cache: &mut ReachabilityCache,
) -> Option<Reachability> {
    if !ancestor.borrow().is_proper_ancestor_of(&descendant.borrow()) {
        return None;
    }

    // Collect the direct owner/child-region boundaries from the descendant out to the ancestor.
    // `position` is always directly contained in `region`, which gives each boundary analysis a
    // concrete starting or target block.
    let mut steps = SmallVec::<[EnclosureStep; 4]>::new();
    let mut position = descendant;
    loop {
        let Some(region) = position.borrow().parent_region() else {
            // Defensive fallback for malformed ancestry.
            return Some(Reachability::Maybe);
        };
        if !region_has_ssa_dominance(region) {
            return Some(Reachability::Indeterminate);
        }
        let Some(owner) = region.parent() else {
            return Some(Reachability::Maybe);
        };
        steps.push(EnclosureStep {
            owner,
            region,
            position,
        });
        if owner == ancestor {
            break;
        }
        position = owner;
    }

    match direction {
        EnclosureDirection::Entering => {
            for step in steps.iter().rev() {
                match region_entry_reachability(step, cache) {
                    BoundaryReachability::CanCross => {}
                    BoundaryReachability::CannotCross => {
                        return Some(Reachability::Impossible);
                    }
                    BoundaryReachability::Unknown => return Some(Reachability::Maybe),
                }
            }
        }
        EnclosureDirection::Exiting => {
            for step in &steps {
                match region_exit_reachability(step, cache) {
                    BoundaryReachability::CanCross => {}
                    BoundaryReachability::CannotCross => {
                        return Some(Reachability::Impossible);
                    }
                    BoundaryReachability::Unknown => return Some(Reachability::Maybe),
                }
            }
        }
    }

    Some(Reachability::Maybe)
}

fn region_entry_reachability(
    step: &EnclosureStep,
    cache: &mut ReachabilityCache,
) -> BoundaryReachability {
    let owner_is_region_branch = step.owner.borrow().implements::<dyn RegionBranchOpInterface>();
    let owner_is_callable = is_callable_body(step.owner, step.region);
    if !owner_is_region_branch && !owner_is_callable {
        return BoundaryReachability::Unknown;
    }

    let Some(target_block) = step.position.borrow().parent() else {
        return BoundaryReachability::Unknown;
    };
    let Some(entry_block) = step.region.borrow().entry_block_ref() else {
        return BoundaryReachability::CannotCross;
    };
    if !block_reaches(entry_block, target_block, cache) {
        return BoundaryReachability::CannotCross;
    }

    if owner_is_region_branch {
        return region_branch_reachability(
            step.owner,
            RegionGraphStart::Parent,
            RegionBranchPoint::Child(step.region),
            cache,
        );
    }

    if owner_is_callable {
        BoundaryReachability::CanCross
    } else {
        BoundaryReachability::Unknown
    }
}

fn region_exit_reachability(
    step: &EnclosureStep,
    cache: &mut ReachabilityCache,
) -> BoundaryReachability {
    let Some(start_block) = step.position.borrow().parent() else {
        return BoundaryReachability::Unknown;
    };

    if step.owner.borrow().implements::<dyn RegionBranchOpInterface>() {
        return region_branch_reachability(
            step.owner,
            RegionGraphStart::Child {
                region: step.region,
                block: start_block,
            },
            RegionBranchPoint::Parent,
            cache,
        );
    }

    if is_callable_body(step.owner, step.region) {
        return callable_region_exit_reachability(step.owner, step.region, start_block, cache);
    }

    BoundaryReachability::Unknown
}

fn region_branch_reachability(
    owner: OperationRef,
    start: RegionGraphStart,
    target: RegionBranchPoint,
    cache: &mut ReachabilityCache,
) -> BoundaryReachability {
    let mut worklist = SmallVec::<[(RegionRef, Option<BlockRef>); 8]>::new();

    match start {
        RegionGraphStart::Parent => {
            let successors = {
                let owner = owner.borrow();
                let operands = unknown_operands(owner.num_operands());
                let branch = owner
                    .as_trait::<dyn RegionBranchOpInterface>()
                    .expect("expected a region branch operation");
                branch
                    .get_entry_successor_regions(&operands)
                    .map(RegionBranchPoint::from)
                    .collect::<SmallVec<[_; 4]>>()
            };
            for successor in successors {
                if successor == target {
                    return BoundaryReachability::CanCross;
                }
                if let RegionBranchPoint::Child(region) = successor {
                    worklist.push((region, None));
                }
            }
        }
        RegionGraphStart::Child { region, block } => {
            worklist.push((region, Some(block)));
        }
    }

    // A child reached from another child starts at its entry. The starting exit child instead
    // starts at the descendant's block; track both states independently so a later region-graph
    // cycle may legitimately re-enter that same child through its entry block.
    let mut visited = SmallVec::<[(RegionRef, Option<BlockRef>); 8]>::new();
    let mut has_unknown_path = false;

    while let Some((region, start_block)) = worklist.pop() {
        if visited.contains(&(region, start_block)) {
            continue;
        }
        visited.push((region, start_block));

        let start_block = match start_block.or_else(|| region.borrow().entry_block_ref()) {
            Some(block) => block,
            None => {
                has_unknown_path = true;
                continue;
            }
        };
        let exits = reachable_region_exits(owner, region, start_block, cache);
        has_unknown_path |= exits.has_unknown_exit;

        for successor in exits.successors {
            if successor == target {
                return BoundaryReachability::CanCross;
            }
            if let RegionBranchPoint::Child(region) = successor {
                worklist.push((region, None));
            }
            // Parent reached after a child is terminal for this execution of the region owner;
            // it must not be expanded as though the operation were being entered again.
        }
    }

    if has_unknown_path {
        BoundaryReachability::Unknown
    } else {
        BoundaryReachability::CannotCross
    }
}

fn reachable_region_exits(
    owner: OperationRef,
    region: RegionRef,
    start_block: BlockRef,
    cache: &mut ReachabilityCache,
) -> RegionExitAnalysis {
    let mut analysis = RegionExitAnalysis {
        successors: SmallVec::new(),
        has_unknown_exit: false,
    };

    for block in region.borrow().body().iter() {
        let block = block.as_block_ref();
        if !block_reaches(start_block, block, cache) {
            continue;
        }
        let terminator = block.borrow().terminator();
        let Some(terminator) = terminator else {
            if owner.borrow().implements::<dyn NoTerminator>() {
                let successors = {
                    let owner = owner.borrow();
                    let branch = owner
                        .as_trait::<dyn RegionBranchOpInterface>()
                        .expect("expected a region branch operation");
                    branch
                        .get_successor_regions(RegionBranchPoint::Child(region))
                        .map(RegionBranchPoint::from)
                        .collect::<SmallVec<[_; 4]>>()
                };
                append_unique_successors(&mut analysis.successors, successors);
            } else {
                analysis.has_unknown_exit = true;
            }
            continue;
        };

        let terminator = terminator.borrow();
        if let Some(region_terminator) =
            terminator.as_trait::<dyn RegionBranchTerminatorOpInterface>()
        {
            let operands = unknown_operands(terminator.num_operands());
            let successors = region_terminator
                .get_successor_regions(&operands)
                .into_iter()
                .map(|successor| successor.successor());
            append_unique_successors(&mut analysis.successors, successors);
        } else if terminator.implements::<dyn ReturnLike>() {
            // Plain returns are only valid as direct terminators of a callable body. A
            // ReturnLike under a RegionBranch owner is malformed or otherwise unmodeled.
            analysis.has_unknown_exit = true;
        } else if terminator.num_successors() == 0 {
            // A terminator with no CFG or region successors may abort, throw, or transfer control
            // by semantics unavailable here. It cannot justify a false `Impossible`.
            analysis.has_unknown_exit = true;
        }
    }

    analysis
}

fn callable_region_exit_reachability(
    owner: OperationRef,
    region: RegionRef,
    start_block: BlockRef,
    cache: &mut ReachabilityCache,
) -> BoundaryReachability {
    let mut has_unknown_exit = false;

    for block in region.borrow().body().iter() {
        let block = block.as_block_ref();
        if !block_reaches(start_block, block, cache) {
            continue;
        }
        let Some(terminator) = block.borrow().terminator() else {
            if owner.borrow().implements::<dyn NoTerminator>() {
                return BoundaryReachability::CanCross;
            }
            has_unknown_exit = true;
            continue;
        };
        let terminator = terminator.borrow();
        if terminator.implements::<dyn RegionBranchTerminatorOpInterface>() {
            // Such a terminator only has a defined destination under a RegionBranch owner.
            has_unknown_exit = true;
        } else if terminator.implements::<dyn ReturnLike>() {
            return BoundaryReachability::CanCross;
        } else if terminator.num_successors() == 0 {
            has_unknown_exit = true;
        }
    }

    if has_unknown_exit {
        BoundaryReachability::Unknown
    } else {
        BoundaryReachability::CannotCross
    }
}

fn is_callable_body(owner: OperationRef, region: RegionRef) -> bool {
    owner
        .borrow()
        .as_trait::<dyn CallableOpInterface>()
        .is_some_and(|callable| callable.get_callable_region() == Some(region))
}

fn block_reaches(from: BlockRef, to: BlockRef, cache: &mut ReachabilityCache) -> bool {
    from == to || cache.leads_to(from, to)
}

fn unknown_operands(count: usize) -> SmallVec<[Option<AttributeRef>; 4]> {
    core::iter::repeat_n(None, count).collect()
}

fn append_unique_successors(
    successors: &mut SmallVec<[RegionBranchPoint; 4]>,
    additional: impl IntoIterator<Item = RegionBranchPoint>,
) {
    for successor in additional {
        if !successors.contains(&successor) {
            successors.push(successor);
        }
    }
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

#[cfg(test)]
mod tests {
    use alloc::vec::Vec;

    use super::*;
    use crate::{
        Builder, BuilderExt, Op, RegionSuccessorInfo, RegionSuccessorIter, SourceSpan,
        SuccessorOperandRange, SuccessorOperandRangeMut, ValueRef,
        derive::operation,
        dialects::test::TestDialect,
        testing::Test,
        traits::{AnyType, BranchOpInterface, Terminator},
    };

    #[operation(dialect = TestDialect, implements(RegionBranchOpInterface))]
    pub struct TestRegionBranch {
        #[region]
        first: Region,
        #[region]
        second: Region,
    }

    impl RegionBranchOpInterface for TestRegionBranch {
        fn get_successor_regions(&self, point: RegionBranchPoint) -> RegionSuccessorIter<'_> {
            let first = self.first().as_region_ref();
            let second = self.second().as_region_ref();
            let successors = match point {
                RegionBranchPoint::Parent => {
                    SmallVec::from_buf([RegionSuccessorInfo::Entering(first)])
                }
                RegionBranchPoint::Child(region) if region == first => {
                    SmallVec::from_buf([RegionSuccessorInfo::Entering(second)])
                }
                RegionBranchPoint::Child(_) => {
                    SmallVec::from_buf([RegionSuccessorInfo::Returning(SmallVec::new())])
                }
            };
            RegionSuccessorIter::new(self.as_operation(), successors)
        }
    }

    #[operation(
        dialect = TestDialect,
        traits(Terminator),
        implements(BranchOpInterface)
    )]
    pub struct TestBranch {
        #[successor]
        target: Successor,
    }

    impl BranchOpInterface for TestBranch {}

    #[operation(
        dialect = TestDialect,
        traits(Terminator),
        implements(RegionBranchTerminatorOpInterface)
    )]
    pub struct TestRegionYield {
        #[operands]
        yielded: AnyType,
    }

    impl RegionBranchTerminatorOpInterface for TestRegionYield {
        fn get_successor_operands(&self, _point: RegionBranchPoint) -> SuccessorOperandRange<'_> {
            SuccessorOperandRange::forward(self.yielded())
        }

        fn get_mutable_successor_operands(
            &mut self,
            _point: RegionBranchPoint,
        ) -> SuccessorOperandRangeMut<'_> {
            SuccessorOperandRangeMut::forward(self.yielded_mut())
        }

        fn get_successor_regions(
            &self,
            _operands: &[Option<AttributeRef>],
        ) -> SmallVec<[RegionSuccessorInfo; 2]> {
            let region = self.parent_region().expect("test yield must be in a region");
            let owner = self.parent_op().expect("test yield region must have an owner");
            let owner = owner.borrow();
            let owner = owner
                .downcast_ref::<TestRegionBranch>()
                .expect("test yield must be nested in TestRegionBranch");
            if region == owner.first().as_region_ref() {
                core::iter::once(RegionSuccessorInfo::Entering(owner.second().as_region_ref()))
                    .collect()
            } else {
                core::iter::once(RegionSuccessorInfo::Returning(SmallVec::new())).collect()
            }
        }
    }

    #[operation(dialect = TestDialect)]
    pub struct TestUnknownRegionOwner {
        #[region]
        body: Region,
    }

    #[operation(dialect = TestDialect)]
    pub struct TestMarker {}

    #[test]
    fn region_branch_entry_does_not_bypass_an_intermediate_child_sink() {
        let mut test =
            Test::new("region_branch_entry_does_not_bypass_an_intermediate_child_sink", &[], &[]);
        let (owner, target) = {
            let mut builder = test.function_builder();
            let owner = builder.builder_mut().create::<TestRegionBranch, ()>(SourceSpan::UNKNOWN)()
                .unwrap();
            let first = owner.borrow().first().as_region_ref();
            let second = owner.borrow().second().as_region_ref();

            let sink = builder.builder_mut().create_block(first, None, &[]);
            builder.builder_mut().set_insertion_point_to_end(sink);
            builder
                .builder_mut()
                .create::<TestBranch, (BlockRef, Vec<ValueRef>)>(SourceSpan::UNKNOWN)(
                sink,
                Vec::new(),
            )
            .unwrap();

            // The abstract owner graph advertises first -> second, but this concrete terminator is
            // disconnected from the first region's entry and therefore cannot enable that edge.
            let disconnected = builder.builder_mut().create_block(first, None, &[]);
            builder.builder_mut().set_insertion_point_to_end(disconnected);
            builder
                .builder_mut()
                .create::<TestRegionYield, (Vec<ValueRef>,)>(SourceSpan::UNKNOWN)(
                Vec::new()
            )
            .unwrap();

            let second_entry = builder.builder_mut().create_block(second, None, &[]);
            builder.builder_mut().set_insertion_point_to_end(second_entry);
            let target = builder
                .builder_mut()
                .create::<TestRegionYield, (Vec<ValueRef>,)>(SourceSpan::UNKNOWN)(
                Vec::new()
            )
            .unwrap();
            (owner.as_operation_ref(), target.as_operation_ref())
        };

        assert_eq!(
            Operation::reachability(owner, target),
            Reachability::Impossible,
            "an abstract child-to-child edge must not bypass a sink in the intermediate child"
        );
    }

    #[test]
    fn unknown_region_owner_semantics_remain_maybe() {
        let mut test = Test::new("unknown_region_owner_semantics_remain_maybe", &[], &[]);
        let (owner, nested) = {
            let mut builder = test.function_builder();
            let owner =
                builder.builder_mut().create::<TestUnknownRegionOwner, ()>(SourceSpan::UNKNOWN)()
                    .unwrap();
            let body = owner.borrow().body().as_region_ref();
            let entry = builder.builder_mut().create_block(body, None, &[]);
            builder.builder_mut().set_insertion_point_to_end(entry);
            builder
                .builder_mut()
                .create::<TestBranch, (BlockRef, Vec<ValueRef>)>(SourceSpan::UNKNOWN)(
                entry,
                Vec::new(),
            )
            .unwrap();
            let disconnected = builder.builder_mut().create_block(body, None, &[]);
            builder.builder_mut().set_insertion_point_to_end(disconnected);
            let nested =
                builder.builder_mut().create::<TestMarker, ()>(SourceSpan::UNKNOWN)().unwrap();
            (owner.as_operation_ref(), nested.as_operation_ref())
        };

        assert_eq!(
            Operation::reachability(owner, nested),
            Reachability::Maybe,
            "an unmodeled region owner must stay conservative even for a disconnected block"
        );
    }
}
