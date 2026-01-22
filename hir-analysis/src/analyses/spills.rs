use alloc::{boxed::Box, collections::VecDeque, vec::Vec};

use midenc_hir::{
    AttributeValue, Block, BlockRef, FxHashMap, FxHashSet, LoopLikeOpInterface, Op, Operation,
    OperationRef, ProgramPoint, Region, RegionBranchOpInterface, RegionBranchPoint,
    RegionBranchTerminatorOpInterface, Report, SmallVec, SourceSpan, Spanned, SuccessorOperands,
    Value, ValueOrAlias, ValueRange, ValueRef,
    adt::{SmallOrdMap, SmallSet},
    cfg::Graph,
    dialects::builtin::Function,
    dominance::{DominanceInfo, DominanceTree},
    formatter::DisplayValues,
    loops::{Loop, LoopForest, LoopInfo},
    pass::{Analysis, AnalysisManager, PreservedAnalyses},
    traits::{BranchOpInterface, IsolatedFromAbove, Terminator},
};

use super::dce::{CfgEdge, Executable};
use crate::{
    Lattice,
    analyses::{
        LivenessAnalysis, constant_propagation::ConstantValue, dce::PredecessorState,
        liveness::LOOP_EXIT_DISTANCE,
    },
};

#[cfg(test)]
mod tests;

/// This analysis is responsible for simulating the state of the operand stack at each program
/// point, taking into account the results of liveness analysis, and computing whether or not to
/// insert spills/reloads of values which would cause the operand stack depth to exceed 16 elements,
/// the maximum addressable depth.
///
/// The algorithm here is based on the paper [_Register Spilling and Live-Range Splitting for
/// SSA-form Programs_ by Matthias Braun and Sebastian Hack](https://pp.ipd.kit.edu/uploads/publikationen/braun09cc.pdf),
/// which also happens to describe the algorithm we based our liveness analysis on. While the broad
/// strokes are the same, various modifications/tweaks to the algorithm they describe are needed in
/// order to be suitable for our use case. In particular, we must distinguish between the SSA values
/// which uniquely identify each operand, from the raw elements on the operand stack which represent
/// those values. The need for spills is determined solely on the low-level operand stack
/// representation, _not_ the number of live SSA values (although there can be a correspondence in
/// cases where each SSA value has an effective size of 1 stack element). As this is a type-
/// sensitive analysis, it differs from the algorithm in the paper, which is based on an assumption
/// that all operands are machine-word sized, and thus each value only requires a single register to
/// hold.
///
/// Despite these differences, the overall approach is effectively identical. We still are largely
/// concerned with the SSA values, the primary difference being that we are computing spills based
/// on the raw operand stack state, rather than virtual register pressure as described in the paper.
/// As a result, the number of spills needed at a given program point are not necessarily 1:1, as
/// it may be necessary to spill multiple values in order to free sufficient capacity on the operand
/// stack to hold the required operands; or conversely, we may evict operands that free up more
/// operand stack space than is strictly needed due to the size of those values.
///
/// The general algorithm, once liveness has been computed (see [LivenessAnalysis] for more
/// details), can be summarized as follows:
///
/// In reverse CFG postorder, visit each block B, and:
///
/// 1. Determine initialization of W at entry to B (W^entry). W is the set of operands on the
///    operand stack. From this we are able to determine what, if any, actions are required to
///    keep |W| <= K where K is the maximum allowed operand stack depth.
///
/// 2. Determine initialization of S at entry to B (S^entry). S is the set of values which have
///    been spilled up to that point in the program. We can use S to determine whether or not
///    to actually emit a spill instruction when a spill is necessary, as due to the SSA form of
///    the program, every value has a single definition, so we need only emit a spill for a given
///    value once.
///
/// 3. For each predecessor P of B, determine what, if any, spills and/or reloads are needed to
///    ensure that W and S are consistent regardless of what path is taken to reach B, and that
///    |W| <= K. Depending on whether P has multiple successors, it may be necessary to split the
///    edge between P and B, so that the emitted spills/reloads only apply along that edge.
///
/// 4. Perform the MIN algorithm on B, which is used to determine spill/reloads at each instruction
///    in the block. MIN is designed to make optimal decisions about what to spill, so as to
///    minimize the number of spill/reload-related instructions executed by any given program
///    execution trace. It does this by using the next-use distance associated with values in W,
///    which is computed as part of our liveness analysis. Unlike traditional liveness analysis
///    which only tracks what is live at a given program point, next-use distances not only tell
///    you whether a value is live or dead, but how far away the next use of that value is. MIN
///    uses this information to select spill candidates from W furthest away from the current
///    instruction; and on top of this we also add an additional heuristic based on the size of
///    each candidate as represented on the operand stack. Given two values with equal next-use
///    distances, the largest candidates are spilled first, allowing us to free more operand stack
///    space with fewer spills.
///
/// The MIN algorithm works as follows:
///
/// 1. Starting at the top of the block, B, W is initialized with the set W^entry(B), and S with
///    S^entry(B)
///
/// 2. For each instruction, I, in the block, update W and S according to the needs of I, while
///    attempting to preserve as many live values in W as possible. Each instruction fundamentally
///    requires that: On entry, W contains all the operands of I; on exit, W contains all of the
///    results of I; and that at all times, |W| <= K. This means that we may need to reload operands
///    of I that are not in W (because they were spilled), and we may need to spill values from W to
///    ensure that the stack depth <= K. The specific effects for I are computed as follows:
///    a. All operands of I not in W, must be reloaded in front of I, thus adding them to W.
///    This is also one means by which values are added to S, as by definition a reload implies that
///    the value must have been spilled, or it would still be in W. Thus, when we emit reloads, we
///    also ensure that the reloaded value is added to S.
///    b. If a reload would cause |W| to exceed K, we must select values in W to spill. Candidates
///    are selected from the set of values in W which are not operands of I, prioritized first by
///    greatest next-use distance, then by stack consumption, as determined by the representation of
///    the value type on the operand stack.
///    c. By definition, none of I's results can be in W directly in front of I, so we must always
///    ensure that W has sufficient capacity to hold all of I's results. The analysis of sufficient
///    capacity is somewhat subtle:
///       - Any of I's operands that are live-at I, but _not_ live-after I, do _not_ count towards
///         the operand stack usage when calculating available capacity for the results. This is
///         because those operands will be consumed, and their space can be re-used for results.
///       - Any of I's operands that are live-after I, however, _do_ count towards the stack usage
///       - If W still has insufficient capacity for all the results, we must select candidates
///         to spill. Candidates are the set of values in W which are either not operands of I,
///         or are operands of I which are live-after I. Selection criteria is the same as before.
///
///    d. Operands of I which are _not_ live-after I, are removed from W on exit from I, thus W
///    reflects only those values which are live at the current program point.
///    e. Lastly, when we select a value to be spilled, we only emit spill instructions for those
///    values which are not yet in S, i.e. they have not yet been spilled; and which have a finite
///    next-use distance, i.e. the value is still live. If a value to be spilled _is_ in S and/or is
///    unused after that point in the program, we can elide the spill entirely.
///
/// What we've described above represents both the analysis itself, as well as the effects of
/// applying that analysis to the actual control flow graph of the function. However, doing so
/// introduces a problem that must be addressed: SSA-form programs can only have a single definition
/// of each value, but by introducing spills (and consequently, reloads of the spilled values), we
/// have introduced new definitions of those values - each reload constitutes a new definition.
/// As a result, our program is no longer in SSA form, and we must restore that property in order
/// to proceed with compilation.
///
/// **NOTE:** The way that we represent reloads doesn't _literally_ introduce multiple definitions
/// of a given [Value], our IR does not permit representing that. Instead, we represent reloads as
/// an instruction which takes the spilled SSA value we want to reload as an argument, and produces
/// a new SSA value representing the reloaded spill. As a result of this representation, our program
/// always remains technically in SSA form, but the essence of the problem remains the same: When a
/// value is spilled, its live range is terminated; a reload effectively brings the spilled value
/// back to life, starting a new live range. Thus references to the spilled value which are now
/// dominated by a reload in the control flow graph, are no longer semantically correct - they must
/// be rewritten to reference the nearest dominating definition.
///
/// Restoring SSA form is not the responsibility of this analysis, however I will briefly describe
/// the method here, while you have the context at hand. The obvious assumption would be that we
/// simply treat each reload as a new SSA value, and update any uses of the original value with the
/// nearest dominating definition. The way we represent reloads in HIR already does the first step
/// for us, however there is a subtle problem with the second part: join points in the control flow
/// graph. Consider the following:
///
/// ```text,ignore
/// (block 0 (param v0) (param v1)
///   (cond_br v1 (block 1) (block 2)))
///
/// (block 1
///   (spill v0)
///   ...
///   (let v2 (reload v0)) ; here we've assigned the reload of v0 a new SSA value
///   (br (block 3)))
///
/// (block 2
///   ...
///   (br (block 3)))
///
/// (block 3
///    (ret v2)) ; here we've updated a v0 reference to the nearest definition
/// ```
///
/// Above, control flow branches in one of two directions from the entry block, and along one of
/// those branches `v0` is spilled and later reloaded. Control flow joins again in the final block
/// where `v0` is returned. We attempted to restore the program to SSA form as described above,
/// first by assigning reloads a new SSA value, then by finding all uses of the spilled value and
/// rewriting those uses to reference the nearest dominating definition.
///
/// Because the use of `v0` in block 3 is dominated by the reload in block 1, it is rewritten to
/// reference `v2` instead. The problem with that is obvious - the reload in block 1 does not
/// _strictly_ dominate the use in block 3, i.e. there are paths through the function which can
/// reach block 3 without passing through block 1 first, and `v2` will be undefined along those
/// paths!
///
/// However this problem also has an obvious solution: introduce a new block parameter in block 3
/// to represent the appropriate definition of `v0` that applies based on the predecessor used to
/// reach block 3. This ensures that the use in block 3 is strictly dominated by an appropriate
/// definition.
///
/// So now that we've understood the problem with the naive approach, and the essence of the
/// solution to that particular problem, we can walk through the generalized solution that can be
/// used to reconstruct SSA form for any program we can represent in our IR.
///
/// 1. Given the set of spilled values, S, visit the dominance tree in postorder (bottom-up)
/// 2. In each block, working towards the start of the block from the end, visit each instruction
///    until one of the following occurs:
///    a. We find a use of a value in S. We append the use to the list of other uses of that value
///    which are awaiting a rewrite while we search for the nearest dominating definition.
///    b. We find a reload of a value in S. This reload is, by construction, the nearest dominating
///    definition for all uses of the reloaded value that we have found so far. We rewrite all of
///    those uses to reference the reloaded value, and remove them from the list.
///    c. We find the original definition of a value in S. This is similar to what happens when we
///    find a reload, except no rewrite is needed, so we simply remove all pending uses of that
///    value from the list.
///    d. We reach the top of the block. Note that block parameters are treated as definitions, so
///    those are handled first as described in the previous point. However, an additional step is
///    required here: If the current block is in the iterated dominance frontier for S, i.e. for any
///    value in S, the current block is in the dominance frontier of the original definition of that
///    value - then for each such value for which we have found at least one use, we must add a new
///    block parameter representing that value; rewrite all uses we have found so far to use the
///    block parameter instead; remove those uses from the list; and lastly, rewrite the branch
///    instruction in each predecessor to pass the value as a new block argument when branching to
///    the current block.
/// 3. When we start processing a block, the union of the set of unresolved uses found in each
///    successor, forms the initial state of that set for the current block. If a block has no
///    successors, then the set is initially empty. This is how we propagate uses up the dominance
///    tree until we find an appropriate definition. Since we ensure that block parameters are added
///    along the dominance frontier for each spilled value, we guarantee that the first definition
///    we reach always strictly dominates the uses we have propagated to that point.
///
/// NOTE: A nice side effect of this algorithm is that any reloads we reach for which we have
/// no uses, are dead and can be eliminated. Similarly, a reload we never reach must also be
/// dead code - but in practice that won't happen, since we do not visit unreachable blocks
/// during the spill analysis anyway.
#[derive(Debug, Default, Clone)]
pub struct SpillAnalysis {
    // The set of control flow edges that must be split to accommodate spills/reloads.
    pub splits: SmallVec<[SplitInfo; 1]>,
    // The set of values that have been spilled
    pub spilled: FxHashSet<ValueRef>,
    // The spills themselves
    pub spills: SmallVec<[SpillInfo; 4]>,
    // The set of instructions corresponding to the reload of a spilled value
    pub reloads: SmallVec<[ReloadInfo; 4]>,
    // Index spills by (placement, spilled value) for fast global deduplication.
    spill_ids: FxHashMap<(Placement, ValueRef), Spill>,
    // Index reloads by (placement, spilled value) for fast global deduplication.
    reload_ids: FxHashMap<(Placement, ValueRef), Reload>,
    // The set of operands in registers on entry to a given program point
    w_entries: FxHashMap<ProgramPoint, SmallSet<ValueOrAlias, 4>>,
    // The set of operands that have been spilled upon entry to a given program point
    s_entries: FxHashMap<ProgramPoint, SmallSet<ValueOrAlias, 4>>,
    // The set of operands in registers on exit from a given program point
    w_exits: FxHashMap<ProgramPoint, SmallSet<ValueOrAlias, 4>>,
    // The set of operands that have been spilled so far, on exit from a given program point
    s_exits: FxHashMap<ProgramPoint, SmallSet<ValueOrAlias, 4>>,
}

/// Represents a single predecessor for some [ProgramPoint]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Predecessor {
    /// The predecessor of the point, is one of the following:
    ///
    /// 1. For a point at the start of a block, the predecessor is the operation itself on entry
    /// 2. For a point after an op, the predecessor is the entry of that op, i.e. control bypassed
    ///    all of the op's nested regions and skipped straight to after the op.
    Parent,
    /// The predecessor of the point is cross-region control flow
    Region(OperationRef),
    /// The predecessor of the point is unstructured control flow
    Block { op: OperationRef, index: u8 },
}

impl Predecessor {
    pub fn operation(&self, point: ProgramPoint) -> OperationRef {
        match self {
            Self::Parent => match point {
                ProgramPoint::Block { block, .. } => block.grandparent().unwrap(),
                ProgramPoint::Op { op, .. } => op,
                _ => unreachable!(),
            },
            Self::Region(op) | Self::Block { op, .. } => *op,
        }
    }

    pub fn block(&self) -> Option<BlockRef> {
        match self {
            Self::Parent => None,
            Self::Region(op) | Self::Block { op, .. } => op.parent(),
        }
    }

    pub fn arguments(&self, point: ProgramPoint) -> ValueRange<'static, 4> {
        match self {
            Self::Parent => match point {
                ProgramPoint::Block { block, .. } => {
                    // We need to get the entry successor operands from the parent branch op to
                    // `block`
                    let op = block.grandparent().unwrap();
                    let op = op.borrow();
                    let branch = op.as_trait::<dyn RegionBranchOpInterface>().unwrap();
                    let args = branch.get_entry_successor_operands(RegionBranchPoint::Child(
                        block.parent().unwrap(),
                    ));
                    ValueRange::<4>::from(args).into_owned()
                }
                ProgramPoint::Op { op, .. } => {
                    // There cannot be any successor arguments in this case, and the op itself
                    // cannot have any results
                    assert_eq!(op.borrow().num_results(), 0);
                    ValueRange::Empty
                }
                _ => unreachable!(),
            },
            Self::Region(op) => {
                let op = op.borrow();
                let terminator = op.as_trait::<dyn RegionBranchTerminatorOpInterface>().unwrap();
                let branch_point = match point {
                    ProgramPoint::Block { block, .. } => {
                        // Transfer of control to another region of the parent op
                        RegionBranchPoint::Child(block.parent().unwrap())
                    }
                    ProgramPoint::Op { .. } => {
                        // Returning from the predecessor region back to the parent op's exit
                        RegionBranchPoint::Parent
                    }
                    _ => unreachable!(),
                };
                let args = terminator.get_successor_operands(branch_point);
                ValueRange::<4>::from(args).into_owned()
            }
            Self::Block { op, index } => {
                ValueRange::<4>::from(op.borrow().successor(*index as usize).arguments).into_owned()
            }
        }
    }
}

/// The state of the W and S sets on entry to a given block
#[derive(Debug)]
struct ProgramPointInfo {
    point: ProgramPoint,
    w_entry: SmallSet<ValueOrAlias, 4>,
    s_entry: SmallSet<ValueOrAlias, 4>,
    live_predecessors: SmallVec<[Predecessor; 2]>,
}

/// Uniquely identifies a computed split control flow edge in a [SpillAnalysis]
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Split(u32);
impl Split {
    pub fn new(id: usize) -> Self {
        Self(id.try_into().expect("invalid index"))
    }

    #[inline(always)]
    pub const fn as_usize(&self) -> usize {
        self.0 as usize
    }
}

/// Metadata about a control flow edge which needs to be split in order to accommodate spills and/or
/// reloads along that edge.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct SplitInfo {
    pub id: Split,
    /// The destination program point for the control flow edge being split
    pub point: ProgramPoint,
    /// The predecessor, or origin, of the control flow edge being split
    pub predecessor: Predecessor,
    /// The block representing the split, if materialized
    pub split: Option<BlockRef>,
}

/// Uniquely identifies a computed spill in a [SpillAnalysis]
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Spill(u32);
impl Spill {
    pub fn new(id: usize) -> Self {
        Self(id.try_into().expect("invalid index"))
    }

    #[inline(always)]
    pub const fn as_usize(&self) -> usize {
        self.0 as usize
    }
}

/// Metadata about a computed spill
#[derive(Debug, Clone)]
pub struct SpillInfo {
    pub id: Spill,
    /// The point in the program where this spill should be placed
    pub place: Placement,
    /// The value to be spilled
    pub value: ValueRef,
    /// The span associated with the source code that triggered the spill
    pub span: SourceSpan,
    /// The spill instruction, if materialized
    pub inst: Option<OperationRef>,
}

impl SpillInfo {
    pub fn stack_size(&self) -> usize {
        self.value.borrow().ty().size_in_felts()
    }
}

/// Uniquely identifies a computed reload in a [SpillAnalysis]
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Reload(u32);
impl Reload {
    pub fn new(id: usize) -> Self {
        Self(id.try_into().expect("invalid index"))
    }

    #[inline(always)]
    pub const fn as_usize(&self) -> usize {
        self.0 as usize
    }
}

/// Metadata about a computed reload
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct ReloadInfo {
    pub id: Reload,
    /// The point in the program where this spill should be placed
    pub place: Placement,
    /// The spilled value to be reloaded
    pub value: ValueRef,
    /// The span associated with the source code that triggered the spill
    pub span: SourceSpan,
    /// The reload instruction, if materialized
    pub inst: Option<OperationRef>,
}

/// This enumeration represents a program location where a spill or reload operation should be
/// placed.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum Placement {
    /// A concrete location in the current program.
    ///
    /// The operation will be placed according to the semantics of the given [InsertionPoint]
    At(ProgramPoint),
    /// A pseudo-location, corresponding to the end of the block that will be materialized
    /// to split the control flow edge represented by [Split].
    Split(Split),
}

/// The maximum number of operand stack slots which can be assigned without spills.
const K: usize = 16;

impl Analysis for SpillAnalysis {
    type Target = Function;

    fn name(&self) -> &'static str {
        "spills"
    }

    fn analyze(
        &mut self,
        op: &Self::Target,
        analysis_manager: AnalysisManager,
    ) -> Result<(), Report> {
        log::debug!(target: "spills", "running spills analysis for {}", op.as_operation());

        let liveness = analysis_manager.get_analysis::<LivenessAnalysis>()?;
        self.compute(op, &liveness, analysis_manager)
    }

    fn invalidate(&self, preserved_analyses: &mut PreservedAnalyses) -> bool {
        !preserved_analyses.is_preserved::<LivenessAnalysis>()
    }
}

/// Queries
impl SpillAnalysis {
    /// Returns true if at least one value must be spilled
    pub fn has_spills(&self) -> bool {
        !self.spills.is_empty()
    }

    /// Returns the set of control flow edges that must be split to accommodate spills/reloads
    pub fn splits(&self) -> &[SplitInfo] {
        self.splits.as_slice()
    }

    /// Same as [SpillAnalysis::splits], but as a mutable reference
    pub fn splits_mut(&mut self) -> &mut [SplitInfo] {
        self.splits.as_mut_slice()
    }

    pub fn get_split(&self, split: Split) -> &SplitInfo {
        &self.splits[split.as_usize()]
    }

    /// Returns the set of values which require spills
    pub fn spilled(&self) -> &FxHashSet<ValueRef> {
        &self.spilled
    }

    /// Returns true if `value` is spilled at some point
    pub fn is_spilled(&self, value: &ValueRef) -> bool {
        self.spilled.contains(value)
    }

    /// Returns true if `value` is spilled at the given program point (i.e. inserted before)
    pub fn is_spilled_at(&self, value: ValueRef, pp: ProgramPoint) -> bool {
        let place = match pp {
            ProgramPoint::Block { block, .. } => {
                match self.splits.iter().find(|split| split.split == Some(block)) {
                    // Treat a query using the materialized split block as a query on the
                    // corresponding edge split placement.
                    Some(split) => Placement::Split(split.id),
                    // Preserve the original program point (including before/after) for non-split blocks.
                    None => Placement::At(pp),
                }
            }
            point => Placement::At(point),
        };
        self.spill_ids.contains_key(&(place, value))
    }

    /// Returns true if `value` will be spilled in the given split
    pub fn is_spilled_in_split(&self, value: ValueRef, split: Split) -> bool {
        self.spills.iter().any(|info| {
            info.value == value && matches!(info.place, Placement::Split(s) if s == split)
        })
    }

    /// Returns the set of computed spills
    pub fn spills(&self) -> &[SpillInfo] {
        self.spills.as_slice()
    }

    /// Returns true if `value` is reloaded at some point
    pub fn is_reloaded(&self, value: &ValueRef) -> bool {
        self.reloads.iter().any(|info| &info.value == value)
    }

    /// Returns true if `value` is reloaded at the given program point (i.e. inserted before)
    pub fn is_reloaded_at(&self, value: ValueRef, pp: ProgramPoint) -> bool {
        let place = match pp {
            ProgramPoint::Block { block, .. } => {
                match self.splits.iter().find(|split| split.split == Some(block)) {
                    // Treat a query using the materialized split block as a query on the
                    // corresponding edge split placement.
                    Some(split) => Placement::Split(split.id),
                    // Preserve the original program point (including before/after) for non-split blocks.
                    None => Placement::At(pp),
                }
            }
            point => Placement::At(point),
        };
        self.reload_ids.contains_key(&(place, value))
    }

    /// Returns true if `value` will be reloaded in the given split
    pub fn is_reloaded_in_split(&self, value: ValueRef, split: Split) -> bool {
        self.reloads.iter().any(|info| {
            info.value == value && matches!(info.place, Placement::Split(s) if s == split)
        })
    }

    /// Returns the set of computed reloads
    pub fn reloads(&self) -> &[ReloadInfo] {
        self.reloads.as_slice()
    }

    /// Returns the operands in W upon entry to `point`
    pub fn w_entry(&self, point: &ProgramPoint) -> &[ValueOrAlias] {
        self.w_entries[point].as_slice()
    }

    /// Returns the operands in S upon entry to `point`
    pub fn s_entry(&self, point: &ProgramPoint) -> &[ValueOrAlias] {
        self.s_entries[point].as_slice()
    }

    /// Returns the operands in W upon exit from `point`
    pub fn w_exit(&self, point: &ProgramPoint) -> &[ValueOrAlias] {
        self.w_exits[point].as_slice()
    }

    /// Returns the operands in S upon exit from `point`
    pub fn s_exit(&self, point: &ProgramPoint) -> &[ValueOrAlias] {
        self.s_exits[point].as_slice()
    }
}

/// Analysis
impl SpillAnalysis {
    fn compute(
        &mut self,
        function: &Function,
        liveness: &LivenessAnalysis,
        analysis_manager: AnalysisManager,
    ) -> Result<(), Report> {
        if function.body().has_one_block() {
            let mut _deferred = Vec::<(BlockRef, SmallVec<[BlockRef; 2]>)>::default();
            self.visit_single_block(
                function.as_operation(),
                &function.entry_block().borrow(),
                None,
                liveness,
                analysis_manager,
                &mut _deferred,
            )?;
            assert!(_deferred.is_empty());
            Ok(())
        } else {
            // We generally expect that control flow lifting will have removed all but the entry
            // block, but in some cases there can be some remaining unstructured control flow, so
            // we handle that in the usual way here
            let dominfo = analysis_manager.get_analysis::<DominanceInfo>()?;
            let loops = analysis_manager.get_analysis::<LoopInfo>()?;
            let entry_region = function.body().as_region_ref();
            let domtree = dominfo.dominance(entry_region);
            if let Some(loop_forest) = loops.get(&entry_region) {
                self.visit_cfg(
                    function.as_operation(),
                    &domtree,
                    loop_forest,
                    liveness,
                    analysis_manager,
                )
            } else {
                let loop_forest = LoopForest::new(&domtree);
                self.visit_cfg(
                    function.as_operation(),
                    &domtree,
                    &loop_forest,
                    liveness,
                    analysis_manager,
                )
            }
        }
    }

    fn visit_cfg(
        &mut self,
        op: &Operation,
        domtree: &DominanceTree,
        loops: &LoopForest,
        liveness: &LivenessAnalysis,
        analysis_manager: AnalysisManager,
    ) -> Result<(), Report> {
        log::trace!(target: "spills", "visiting cfg");

        // Visit the blocks of the CFG in reverse postorder (top-down)
        let mut block_q = VecDeque::from(domtree.reverse_postorder());

        // If a block has a predecessor which it dominates (i.e. control flow always flows through
        // the block in question before the given predecessor), then we must defer computing spills
        // and reloads for that edge until we have visited the predecessor. This map is used to
        // track deferred edges for each block.
        let mut deferred = Vec::<(BlockRef, SmallVec<[BlockRef; 2]>)>::default();

        while let Some(node) = block_q.pop_front() {
            let Some(block_ref) = node.block() else {
                continue;
            };

            self.visit_single_block(
                op,
                &block_ref.borrow(),
                Some(loops),
                liveness,
                analysis_manager.clone(),
                &mut deferred,
            )?;
        }

        // We've visited all blocks at least once, now we need to go back and insert
        // spills/reloads along loopback edges, as we skipped those on the first pass
        for (block_ref, preds) in deferred {
            let block = block_ref.borrow();

            // W^entry(B)
            let w_entry = self.w_entries[&ProgramPoint::at_start_of(block_ref)].clone();

            // Derive S^entry(B) and construct information about the program point at block start
            let block_info = self.block_entry_info(op, &block, liveness, w_entry);

            // For each predecessor P of B, insert spills/reloads along the inbound control flow
            // edge as follows:
            //
            // * All variables in W^entry(B) \ W^exit(P) need to be reloaded
            // * All variables in (S^entry(B) \ S^exit(P)) ∩ W^exit(P) need to be spilled
            //
            // If a given predecessor has not been processed yet, skip P, and revisit the edge later
            // after we have processed P.
            let mut _defer = SmallVec::default();
            for pred in block_info.live_predecessors.iter() {
                let predecessor = pred.block().unwrap();

                // Only visit predecessors that were deferred
                if !preds.contains(&predecessor) {
                    continue;
                }

                self.compute_control_flow_edge_spills_and_reloads(
                    &block_info,
                    pred,
                    &mut _defer,
                    liveness,
                );
            }
        }

        Ok(())
    }

    fn visit_region_cfg(
        &mut self,
        op: &dyn RegionBranchOpInterface,
        entry: &Block,
        liveness: &LivenessAnalysis,
        analysis_manager: AnalysisManager,
    ) -> Result<(), Report> {
        log::trace!(target: "spills", "visiting region cfg");

        // Visit the blocks of the CFG in reverse postorder (top-down)
        let region = entry.parent().unwrap();
        let mut region_q = Region::postorder_region_graph(&region.borrow());

        // If a region has a predecessor which it dominates (i.e. control flow always flows through
        // the region in question before the given predecessor), then we must defer computing spills
        // and reloads for that edge until we have visited the predecessor. This map is used to
        // track deferred edges for each block.
        let mut deferred = Vec::<(BlockRef, SmallVec<[BlockRef; 2]>)>::default();

        let operation = op.as_operation();
        while let Some(region) = region_q.pop() {
            let region = region.borrow();
            let block = region.entry();

            self.visit_single_block(
                operation,
                &block,
                None,
                liveness,
                analysis_manager.clone(),
                &mut deferred,
            )?;
        }

        // We've visited all blocks at least once, now we need to go back and insert
        // spills/reloads along loopback edges, as we skipped those on the first pass
        for (block_ref, preds) in deferred {
            let block = block_ref.borrow();

            // W^entry(B)
            let w_entry = self.w_entries[&ProgramPoint::at_start_of(block_ref)].clone();

            // Derive S^entry(B) and construct information about the program point at block start
            let block_info = self.block_entry_info(operation, &block, liveness, w_entry);

            // For each predecessor P of B, insert spills/reloads along the inbound control flow
            // edge as follows:
            //
            // * All variables in W^entry(B) \ W^exit(P) need to be reloaded
            // * All variables in (S^entry(B) \ S^exit(P)) ∩ W^exit(P) need to be spilled
            //
            // If a given predecessor has not been processed yet, skip P, and revisit the edge later
            // after we have processed P.
            let mut _defer = SmallVec::default();
            for pred in block_info.live_predecessors.iter() {
                let predecessor = pred.block();

                // Only visit predecessors that were deferred
                if predecessor.is_some_and(|p| !preds.contains(&p)) {
                    continue;
                }

                self.compute_control_flow_edge_spills_and_reloads(
                    &block_info,
                    pred,
                    &mut _defer,
                    liveness,
                );
            }
        }

        Ok(())
    }

    fn visit_single_block(
        &mut self,
        op: &Operation,
        block: &Block,
        loops: Option<&LoopForest>,
        liveness: &LivenessAnalysis,
        analysis_manager: AnalysisManager,
        deferred: &mut Vec<(BlockRef, SmallVec<[BlockRef; 2]>)>,
    ) -> Result<(), Report> {
        let block_ref = block.as_block_ref();

        log::trace!(target: "spills", "visiting {block}");

        // Compute W^entry(B)
        self.compute_w_entry(op, block, loops, liveness);

        // Derive S^entry(B) from W^entry(B) and compute live predecessors at block start
        let w_entry = self.w_entries[&ProgramPoint::at_start_of(block)].clone();
        log::trace!(target: "spills", "computing block information");
        let block_info = self.block_entry_info(op, block, liveness, w_entry);
        log::trace!(target: "spills", "  W^entry({block}) = {{{}}}", DisplayValues::new(block_info.w_entry.iter()));
        log::trace!(target: "spills", "  S^entry({block}) = {{{}}}", DisplayValues::new(block_info.s_entry.iter()));

        // For each predecessor P of B, insert spills/reloads along the inbound control flow
        // edge as follows:
        //
        // * All variables in W^entry(B) \ W^exit(P) need to be reloaded
        // * All variables in (S^entry(B) \ S^exit(P)) ∩ W^exit(P) need to be spilled
        //
        // If a given predecessor has not been processed yet, skip P, and revisit the edge later
        // after we have processed P.
        //
        // NOTE: Because W^exit(P) does not contain the block parameters for any given
        // successor, as those values are renamed predecessor operands, some work must be done
        // to determine the true contents of W^exit(P) for each predecessor/successor edge, and
        // only then insert spills/reloads as described above.
        let mut deferred_preds = SmallVec::<[BlockRef; 2]>::default();
        for pred in block_info.live_predecessors.iter() {
            // As soon as we need to start inserting spills/reloads, mark the function changed
            self.compute_control_flow_edge_spills_and_reloads(
                &block_info,
                pred,
                &mut deferred_preds,
                liveness,
            );
        }
        if !deferred_preds.is_empty() {
            deferred.push((block_ref, deferred_preds));
        }

        // We have our W and S sets for the entry of B, and we have inserted all spills/reloads
        // needed on incoming control flow edges to ensure that the contents of W and S are the
        // same regardless of which predecessor we reach B from.
        //
        // Now, we essentially repeat this process for each instruction I in B, i.e. we apply
        // the MIN algorithm to B. As a result, we will also have computed the contents of W
        // and S at the exit of B, which will be needed subsequently for the successors of B
        // when we process them.
        //
        // The primary differences here, are that we:
        //
        // * Assume that if a reload is needed (not in W), that it was previously spilled (must
        //   be in S)
        // * We do not issue spills for values that have already been spilled
        // * We do not emit spill instructions for values which are dead, they are just dropped
        // * We must spill from W to make room for operands and results of I, if there is
        //   insufficient space to hold the current contents of W + whatever operands of I we
        //   need to reload + the results of I that will be placed on the operand stack. We do
        //   so by spilling values with the greatest next-use distance first, preferring to
        //   spill larger values where we have an option. We also may factor in liveness - if an
        //   operand of I is dead after I, we do not need to count that operand when computing
        //   the operand stack usage for results (thus reusing the space of the operand for one
        //   or more results).
        // * It is important to note that we must count _all_ uses of the same value towards the
        //   operand stack usage, unless the semantics of an instruction explicitly dictate that
        //   a specific operand pattern only requires a single copy on the operand stack.
        //   Currently that is not the case for any instructions, and we would prefer to be more
        //   conservative at this point anyway.
        let mut w = block_info.w_entry;
        let mut s = block_info.s_entry;
        for op in block.body() {
            if let Some(loop_like) = op.as_trait::<dyn LoopLikeOpInterface>() {
                // If we hit a loop-like region branch operation, we need to process it much
                // like how we do unstructured control flow loops. The primary difference is
                // that we do not use the dominance tree to determine the order in which the
                // loop is visited, and we must also take into account op results on exit
                // from the op, unlike how "results" are represented in an unstructured loop
                self.visit_loop_like(
                    loop_like,
                    &mut w,
                    &mut s,
                    liveness,
                    analysis_manager.nest(op.as_operation_ref()),
                )?;
            } else if let Some(branch) = op.as_trait::<dyn RegionBranchOpInterface>() {
                // If we hit a region branch operation, we need to process it much like how
                // we do unstructured control flow. The primary difference is that we do not
                // use the dominance tree to determine the order in which the regions of the
                // op are visited, and we must take into account op results on exit from the
                // op.
                self.visit_region_branch_operation(
                    branch,
                    &mut w,
                    &mut s,
                    liveness,
                    analysis_manager.nest(op.as_operation_ref()),
                )?;
            } else {
                self.min(&op, &mut w, &mut s, liveness);
            }
        }

        let end_of_block = ProgramPoint::at_end_of(block_ref);
        self.w_exits.insert(end_of_block, w);
        self.s_exits.insert(end_of_block, s);

        Ok(())
    }

    fn visit_loop_like(
        &mut self,
        loop_like: &dyn LoopLikeOpInterface,
        w: &mut SmallSet<ValueOrAlias, 4>,
        s: &mut SmallSet<ValueOrAlias, 4>,
        liveness: &LivenessAnalysis,
        analysis_manager: AnalysisManager,
    ) -> Result<(), Report> {
        // Compute W and S for entry into the entry successor regions of `branch`, according to
        // the standard MIN rules up to the point where we have computed any necessary spills and
        // reloads, _without_ considering operation results of `branch`, so long as `branch` cannot
        // ever skip all of its nested regions (i.e. `branch` is not a predecessor of its own exit)

        // Compute W and S through the region graph of `branch` from the loop header, and
        // then derive W and S for exit from `branch` using predecessors of the exit point. W and
        // S at exit from `branch` are not computed using the standard MIN approach, as exiting
        // from nested regions with results is akin to an unstructured branch with arguments, so
        // we handle it as such.
        //
        // NOTE: This differs from visit_region_branch_operation in how we select spill/reload
        // candidates, so as to avoid spilling in a loop, or reloading in a loop, when either of
        // those could be lifted or pushed down to loop exits.
        let branch = loop_like.as_operation().as_trait::<dyn RegionBranchOpInterface>().unwrap();
        self.visit_region_branch_operation(branch, w, s, liveness, analysis_manager)
    }

    fn visit_region_branch_operation(
        &mut self,
        branch: &dyn RegionBranchOpInterface,
        w: &mut SmallSet<ValueOrAlias, 4>,
        s: &mut SmallSet<ValueOrAlias, 4>,
        liveness: &LivenessAnalysis,
        analysis_manager: AnalysisManager,
    ) -> Result<(), Report> {
        log::trace!(target: "spills", "visiting region branch op '{}'", branch.as_operation());
        log::trace!(target: "spills", "  W^in = {w:?}");
        log::trace!(target: "spills", "  S^in = {s:?}");

        // PHASE 1:
        //
        // Compute W and S at entry to `branch`, i.e. before any of its control flow is evaluated -
        // purely what is needed to begin evaluating the op. This will be used to derive W and S
        // within regions of the op.
        //
        // NOTE: This does not take into account results of `op` like MIN does for other types of
        // operations, as in the case of region control flow, the "results" are akin to successor
        // block arguments, rather than needing to compete for space with operands of the op itself.
        //
        // TODO(pauls): Make sure we properly handle cases where control can pass directly from op
        // entry to exit, skipping nested regions. For now we have no such operations which also
        // produce results, which is the only case that matters.

        let op = branch.as_operation();
        let before_op = ProgramPoint::before(op);
        let place = Placement::At(before_op);
        let span = op.span();

        let args = ValueRange::<2>::from(op.operands().group(0));
        let mut to_reload = args.iter().map(ValueOrAlias::new).collect::<SmallVec<[_; 2]>>();

        // Remove the first occurrance of any operand already in W, remaining uses
        // must be considered against the stack usage calculation (but will not
        // actually be reloaded)
        for operand in w.iter() {
            if let Some(pos) = to_reload.iter().position(|o| o == operand) {
                to_reload.swap_remove(pos);
            }
        }
        log::trace!(target: "spills", "  require reloading = {to_reload:#?}");

        // Precompute the starting stack usage of W
        let w_used = w.iter().map(|o| o.stack_size()).sum::<usize>();
        log::trace!(target: "spills", "  current stack usage = {w_used}");

        // Compute the needed operand stack space for all operands not currently in W, i.e. those
        // which must be reloaded from a spill slot
        let in_needed = to_reload.iter().map(|o| o.stack_size()).sum::<usize>();
        log::trace!(target: "spills", "  required by reloads = {in_needed}");

        // If we have room for operands and results in W, then no spills are needed,
        // otherwise we require two passes to compute the spills we will need to issue
        let mut to_spill = SmallSet::<_, 4>::default();

        // First pass: compute spills for entry to I (making room for operands)
        //
        // The max usage in is determined by the size of values currently in W, plus the size
        // of any duplicate operands (i.e. values used as operands more than once), as well as
        // the size of any operands which must be reloaded.
        let max_usage_in = w_used + in_needed;
        if max_usage_in > K {
            log::trace!(target: "spills", "  max usage on entry ({max_usage_in}) exceeds K ({K}), spills required");
            // We must spill enough capacity to keep K >= 16
            let mut must_spill = max_usage_in - K;
            // Our initial set of candidates consists of values in W which are not operands
            // of the current instruction.
            let mut candidates =
                w.iter().filter(|o| !args.contains(*o)).copied().collect::<SmallVec<[_; 4]>>();
            // We order the candidates such that those whose next-use distance is greatest, are
            // placed last, and thus will be selected first. We further break ties between
            // values with equal next-use distances by ordering them by the
            // effective size on the operand stack, so that larger values are
            // spilled first.
            candidates.sort_by(|a, b| {
                let a_dist = liveness.next_use_after(a, op);
                let b_dist = liveness.next_use_after(b, op);
                a_dist.cmp(&b_dist).then(a.stack_size().cmp(&b.stack_size()))
            });
            // Spill until we have made enough room
            while must_spill > 0 {
                let candidate = candidates.pop().unwrap_or_else(|| {
                    panic!(
                        "unable to spill sufficient capacity to hold all operands on stack at one \
                         time at {op}"
                    )
                });
                must_spill = must_spill.saturating_sub(candidate.stack_size());
                to_spill.insert(candidate);
            }
        } else {
            log::trace!(target: "spills", "  spills required on entry: no");
        }

        log::trace!(target: "spills", "  spills = {to_spill:?}");

        // Emit spills first, to make space for reloaded values on the operand stack
        for spill in to_spill.iter() {
            if s.insert(*spill) {
                self.spill(place, spill.value(), span);
            }

            // Remove spilled values from W
            w.remove(spill);
        }

        // Emit reloads for those operands of I not yet in W
        for reload in to_reload {
            // We only need to emit a reload for a given value once
            if w.insert(reload) {
                // By definition, if we are emitting a reload, the value must have been spilled
                s.insert(reload);
                self.reload(place, reload.value(), span);
            }
        }

        // At this point, we have our W^entry and S^entry for `branch`
        self.w_entries.insert(before_op, w.clone());
        self.s_entries.insert(before_op, s.clone());

        log::trace!(target: "spills", "  W^entry = {w:?}");
        log::trace!(target: "spills", "  S^entry = {s:?}");

        // PHASE 2:
        //
        // For each entry successor region, we propagate W and S from `branch` entry to the start
        // of each successor region's entry block, updating their contents to reflect the renaming
        // of successor arguments now that we're in a new block.
        //
        // From each entry region, we then visit the region control graph in reverse post-order,
        // just like we do unstructured CFGs, propgating W and S along the way. Once all regions
        // have been visited, we can move on to the final phase.

        // Compute the constant values for operands of `branch`, in case it allows us to elide
        // a subset of successor regions.
        let mut operands = SmallVec::<[Option<Box<dyn AttributeValue>>; 4]>::with_capacity(
            branch.operands().group(0).len(),
        );
        for operand in branch.operands().group(0).iter() {
            let value = operand.borrow().as_value_ref();
            let constant_prop_lattice = liveness.solver().get::<Lattice<ConstantValue>, _>(&value);
            if let Some(lattice) = constant_prop_lattice {
                if lattice.value().is_uninitialized() {
                    operands.push(None);
                    continue;
                }
                operands.push(lattice.value().constant_value());
            }
        }

        for successor in branch.get_entry_successor_regions(&operands) {
            //let mut w_entry = w.clone();
            //let mut s_entry = s.clone();

            // Fixup W and S based on successor operands
            let branch_point = *successor.branch_point();
            let inputs = branch.get_entry_successor_operands(branch_point);
            assert_eq!(
                inputs.num_produced(),
                0,
                "we don't currently support internally-produced successor operands"
            );
            match successor.into_successor() {
                Some(region) => {
                    let region = region.borrow();
                    let block = region.entry();
                    log::trace!(target: "spills", "  processing successor {block}");

                    // Visit the contents of `region`
                    //
                    // After this, W and S will have been set/propagated from `block` entry through
                    // all of its operations, to `block` exit.
                    //
                    // TODO(pauls): For now we assume that all regions are single-block
                    assert!(
                        region.has_one_block(),
                        "support for multi-block regions in this pass has not been implemented"
                    );
                    self.visit_region_cfg(branch, &block, liveness, analysis_manager.clone())?;
                }
                None => {
                    // TODO(pauls): Need to compute W and S on exit from `branch` as if the exit
                    // point of `branch` is the entry of a new block, i.e. as computed via
                    // `compute_w_entry_normal`
                    log::trace!(target: "spills", "  processing self as successor");
                    todo!()
                }
            }
        }

        // PHASE 3:
        //
        // We must compute W and S on exit from `branch` by obtaining the W^exit and S^exit sets
        // computed in Phase 2, and handle it much like we do join points in a unstructured CFG.
        //
        // In this case, the results of `branch` correspond to the arguments yielded from each
        // predecessor. So to determine whether any spills/reloads are needed, we must first
        // rename the yielded values to the result values, duplicating any of the arguments if the
        // referenced value is still live after `branch`. Then, for any values remaining in W that
        // are live after `branch`, but not live on exit from all predecessors, we must issue a
        // reload for that value. Correspondingly, if there are any values in S which are live after
        // `branch`, but not spilled in every predecessor, we must issue a spill for that value.
        log::trace!(target: "spills", "  computing W^exit for '{}'..", branch.as_operation().name());

        // First, compute W^exit(branch)
        self.compute_w_exit_region_branch_op(branch, liveness);

        // Then, derive S^exit(branch)
        let ProgramPointInfo {
            w_entry: w_exit,
            s_entry: s_exit,
            ..
        } = self.op_exit_info(
            branch,
            liveness,
            &self.w_exits[&ProgramPoint::after(branch.as_operation())],
        );

        *w = w_exit;
        *s = s_exit;

        log::trace!(target: "spills", "  W^exit = {w:?}");
        log::trace!(target: "spills", "  S^exit = {s:?}");

        Ok(())
    }

    pub fn set_materialized_split(&mut self, split: Split, block: BlockRef) {
        self.splits[split.as_usize()].split = Some(block);
    }

    pub fn set_materialized_spill(&mut self, spill: Spill, op: OperationRef) {
        self.spills[spill.as_usize()].inst = Some(op);
    }

    pub fn set_materialized_reload(&mut self, reload: Reload, op: OperationRef) {
        self.reloads[reload.as_usize()].inst = Some(op);
    }

    fn spill(&mut self, place: Placement, value: ValueRef, span: SourceSpan) -> Spill {
        // Spills are computed by multiple routines (MIN, edge reconciliation, over-K entry/results
        // handling). Deduplicate globally to avoid materializing the same spill multiple times when
        // distinct control-flow edges are forced to share a single insertion point.
        let key = (place, value);
        if let Some(existing) = self.spill_ids.get(&key).copied() {
            self.spilled.insert(value);
            return existing;
        }
        let id = Spill::new(self.spills.len());
        self.spilled.insert(value);
        self.spills.push(SpillInfo {
            id,
            place,
            value,
            span,
            inst: None,
        });
        self.spill_ids.insert(key, id);
        id
    }

    fn reload(&mut self, place: Placement, value: ValueRef, span: SourceSpan) -> Reload {
        // See `spill` for details on why reloads are globally deduplicated.
        let key = (place, value);
        if let Some(existing) = self.reload_ids.get(&key).copied() {
            return existing;
        }
        let id = Reload::new(self.reloads.len());
        self.reloads.push(ReloadInfo {
            id,
            place,
            value,
            span,
            inst: None,
        });
        self.reload_ids.insert(key, id);
        id
    }

    fn split(&mut self, point: ProgramPoint, predecessor: Predecessor) -> Split {
        let id = Split::new(self.splits.len());
        self.splits.push(SplitInfo {
            id,
            point,
            predecessor,
            split: None,
        });
        id
    }

    fn compute_w_entry(
        &mut self,
        op: &Operation,
        block: &Block,
        loops: Option<&LoopForest>,
        liveness: &LivenessAnalysis,
    ) {
        let block_ref = block.as_block_ref();
        if let Some(loop_info) =
            loops.and_then(|loops| loops.loop_for(block_ref).filter(|l| l.header() == block_ref))
        {
            log::trace!(target: "spills", "computing W^entry for loop header {block}");
            return self.compute_w_entry_loop(block, &loop_info, liveness);
        } else if let Some(loop_like) = op.as_trait::<dyn LoopLikeOpInterface>() {
            let region = block.parent().unwrap();
            if loop_like.get_loop_header_region() == region && block.is_entry_block() {
                log::trace!(target: "spills", "computing W^entry for loop-like header {block}");
                return self.compute_w_entry_loop_like(loop_like, block, liveness);
            }
        }

        log::trace!(target: "spills", "computing W^entry normally for {block}");
        self.compute_w_entry_normal(op, block, liveness);
    }

    fn compute_w_entry_normal(
        &mut self,
        op: &Operation,
        block: &Block,
        liveness: &LivenessAnalysis,
    ) {
        let mut freq = SmallOrdMap::<ValueOrAlias, u8, 4>::default();
        let mut take = SmallSet::<ValueOrAlias, 4>::default();
        let mut cand = SmallSet::<ValueOrAlias, 4>::default();

        // Block arguments are always in w_entry by definition
        //
        // However, it is possible for a block to have more than K stack slots' worth of arguments.
        // When this occurs, we proactively spill as many of the highest-indexed block arguments as
        // needed to ensure W^entry fits within K, inserting those spills at the start of the block.
        let start_of_block = ProgramPoint::at_start_of(block);
        let mut block_args = SmallVec::<[ValueOrAlias; 4]>::default();
        for arg in block.arguments().iter().copied() {
            let arg = ValueOrAlias::new(arg as ValueRef);
            take.insert(arg);
            block_args.push(arg);
        }
        let mut w_entry_usage = take.iter().map(|o| o.stack_size()).sum::<usize>();
        if w_entry_usage > K {
            let place = Placement::At(start_of_block);
            while w_entry_usage > K {
                let arg = block_args
                    .pop()
                    .expect("expected at least one block argument when spilling entry args");
                take.remove(&arg);
                w_entry_usage = w_entry_usage
                    .checked_sub(arg.stack_size())
                    .expect("w_entry_usage underflow when spilling entry args");
                self.spill(place, arg.value(), arg.value().borrow().span());
            }
        }

        // If this is the entry block to an IsolatedFromAbove region, the operands in w_entry are
        // guaranteed to be equal to the set of region arguments, so we're done.
        if block.is_entry_block() && op.implements::<dyn IsolatedFromAbove>() {
            self.w_entries.insert(ProgramPoint::at_start_of(block), take);
            return;
        }

        // If this block is the entry block of a RegionBranchOpInterface op, then we compute the
        // set of predecessors differently than unstructured CFG ops.
        let mut predecessor_count = 0;
        if op.implements::<dyn RegionBranchOpInterface>() && block.is_entry_block() {
            let predecessors = liveness
                .solver()
                .get::<PredecessorState, _>(&ProgramPoint::at_start_of(block))
                .expect("expected all predecessors of region block to be known");
            assert!(
                predecessors.all_predecessors_known(),
                "unexpected unresolved region successors"
            );
            let operation = op.as_operation_ref();
            for predecessor in predecessors.known_predecessors().iter().copied() {
                if predecessor == operation {
                    predecessor_count += 1;
                    let end_of_pred = ProgramPoint::before(operation);
                    for o in self.w_entries[&end_of_pred].iter().copied() {
                        // Do not add candidates which are not live-after the predecessor
                        if liveness.is_live_after_entry(o, op) {
                            *freq.entry(o).or_insert(0) += 1;
                            cand.insert(o);
                        }
                    }
                    continue;
                }

                let predecessor_block = predecessor.parent().unwrap();
                if !liveness.is_block_executable(predecessor_block) {
                    continue;
                }

                predecessor_count += 1;
                let end_of_pred = ProgramPoint::at_end_of(predecessor_block);
                for o in self.w_exits[&end_of_pred].iter().copied() {
                    // Do not add candidates which are not live-after the predecessor
                    if liveness.is_live_at_end(o, predecessor_block) {
                        *freq.entry(o).or_insert(0) += 1;
                        cand.insert(o);
                    }
                }
            }
        } else {
            for pred in block.predecessors() {
                let predecessor = pred.predecessor();

                // Skip control edges that aren't executable.
                let edge = CfgEdge::new(predecessor, pred.successor(), block.span());
                if !liveness.solver().get::<Executable, _>(&edge).is_none_or(|exe| exe.is_live()) {
                    continue;
                }

                predecessor_count += 1;
                let end_of_pred = ProgramPoint::at_end_of(predecessor);
                for o in self.w_exits[&end_of_pred].iter().copied() {
                    // Do not add candidates which are not live-after the predecessor
                    if liveness.is_live_at_end(o, predecessor) {
                        *freq.entry(o).or_insert(0) += 1;
                        cand.insert(o);
                    }
                }
            }
        }

        for (v, count) in freq.iter() {
            if *count as usize == predecessor_count {
                cand.remove(v);
                take.insert(*v);
            }
        }

        // If there are paths to B containing > K values on the operand stack, this must be due to the
        // successor arguments that are renamed on entry to B, remaining live in B, which implicitly
        // requires copying so that both the block parameter and the source value are both live in B.
        //
        // However, in order to actually fail this assertion, it would have to be the case that all
        // predecessors of this block are passing the same value as a successor argument, _and_ that the
        // value is still live in this block. This would imply that the block parameter is unnecessary
        // in the first place.
        //
        // Since that is extraordinarily unlikely to occur, and we want to catch any situations in which
        // this assertion fails, we do not attempt to handle it automatically.
        let taken = take.iter().map(|o| o.stack_size()).sum::<usize>();
        assert!(
            taken <= K,
            "implicit operand stack overflow along incoming control flow edges of {block}"
        );

        let entry = ProgramPoint::at_start_of(block);
        let entry_next_uses = liveness.next_uses_at(&entry).unwrap();

        // Prefer to select candidates with the smallest next-use distance, otherwise all else being
        // equal, choose to keep smaller values on the operand stack, and spill larger values, thus
        // freeing more space when spills are needed.
        let mut cand = cand.into_vec();
        cand.sort_by(|a, b| {
            entry_next_uses
                .distance(a)
                .cmp(&entry_next_uses.distance(b))
                .then(a.stack_size().cmp(&b.stack_size()))
        });

        let mut available = K - taken;
        let mut cand = cand.into_iter();
        while available > 0 {
            if let Some(candidate) = cand.next() {
                let size = candidate.stack_size();
                if size <= available {
                    take.insert(candidate);
                    available -= size;
                    continue;
                }
            }
            break;
        }

        self.w_entries.insert(ProgramPoint::at_start_of(block), take);
    }

    fn compute_w_exit_region_branch_op(
        &mut self,
        branch: &dyn RegionBranchOpInterface,
        liveness: &LivenessAnalysis,
    ) {
        let mut freq = SmallOrdMap::<ValueOrAlias, u8, 4>::default();
        let mut take = SmallSet::<ValueOrAlias, 4>::default();
        let mut cand = SmallSet::<ValueOrAlias, 4>::default();

        // Op results are always in W^exit by definition
        let after_branch = ProgramPoint::after(branch.as_operation());
        let mut results = SmallVec::<[ValueOrAlias; 4]>::default();
        for result in branch.results().iter().copied() {
            let result = ValueOrAlias::new(result as ValueRef);
            take.insert(result);
            results.push(result);
        }
        let mut w_exit_usage = take.iter().map(|o| o.stack_size()).sum::<usize>();
        if w_exit_usage > K {
            let place = Placement::At(after_branch);
            while w_exit_usage > K {
                let result = results
                    .pop()
                    .expect("expected at least one result when spilling branch results");
                take.remove(&result);
                w_exit_usage = w_exit_usage
                    .checked_sub(result.stack_size())
                    .expect("w_exit_usage underflow when spilling branch results");
                self.spill(place, result.value(), result.value().borrow().span());
            }
        }

        // If this block is the entry block of a RegionBranchOpInterface op, then we compute the
        // set of predecessors differently than unstructured CFG ops.
        let mut predecessor_count = 0;
        let predecessors = liveness
            .solver()
            .get::<PredecessorState, _>(&ProgramPoint::after(branch.as_operation()))
            .expect("expected all predecessors of region exit to be known");
        assert!(
            predecessors.all_predecessors_known(),
            "unexpected unresolved region predecessors"
        );
        let operation = branch.as_operation_ref();
        for predecessor in predecessors.known_predecessors().iter().copied() {
            if predecessor == operation {
                predecessor_count += 1;
                let end_of_pred = ProgramPoint::before(operation);
                log::trace!(target: "spills", "examining exit predecessor {end_of_pred}");
                for o in self.w_entries[&end_of_pred].iter().copied() {
                    // Do not add candidates which are not live-after the predecessor
                    if liveness.is_live_after_entry(o, branch.as_operation()) {
                        *freq.entry(o).or_insert(0) += 1;
                        cand.insert(o);
                    }
                }
                continue;
            }

            let predecessor_block = predecessor.parent().unwrap();
            if !liveness.is_block_executable(predecessor_block) {
                continue;
            }

            predecessor_count += 1;
            let end_of_pred = ProgramPoint::at_end_of(predecessor_block);
            log::trace!(target: "spills", "examining exit predecessor {end_of_pred}");
            for o in self.w_exits[&end_of_pred].iter().copied() {
                // Do not add candidates which are not live-after the predecessor
                if liveness.is_live_at_end(o, predecessor_block) {
                    *freq.entry(o).or_insert(0) += 1;
                    cand.insert(o);
                }
            }
        }

        for (v, count) in freq.iter() {
            if *count as usize == predecessor_count {
                cand.remove(v);
                take.insert(*v);
            }
        }

        let taken = take.iter().map(|o| o.stack_size()).sum::<usize>();
        assert!(
            taken <= K,
            "implicit operand stack overflow along incoming control flow edges of {}",
            ProgramPoint::after(operation)
        );

        let entry = ProgramPoint::after(operation);
        let entry_next_uses = liveness.next_uses_at(&entry).unwrap();

        // Prefer to select candidates with the smallest next-use distance, otherwise all else being
        // equal, choose to keep smaller values on the operand stack, and spill larger values, thus
        // freeing more space when spills are needed.
        let mut cand = cand.into_vec();
        cand.sort_by(|a, b| {
            entry_next_uses
                .distance(a)
                .cmp(&entry_next_uses.distance(b))
                .then(a.stack_size().cmp(&b.stack_size()))
        });

        let mut available = K - taken;
        let mut cand = cand.into_iter();
        while available > 0 {
            if let Some(candidate) = cand.next() {
                let size = candidate.stack_size();
                if size <= available {
                    take.insert(candidate);
                    available -= size;
                    continue;
                }
            }
            break;
        }

        self.w_exits.insert(entry, take);
    }

    fn compute_w_entry_loop(
        &mut self,
        block: &Block,
        loop_info: &Loop,
        liveness: &LivenessAnalysis,
    ) {
        let entry = ProgramPoint::at_start_of(block);

        let params = block
            .arguments()
            .iter()
            .copied()
            .map(|v| v as ValueRef)
            .collect::<SmallVec<[_; 4]>>();
        let mut alive = params.iter().copied().map(ValueOrAlias::new).collect::<SmallSet<_, 4>>();

        let next_uses = liveness.next_uses_at(&entry).expect("missing liveness for block entry");
        alive.extend(next_uses.iter().filter_map(|v| {
            if v.is_live() {
                Some(ValueOrAlias::new(v.value))
            } else {
                None
            }
        }));

        log::trace!(target: "spills", "  alive at loop entry: {{{}}}", DisplayValues::new(alive.iter()));

        // Initial candidates are values live at block entry which are used in the loop body
        let mut cand = alive
            .iter()
            .filter(|o| next_uses.distance(*o) < LOOP_EXIT_DISTANCE)
            .copied()
            .collect::<SmallSet<_, 4>>();

        log::trace!(target: "spills", "  initial candidates: {{{}}}", DisplayValues::new(cand.iter()));

        // Values which are "live through" the loop, are those which are live at entry, but not
        // used within the body of the loop. If we have excess available operand stack capacity,
        // then we can avoid issuing spills/reloads for at least some of these values.
        let live_through = alive.difference(&cand);

        log::trace!(target: "spills", "  live through loop: {{{}}}", DisplayValues::new(live_through.iter()));

        let w_used = cand.iter().map(|o| o.stack_size()).sum::<usize>();
        let max_loop_pressure = max_loop_pressure(loop_info, liveness);
        log::trace!(target: "spills", "  w_used={w_used}, K={K}, loop_pressure={max_loop_pressure}");

        if w_used < K {
            if let Some(mut free_in_loop) = K.checked_sub(max_loop_pressure) {
                let mut live_through = live_through.into_vec();
                live_through.sort_by(|a, b| {
                    next_uses
                        .distance(a)
                        .cmp(&next_uses.distance(b))
                        .then(a.stack_size().cmp(&b.stack_size()))
                });

                let mut live_through = live_through.into_iter();
                while free_in_loop > 0 {
                    if let Some(operand) = live_through.next()
                        && let Some(new_free) = free_in_loop.checked_sub(operand.stack_size())
                    {
                        if cand.insert(operand) {
                            free_in_loop = new_free;
                        }
                        continue;
                    }
                    break;
                }
            }

            self.w_entries.insert(ProgramPoint::at_start_of(block), cand);
        } else {
            // We require the block parameters to be in W on entry
            let mut take =
                SmallSet::<_, 4>::from_iter(params.iter().copied().map(ValueOrAlias::new));

            // So remove them from the set of candidates, then sort remaining by next-use and size
            let mut cand = cand.into_vec();
            cand.retain(|o| !params.contains(&o.value()));
            cand.sort_by(|a, b| {
                next_uses
                    .distance(a)
                    .cmp(&next_uses.distance(b))
                    .then(a.stack_size().cmp(&b.stack_size()))
            });

            // Fill `take` with as many of the candidates as we can
            let mut taken = take.iter().map(|o| o.stack_size()).sum::<usize>();
            take.extend(cand.into_iter().take_while(|operand| {
                let size = operand.stack_size();
                let new_size = taken + size;
                if new_size <= K {
                    taken = new_size;
                    true
                } else {
                    false
                }
            }));
            self.w_entries.insert(ProgramPoint::at_start_of(block), take);
        }
    }

    fn compute_w_entry_loop_like(
        &mut self,
        loop_like: &dyn LoopLikeOpInterface,
        block: &Block,
        liveness: &LivenessAnalysis,
    ) {
        let entry = ProgramPoint::at_start_of(block);

        let params = ValueRange::<4>::from(block.arguments());
        let mut alive = params.iter().map(ValueOrAlias::new).collect::<SmallSet<_, 4>>();

        let next_uses = liveness.next_uses_at(&entry).expect("missing liveness for block entry");
        alive.extend(next_uses.iter().filter_map(|v| {
            if v.is_live() {
                Some(ValueOrAlias::new(v.value))
            } else {
                None
            }
        }));

        // Initial candidates are values live at block entry which are used in the loop body
        let mut cand = alive
            .iter()
            .filter(|o| next_uses.distance(*o) < LOOP_EXIT_DISTANCE)
            .copied()
            .collect::<SmallSet<_, 4>>();

        // Values which are "live through" the loop, are those which are live at entry, but not
        // used within the body of the loop. If we have excess available operand stack capacity,
        // then we can avoid issuing spills/reloads for at least some of these values.
        let live_through = alive.difference(&cand);

        let w_used = cand.iter().map(|o| o.stack_size()).sum::<usize>();
        if w_used < K {
            if let Some(mut free_in_loop) =
                K.checked_sub(max_loop_pressure_loop_like(loop_like, liveness))
            {
                let mut live_through = live_through.into_vec();
                live_through.sort_by(|a, b| {
                    next_uses
                        .distance(a)
                        .cmp(&next_uses.distance(b))
                        .then(a.stack_size().cmp(&b.stack_size()))
                });

                let mut live_through = live_through.into_iter();
                while free_in_loop > 0 {
                    if let Some(operand) = live_through.next()
                        && let Some(new_free) = free_in_loop.checked_sub(operand.stack_size())
                    {
                        if cand.insert(operand) {
                            free_in_loop = new_free;
                        }
                        continue;
                    }
                    break;
                }
            }

            self.w_entries.insert(ProgramPoint::at_start_of(block), cand);
        } else {
            // We require the block parameters to be in W on entry
            let mut take = SmallSet::<_, 4>::from_iter(params.iter().map(ValueOrAlias::new));

            // So remove them from the set of candidates, then sort remaining by next-use and size
            let mut cand = cand.into_vec();
            cand.retain(|o| !params.contains(o));
            cand.sort_by(|a, b| {
                next_uses
                    .distance(a)
                    .cmp(&next_uses.distance(b))
                    .then(a.stack_size().cmp(&b.stack_size()))
            });

            // Fill `take` with as many of the candidates as we can
            let mut taken = take.iter().map(|o| o.stack_size()).sum::<usize>();
            take.extend(cand.into_iter().take_while(|operand| {
                let size = operand.stack_size();
                let new_size = taken + size;
                if new_size <= K {
                    taken = new_size;
                    true
                } else {
                    false
                }
            }));
            self.w_entries.insert(ProgramPoint::at_start_of(block), take);
        }
    }

    fn block_entry_info(
        &self,
        op: &Operation,
        block: &Block,
        liveness: &LivenessAnalysis,
        w_entry: SmallSet<ValueOrAlias, 4>,
    ) -> ProgramPointInfo {
        let mut info = ProgramPointInfo {
            point: ProgramPoint::at_start_of(block),
            w_entry,
            s_entry: Default::default(),
            live_predecessors: Default::default(),
        };

        // Compute S^entry(B) and live predecessors
        //
        // NOTE: If `block` is the entry block of a nested region of `op`, and `op` implements
        // RegionBranchOpInterface, then derive predecessor state using the information that we
        // know is attached to live predecessor edges of `block`
        if let Some(branch) =
            op.as_trait::<dyn RegionBranchOpInterface>().filter(|_| block.is_entry_block())
        {
            let predecessors = liveness
                .solver()
                .get::<PredecessorState, _>(&info.point)
                .expect("expected all predecessors of region block to be known");
            assert!(
                predecessors.all_predecessors_known(),
                "unexpected unresolved region successors"
            );
            let branch_op = branch.as_operation().as_operation_ref();
            for predecessor in predecessors.known_predecessors() {
                // Is `predecessor` the operation itself? Fetch the computed S attached to before
                // the branch op
                if predecessor == &branch_op {
                    info.live_predecessors.push(Predecessor::Parent);
                    if let Some(s_in) = self.s_entries.get(&ProgramPoint::before(branch_op)) {
                        info.s_entry = info.s_entry.into_union(s_in);
                    }
                    continue;
                }

                info.live_predecessors.push(Predecessor::Region(*predecessor));

                // Merge in the state from the predecessor's terminator.
                let pred_block = predecessor.parent().unwrap();
                if let Some(s_out) = self.s_exits.get(&ProgramPoint::at_end_of(pred_block)) {
                    info.s_entry = info.s_entry.into_union(s_out);
                }
            }
        } else {
            for pred in block.predecessors() {
                let predecessor = pred.predecessor();

                // Skip control edges that aren't executable.
                let edge = CfgEdge::new(predecessor, pred.successor(), block.span());
                if !liveness.solver().get::<Executable, _>(&edge).is_none_or(|exe| exe.is_live()) {
                    continue;
                }

                info.live_predecessors.push(Predecessor::Block {
                    op: pred.owner,
                    index: pred.index,
                });

                if let Some(s_exitp) = self.s_exits.get(&ProgramPoint::at_end_of(predecessor)) {
                    info.s_entry = info.s_entry.into_union(s_exitp);
                }
            }
        }

        info.s_entry = info.s_entry.into_intersection(&info.w_entry);

        info
    }

    fn op_exit_info(
        &self,
        branch: &dyn RegionBranchOpInterface,
        liveness: &LivenessAnalysis,
        w_entry: &SmallSet<ValueOrAlias, 4>,
    ) -> ProgramPointInfo {
        let mut info = ProgramPointInfo {
            point: ProgramPoint::after(branch.as_operation()),
            w_entry: w_entry.clone(),
            s_entry: Default::default(),
            live_predecessors: Default::default(),
        };

        // Compute S^entry(B) and live predecessors
        //
        // NOTE: If `block` is the entry block of a nested region of `op`, and `op` implements
        // RegionBranchOpInterface, then derive predecessor state using the information that we
        // know is attached to live predecessor edges of `block`
        let predecessors = liveness
            .solver()
            .get::<PredecessorState, _>(&info.point)
            .expect("expected all predecessors of region block to be known");
        assert!(predecessors.all_predecessors_known(), "unexpected unresolved region successors");
        let branch_op = branch.as_operation().as_operation_ref();
        for predecessor in predecessors.known_predecessors() {
            // Is `predecessor` the operation itself? Fetch the computed S attached to before
            // the branch op
            if predecessor == &branch_op {
                info.live_predecessors.push(Predecessor::Parent);
                let s_in = &self.s_entries[&ProgramPoint::before(branch_op)];
                info.s_entry = info.s_entry.into_union(s_in);
                continue;
            }

            info.live_predecessors.push(Predecessor::Region(*predecessor));

            // Merge in the state from the predecessor's terminator.
            let pred_block = predecessor.parent().unwrap();
            let s_out = &self.s_exits[&ProgramPoint::at_end_of(pred_block)];

            info.s_entry = info.s_entry.into_union(s_out);
        }

        info.s_entry = info.s_entry.into_intersection(&info.w_entry);

        info
    }
}

/// Compute the maximum operand stack depth required within the body of the given loop.
///
/// If the stack depth never reaches K, the excess capacity represents an opportunity to
/// avoid issuing spills/reloads for values which are live through the loop.
fn max_loop_pressure(loop_info: &Loop, liveness: &LivenessAnalysis) -> usize {
    let header = loop_info.header();
    let mut max = max_block_pressure(&header.borrow(), liveness);
    let mut block_q = VecDeque::from_iter([header]);
    let mut visited = SmallSet::<BlockRef, 4>::default();

    log::trace!(target: "spills", "computing max pressure for loop headed by {header}");

    while let Some(block) = block_q.pop_front() {
        if !visited.insert(block) {
            continue;
        }

        let children = BlockRef::children(block).collect::<Vec<_>>();
        log::trace!(target: "spills", "    children of {block}: {children:?}");
        let loop_children = children
            .iter()
            .filter(|b| loop_info.contains_block(**b))
            .copied()
            .collect::<Vec<_>>();
        log::trace!(target: "spills", "    children in loop: {loop_children:?}");
        block_q.extend(loop_children);

        let max_block_pressure = max_block_pressure(&block.borrow(), liveness);
        log::trace!(target: "spills", "  block {block} pressure = {max_block_pressure}");
        max = core::cmp::max(max, max_block_pressure);
    }

    log::trace!(target: "spills", "  max loop pressure = {max}");
    max
}

fn max_loop_pressure_loop_like(
    loop_like: &dyn LoopLikeOpInterface,
    liveness: &LivenessAnalysis,
) -> usize {
    let mut max_pressure = 0;
    let mut visited = SmallSet::<_, 4>::default();
    for region in loop_like.get_loop_regions() {
        if !visited.insert(region) {
            continue;
        }
        Region::traverse_region_graph(&region.borrow(), |region, _| {
            if !visited.insert(region.as_region_ref()) {
                return true;
            }
            let region_entry = region.entry();
            max_pressure =
                core::cmp::max(max_pressure, max_block_pressure(&region_entry, liveness));
            false
        });
    }
    max_pressure
}

/// Compute the maximum operand stack pressure for `block`, using `liveness`
fn max_block_pressure(block: &Block, liveness: &LivenessAnalysis) -> usize {
    let mut max_pressure = 0;

    let live_in = liveness.next_uses_at(&ProgramPoint::at_start_of(block));
    if let Some(live_in) = live_in {
        for v in live_in.live() {
            max_pressure += v.borrow().ty().size_in_felts();
        }
    }

    let mut operands = SmallVec::<[ValueRef; 8]>::default();
    for op in block.body() {
        operands.clear();
        operands.extend(op.operands().all().iter().map(|v| v.borrow().as_value_ref()));

        let mut live_in_pressure = 0;
        let mut relief = 0usize;
        let live_in = liveness.next_uses_at(&ProgramPoint::before(&*op));
        let live_out = liveness.next_uses_at(&ProgramPoint::after(&*op));
        if let Some(live_in) = live_in {
            for live in live_in.live() {
                if operands.contains(&live) {
                    continue;
                }
                if live_out
                    .as_ref()
                    .is_none_or(|live_out| live_out.get(live).is_none_or(|v| !v.is_live()))
                {
                    continue;
                }
                live_in_pressure += live.borrow().ty().size_in_felts();
            }
        }
        for operand in operands.iter() {
            let size = operand.borrow().ty().size_in_felts();
            if live_out
                .as_ref()
                .is_none_or(|live_out| live_out.get(operand).is_none_or(|v| !v.is_live()))
            {
                relief += size;
            }
            live_in_pressure += size;
        }
        let mut result_pressure = 0usize;
        for result in op.results().all() {
            result_pressure += result.borrow().ty().size_in_felts();
        }

        live_in_pressure += result_pressure.saturating_sub(relief);
        max_pressure = core::cmp::max(max_pressure, live_in_pressure);

        // Visit any nested regions and ensure that the maximum pressure in those regions is taken
        // into account
        if let Some(loop_like) = op.as_trait::<dyn LoopLikeOpInterface>() {
            max_pressure =
                core::cmp::max(max_pressure, max_loop_pressure_loop_like(loop_like, liveness));
        } else if let Some(branch_op) = op.as_trait::<dyn RegionBranchOpInterface>() {
            let mut visited = SmallSet::<_, 4>::default();
            for region in branch_op.get_successor_regions(RegionBranchPoint::Parent) {
                if let Some(region) = region.into_successor() {
                    if !visited.insert(region) {
                        continue;
                    }
                    Region::traverse_region_graph(&region.borrow(), |region, _| {
                        if !visited.insert(region.as_region_ref()) {
                            return true;
                        }
                        let region_entry = region.entry();
                        max_pressure = core::cmp::max(
                            max_pressure,
                            max_block_pressure(&region_entry, liveness),
                        );
                        false
                    });
                }
            }
        }
    }

    max_pressure
}

impl SpillAnalysis {
    /// At join points in the control flow graph, the set of live and spilled values may, and likely
    /// will, differ depending on which predecessor is taken to reach it. We must ensure that for
    /// any given predecessor:
    ///
    /// * Spills are inserted for any values expected in S upon entry to the successor block, which have
    ///   not been spilled yet. This occurs when a spill is needed in some predecessor, but not in
    ///   another, thus we must make sure the spill slot is written to at join points.
    /// * Reloads are inserted for any values expected in W upon entry to the successor block, which are
    ///   not in W yet. This occurs when a value is spilled on the path taken through a given
    ///   predecessor, and hasn't been reloaded again, thus we need to reload it now.
    ///
    /// NOTE: We are not actually mutating the function and inserting instructions here. Instead, we
    /// are computing what instructions need to be inserted, and where, as part of the analysis. A
    /// rewrite pass can then apply the analysis results to the function, if desired.
    fn compute_control_flow_edge_spills_and_reloads(
        &mut self,
        info: &ProgramPointInfo,
        pred: &Predecessor,
        deferred: &mut SmallVec<[BlockRef; 2]>,
        liveness: &LivenessAnalysis,
    ) {
        // Select the appropriate predecessor program point for the point represented by `info`,
        // and then obtain W^exit(P).
        //
        // If we don't have W^exit(P) yet, then P hasn't been processed yet. This is permitted for
        // intra-region control flow, but not inter-region control flow.
        let (w_exitp, s_exitp) = match pred.block() {
            // The predecessor is either another block in the same region, or cross-region control
            // flow from a block in a sibling region.
            Some(predecessor) => {
                let end_of_pred = ProgramPoint::at_end_of(predecessor);
                let Some(w_exitp) = self.w_exits.get(&end_of_pred) else {
                    // We expect both predecessor and successor to be in the same region if we do
                    // not yet have W^exit(P) available, in which case we defer processing of this
                    // program point.
                    /*
                    let successor = info.point.block().unwrap();
                    assert_eq!(
                        successor.parent(),
                        predecessor.parent(),
                        "expected w_exitp to be computed already for cross-region control flow"
                    );
                     */
                    deferred.push(predecessor);
                    return;
                };
                let s_exitp = &self.s_exits[&end_of_pred];
                (w_exitp, s_exitp)
            }
            // The predecessor is the operation itself, but whether the edge we're visiting is
            // entering the operation, or exiting, is determined by `info`
            None => {
                let end_of_pred = match info.point {
                    // The predecessor is `op` itself in a scenario where control has skipped all of
                    // `op`'s nested regions.
                    ProgramPoint::Op { op, .. } => ProgramPoint::before(op),
                    // The predecessor is `op` itself on entry to `block`
                    ProgramPoint::Block { block, .. } => {
                        ProgramPoint::before(block.grandparent().unwrap())
                    }
                    _ => unreachable!(),
                };
                // By definition we must have already visited before(op)
                let w_exitp = &self.w_entries[&end_of_pred];
                let s_exitp = &self.s_entries[&end_of_pred];
                (w_exitp, s_exitp)
            }
        };

        let mut to_reload = info.w_entry.difference(w_exitp);
        let mut to_spill = info.s_entry.difference(s_exitp).into_intersection(w_exitp);

        // We need to issue spills for any items in W^exit(P) / W^entry(B) that are not in S^exit(P),
        // but are live-after P.
        //
        // This can occur when B is a loop header, and the computed W^entry(B) does not include values
        // in W^exit(P) that are live-through the loop, typically because of loop pressure within the
        // loop requiring us to place spills of those values outside the loop.
        let must_spill = w_exitp.difference(&info.w_entry).into_difference(s_exitp);
        let next_uses = liveness
            .next_uses_at(&info.point)
            .expect("missing liveness info for program point");
        to_spill.extend(must_spill.into_iter().filter(|o| next_uses.is_live(o)));

        // We expect any block parameters present to be in `to_reload` at this point, as they will never
        // be in W^exit(P) (the parameters are not in scope at the end of P). The arguments provided in
        // the predecessor corresponding to the block parameters may or may not be in W^exit(P), so we
        // must determine which of those values need to be reloaded, and whether or not to spill any of
        // them so that there is sufficient room in W to hold all the block parameters. Spills may be
        // needed for two reasons:
        //
        // 1. There are multiple predecessors, and we need to spill a value to ensure it is spilled on
        //    all paths to the current block
        //
        // 2. An argument corresponding to a block parameter for this block is still live in/through
        //    this block. Due to values being renamed when used as block arguments, we must ensure there
        //    is a new copy of the argument so that the original value, and the renamed alias, are both
        //    live simultaneously. If there is insufficient operand stack space to accommodate both,
        //    then we must spill values from W to make room.
        //
        // So in short, we post-process `to_reload` by matching any values in the set which are block
        // parameters, with the corresponding source values in W^exit(P) (issuing reloads if the value
        // given as argument in the predecessor is not in W^exit(P))
        let pred_args = pred.arguments(info.point);

        match &info.point {
            // Remove block params from `to_reload`, and replace them, as needed, with reloads of the value
            // in the predecessor which was used as the successor argument
            ProgramPoint::Block { block, .. } => {
                for (i, param) in block.borrow().arguments().iter().enumerate() {
                    let param = *param as ValueRef;
                    to_reload.remove(&param);
                    // Match up this parameter with its source argument, and if the source value is not in
                    // W^exit(P), then a reload is needed
                    let src = pred_args.get(i).unwrap_or_else(|| {
                        panic!("index {i} is out of bounds: len is {}", pred_args.len())
                    });
                    if !w_exitp.contains(&src) {
                        to_reload.insert(ValueOrAlias::new(src));
                    }
                }
            }
            // Remove op results from `to_reload`, and replace them, as needed, with reloads of the
            // value in the predecessor which was used as the successor argument
            ProgramPoint::Op { op: _, .. } => {
                todo!()
            }
            _ => unreachable!(),
        }

        // If there are no reloads or spills needed, we're done
        if to_reload.is_empty() && to_spill.is_empty() {
            return;
        }

        // If spills/reloads are needed on this edge, we need a placement for those instructions.
        //
        // For unstructured control flow (i.e. an explicit branch op), we split the edge and insert
        // spills/reloads in the split block so they are executed only along that predecessor edge.
        //
        // For structured control flow edges (i.e. predecessor is `Parent` or `Region`), we insert
        // spills/reloads at the destination when it has a single live predecessor, otherwise we
        // fall back to placing them before the predecessor operation, as we do not currently
        // support splitting such edges during transformation.
        let (place, span) = match pred {
            Predecessor::Block { .. } => {
                let split = self.split(info.point, *pred);
                (Placement::Split(split), pred.operation(info.point).span())
            }
            Predecessor::Parent | Predecessor::Region(_) => {
                // If the destination has a single live predecessor, placing at the destination is
                // effectively edge-specific.
                if info.live_predecessors.len() == 1 {
                    (Placement::At(info.point), pred.operation(info.point).span())
                } else {
                    let predecessor = pred.operation(info.point);
                    // NOTE: This placement is not edge-specific. Multiple distinct structured edges
                    // may map to the same insertion point, so we rely on spill/reload deduplication
                    // to avoid materializing duplicates.
                    (Placement::At(ProgramPoint::before(predecessor)), predecessor.span())
                }
            }
        };

        // Insert spills first, to end the live ranges of as many variables as possible
        for spill in to_spill {
            self.spill(place, spill.value(), span);
        }

        // Then insert needed reloads
        for reload in to_reload {
            self.reload(place, reload.value(), span);
        }
    }

    /// The MIN algorithm is used to compute the spills and reloads to insert at each instruction in a
    /// block, so as to ensure that there is sufficient space to hold all instruction operands and
    /// results without exceeding K elements on the operand stack simultaneously, and allocating spills
    /// so as to minimize the number of live ranges needing to be split.
    ///
    /// MIN will spill values with the greatest next-use distance first, using the size of the operand
    /// as a tie-breaker for values with equidistant next uses (i.e. the larger values get spilled
    /// first, thus making more room on the operand stack).
    ///
    /// It is expected that upon entry to a given block, that the W and S sets are accurate, regardless
    /// of which predecessor edge was used to reach the block. This is handled earlier during analysis
    /// by computing the necessary spills and reloads to be inserted along each control flow edge, as
    /// required.
    fn min(
        &mut self,
        op: &Operation,
        w: &mut SmallSet<ValueOrAlias, 4>,
        s: &mut SmallSet<ValueOrAlias, 4>,
        liveness: &LivenessAnalysis,
    ) {
        let before_op = ProgramPoint::before(op);
        let place = Placement::At(before_op);
        let span = op.span();

        log::trace!(target: "spills", "scheduling spills/reloads at {before_op}");
        log::trace!(target: "spills", "  W^entry = {w:?}");
        log::trace!(target: "spills", "  S^entry = {s:?}");

        if op.implements::<dyn RegionBranchTerminatorOpInterface>() {
            log::trace!(target: "spills", "  region terminator = true");
            // Region branch terminators forward successor operands across region boundaries.
            //
            // These operands can exceed K, but since control flow transfers immediately, we do not
            // need to keep the entire set addressable for subsequent instructions in the current
            // block. Instead, we ensure that any required operands are present in W and leave edge
            // reconciliation to the normal mechanisms (e.g. spills inserted at join points).
            w.retain(|o| liveness.is_live_before(o, op));
            let to_reload = ValueRange::<4>::from(op.operands().all());
            for reload in to_reload.into_iter().map(ValueOrAlias::new) {
                if w.insert(reload) {
                    log::trace!(target: "spills", "  emitting reload for {reload}");
                    // By definition, if we are emitting a reload, the value must have been spilled
                    s.insert(reload);
                    self.reload(place, reload.value(), span);
                }
            }

            log::trace!(target: "spills", "  W^exit = {w:?}");
            log::trace!(target: "spills", "  S^exit = {s:?}");
            return;
        }

        let is_terminator =
            op.implements::<dyn Terminator>() && !op.implements::<dyn BranchOpInterface>();

        if is_terminator {
            log::trace!(target: "spills", "  terminator = true");
            // A non-branching terminator is either a return, or an unreachable.
            // In the latter case, there are no operands or results, so there is no
            // effect on W or S In the former case, the operands to the instruction are
            // the "results" from the perspective of the operand stack, so we are simply
            // ensuring that those values are in W by issuing reloads as necessary, all
            // other values are dead, so we do not actually issue any spills.
            w.retain(|o| liveness.is_live_before(o, op));
            let to_reload = ValueRange::<4>::from(op.operands().all());
            for reload in to_reload.into_iter().map(ValueOrAlias::new) {
                if w.insert(reload) {
                    log::trace!(target: "spills", "  emitting reload for {reload}");
                    self.reload(place, reload.value(), span);
                }
            }

            log::trace!(target: "spills", "  W^exit = {w:?}");
            log::trace!(target: "spills", "  S^exit = {s:?}");
            return;
        }

        // All other instructions are handled more or less identically according to the effects
        // an instruction has, as described in the documentation of the MIN algorithm.
        //
        // In the case of branch instructions, successor arguments are not considered inputs to
        // the instruction. Instead, we handle spills/reloads for each control flow edge
        // independently, as if they occur on exit from this instruction. The result is that
        // we may or may not have all successor arguments in W on exit from I, but by the time
        // each successor block is reached, all block parameters are guaranteed to be in W
        let args = ValueRange::<4>::from(op.operands().group(0));
        let mut to_reload = args.iter().map(ValueOrAlias::new).collect::<SmallVec<[_; 2]>>();

        // Remove the first occurrance of any operand already in W, remaining uses
        // must be considered against the stack usage calculation (but will not
        // actually be reloaded)
        for operand in w.iter() {
            if let Some(pos) = to_reload.iter().position(|o| o == operand) {
                to_reload.swap_remove(pos);
            }
        }

        // Precompute the starting stack usage of W
        let w_used = w.iter().map(|o| o.stack_size()).sum::<usize>();

        // Compute the needed operand stack space for all operands not currently
        // in W, i.e. those which must be reloaded from a spill slot
        let in_needed = to_reload.iter().map(|o| o.stack_size()).sum::<usize>();

        // Compute the needed operand stack space for results of I
        let results = ValueRange::<2>::from(op.results().all());
        let out_needed = results.iter().map(|v| v.borrow().ty().size_in_felts()).sum::<usize>();

        // Compute the amount of operand stack space needed for operands which are
        // not live across the instruction, i.e. which do not consume stack space
        // concurrently with the results.
        let in_consumed = args
            .iter()
            .filter_map(|v| {
                if liveness.is_live_after(v, op) {
                    None
                } else {
                    Some(v.borrow().ty().size_in_felts())
                }
            })
            .sum::<usize>();

        log::trace!(target: "spills", "  results = {results}");
        log::trace!(target: "spills", "  require copy/reload = {to_reload:?}");
        log::trace!(target: "spills", "  current stack usage = {w_used}");
        log::trace!(target: "spills", "  required by reloads = {in_needed}");
        log::trace!(target: "spills", "  required by results = {out_needed}");
        log::trace!(target: "spills", "  freed by op         = {in_consumed}");

        // If we have room for operands and results in W, then no spills are needed,
        // otherwise we require two passes to compute the spills we will need to issue
        let mut to_spill = SmallSet::<_, 4>::default();

        // First pass: compute spills for entry to I (making room for operands)
        //
        // The max usage in is determined by the size of values currently in W, plus the size
        // of any duplicate operands (i.e. values used as operands more than once), as well as
        // the size of any operands which must be reloaded.
        let max_usage_in = w_used + in_needed;
        if max_usage_in > K {
            log::trace!(target: "spills", "max usage on entry ({max_usage_in}) exceeds K ({K}), spills required");
            // We must spill enough capacity to keep K >= 16
            let mut must_spill = max_usage_in - K;
            // Our initial set of candidates consists of values in W which are not operands
            // of the current instruction.
            let mut candidates =
                w.iter().filter(|o| !args.contains(*o)).copied().collect::<SmallVec<[_; 4]>>();
            // We order the candidates such that those whose next-use distance is greatest, are
            // placed last, and thus will be selected first. We further break ties between
            // values with equal next-use distances by ordering them by the
            // effective size on the operand stack, so that larger values are
            // spilled first.
            candidates.sort_by(|a, b| {
                let a_dist = liveness.next_use_after(a, op);
                let b_dist = liveness.next_use_after(b, op);
                a_dist.cmp(&b_dist).then(a.stack_size().cmp(&b.stack_size()))
            });
            // Spill until we have made enough room
            while must_spill > 0 {
                let candidate = candidates.pop().unwrap_or_else(|| {
                    panic!(
                        "unable to spill sufficient capacity to hold all operands on stack at one \
                         time at {op}"
                    )
                });
                must_spill = must_spill.saturating_sub(candidate.stack_size());
                to_spill.insert(candidate);
            }
        } else {
            log::trace!(target: "spills", "  spills required on entry: no");
        }

        // Second pass: compute spills for exit from I (making room for results)
        let spilled = to_spill.iter().map(|o| o.stack_size()).sum::<usize>();
        log::trace!(target: "spills", "  freed by spills = {spilled}");
        // The max usage out is computed by adding the space required for all results of I, to
        // the max usage in, then subtracting the size of all operands which are consumed by I,
        // as well as the size of those values in W which we have spilled.
        let max_usage_out = (max_usage_in + out_needed).saturating_sub(in_consumed + spilled);
        if max_usage_out > K {
            log::trace!(target: "spills", "max usage on exit ({max_usage_out}) exceeds K ({K}), additional spills required");
            // We must spill enough capacity to keep K >= 16
            let mut must_spill = max_usage_out - K;
            // For this pass, the set of candidates consists of values in W which are not
            // operands of I, and which have not been spilled yet, as well as values in W
            // which are operands of I that are live-after I. The latter group may sound
            // contradictory, how can you spill something before it is used? However, what
            // is actually happening is that we spill those values before I, so that we
            // can treat those values as being "consumed" by I, such that their space in W
            // can be reused by the results of I.
            let mut candidates = w
                .iter()
                .filter(|o| {
                    if !args.contains(*o) {
                        // Not an argument, not yet spilled
                        !to_spill.contains(*o)
                    } else {
                        // A spillable argument
                        liveness.is_live_after(*o, op)
                    }
                })
                .cloned()
                .collect::<SmallVec<[_; 4]>>();
            candidates.sort_by(|a, b| {
                let a_dist = liveness.next_use_after(a, op);
                let b_dist = liveness.next_use_after(b, op);
                a_dist.cmp(&b_dist).then(a.stack_size().cmp(&b.stack_size()))
            });
            while must_spill > 0 {
                let candidate = candidates.pop().unwrap_or_else(|| {
                    panic!(
                        "unable to spill sufficient capacity to hold all operands on stack at one \
                         time at {op}"
                    )
                });
                // If we're spilling an operand of I, we can multiple the amount of space
                // freed by the spill by the number of uses of the spilled value in I
                let num_uses =
                    core::cmp::max(1, args.iter().filter(|v| *v == candidate.value()).count());
                let freed = candidate.stack_size() * num_uses;
                must_spill = must_spill.saturating_sub(freed);
                to_spill.insert(candidate);
            }
        } else {
            log::trace!(target: "spills", "  spills required on exit: no");
        }

        // Emit spills first, to make space for reloaded values on the operand stack
        for spill in to_spill.iter() {
            if s.insert(*spill) {
                log::trace!(target: "spills", "emitting spill for {spill}");
                self.spill(place, spill.value(), span);
            }

            // Remove spilled values from W
            w.remove(spill);
        }

        // Emit reloads for those operands of I not yet in W
        for reload in to_reload {
            // We only need to emit a reload for a given value once
            if w.insert(reload) {
                log::trace!(target: "spills", "emitting reload for {reload}");
                // By definition, if we are emitting a reload, the value must have been spilled
                s.insert(reload);
                self.reload(place, reload.value(), span);
            }
        }

        // At this point, we've emitted our spills/reloads, so we need to prepare W for the next
        // instruction by applying the effects of the instruction to W, i.e. consuming those
        // operands which are consumed, and adding instruction results.
        //
        // First, we remove operands from W which are no longer live-after I, _except_ any
        // which are used as successor arguments. This is because we must know which successor
        // arguments are still in W at the block terminator when we are computing what to spill
        // or reload along each control flow edge.
        //
        // Second, if applicable, we add in the instruction results
        log::trace!(target: "spills", "  applying effects of operation to W..");
        if let Some(branch) = op.as_trait::<dyn BranchOpInterface>() {
            log::trace!(target: "spills", "  op is a control flow branch, attempting to resolve a single successor");
            // Try to determine if we can select a single successor here
            let mut operands = SmallVec::<[Option<Box<dyn AttributeValue>>; 4]>::with_capacity(
                op.operands().group(0).len(),
            );
            for operand in op.operands().group(0).iter() {
                let value = operand.borrow().as_value_ref();
                let constant_prop_lattice =
                    liveness.solver().get::<Lattice<ConstantValue>, _>(&value);
                if let Some(lattice) = constant_prop_lattice {
                    if lattice.value().is_uninitialized() {
                        operands.push(None);
                        continue;
                    }
                    operands.push(lattice.value().constant_value());
                }
            }

            if let Some(succ) = branch.get_successor_for_operands(&operands) {
                log::trace!(target: "spills", "  resolved single succeessor {}", succ.successor());
                w.retain(|o| {
                    op.operands()
                        .group(succ.successor_operand_group())
                        .iter()
                        .any(|arg| arg.borrow().as_value_ref() == o.value())
                        || liveness.is_live_after(o, op)
                });
            } else {
                let successor_operand_groups = op
                    .successors()
                    .iter()
                    .filter_map(|s| {
                        let successor = s.successor();
                        if liveness.is_block_executable(successor) {
                            Some(s.successor_operand_group())
                        } else {
                            None
                        }
                    })
                    .collect::<SmallVec<[_; 2]>>();
                log::trace!(target: "spills", "  resolved {} successors", successor_operand_groups.len());
                w.retain(|o| {
                    let is_succ_arg = successor_operand_groups.iter().copied().any(|succ| {
                        op.operands()
                            .group(succ)
                            .iter()
                            .any(|arg| arg.borrow().as_value_ref() == o.value())
                    });
                    is_succ_arg || liveness.is_live_after(o, op)
                });
            }
        } else {
            log::trace!(target: "spills", "  '{}' is a primitive operation", op.name());

            // This is a simple operation
            log::trace!(target: "spills", "  removing dead operands from W");
            w.retain(|o| liveness.is_live_after(o, op));
            log::trace!(target: "spills", "  adding results to W");
            w.extend(results.iter().map(ValueOrAlias::new));
        }

        log::trace!(target: "spills", "  W^exit = {w:?}");
        log::trace!(target: "spills", "  S^exit = {s:?}");
    }
}
