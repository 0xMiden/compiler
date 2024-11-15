use alloc::collections::VecDeque;

use smallvec::SmallVec;

use crate::{
    adt::{SmallMap, SmallSet},
    cfg::Graph,
    dataflow::{
        analyses::{
            constant_propagation::ConstantValue,
            dce::PredecessorState,
            liveness::{LivenessAnalysis, LOOP_EXIT_DISTANCE},
        },
        Lattice,
    },
    dialects::hir::Function,
    dominance::DominanceInfo,
    formatter::DisplayValues,
    loops::{Loop, LoopForest, LoopInfo},
    pass::{Analysis, AnalysisManager},
    traits::{BranchOpInterface, IsolatedFromAbove, Terminator},
    AttributeValue, Block, BlockArgument, BlockOperand, BlockRef, EntityWithId, FxHashMap,
    FxHashSet, LoopLikeOpInterface, Op, Operation, OperationRef, ProgramPoint, Region,
    RegionBranchOpInterface, RegionBranchPoint, RegionRef, Report, SourceSpan, Spanned,
    SuccessorOperands, Value, ValueRef,
};

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
///       This is also one means by which values are added to S, as by definition a reload
///       implies that the value must have been spilled, or it would still be in W. Thus, when
///       we emit reloads, we also ensure that the reloaded value is added to S.
///    b. If a reload would cause |W| to exceed K, we must select values in W to spill. Candidates
///       are selected from the set of values in W which are not operands of I, prioritized first
///       by greatest next-use distance, then by stack consumption, as determined by the
///       representation of the value type on the operand stack.
///    c. By definition, none of I's results can be in W directly in front of I, so we must
///       always ensure that W has sufficient capacity to hold all of I's results. The analysis
///       of sufficient capacity is somewhat subtle:
///       - Any of I's operands that are live-at I, but _not_ live-after I, do _not_ count towards
///         the operand stack usage when calculating available capacity for the results. This is
///         because those operands will be consumed, and their space can be re-used for results.
///       - Any of I's operands that are live-after I, however, _do_ count towards the stack usage
///       - If W still has insufficient capacity for all the results, we must select candidates
///         to spill. Candidates are the set of values in W which are either not operands of I,
///         or are operands of I which are live-after I. Selection criteria is the same as before.
///
///    d. Operands of I which are _not_ live-after I, are removed from W on exit from I, thus W
///       reflects only those values which are live at the current program point.
///    e. Lastly, when we select a value to be spilled, we only emit spill instructions for those
///       values which are not yet in S, i.e. they have not yet been spilled; and which have a
///       finite next-use distance, i.e. the value is still live. If a value to be spilled _is_
///       in S and/or is unused after that point in the program, we can elide the spill entirely.
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
///       which are awaiting a rewrite while we search for the nearest dominating definition.
///    b. We find a reload of a value in S. This reload is, by construction, the nearest dominating
///       definition for all uses of the reloaded value that we have found so far. We rewrite all of
///       those uses to reference the reloaded value, and remove them from the list.
///    c. We find the original definition of a value in S. This is similar to what happens when we
///       find a reload, except no rewrite is needed, so we simply remove all pending uses of that
///       value from the list.
///    d. We reach the top of the block. Note that block parameters are treated as definitions, so
///       those are handled first as described in the previous point. However, an additional step
///       is required here: If the current block is in the iterated dominance frontier for S, i.e.
///       for any value in S, the current block is in the dominance frontier of the original
///       definition of that value - then for each such value for which we have found at least one
///       use, we must add a new block parameter representing that value; rewrite all uses we have
///       found so far to use the block parameter instead; remove those uses from the list; and
///       lastly, rewrite the branch instruction in each predecessor to pass the value as a new block
///       argument when branching to the current block.
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
#[derive(Default)]
pub struct SpillAnalysis {
    // The set of control flow edges that must be split to accommodate spills/reloads.
    pub splits: SmallVec<[SplitInfo; 1]>,
    // The set of values that have been spilled
    pub spilled: FxHashSet<ValueRef>,
    // The spills themselves
    pub spills: SmallVec<[SpillInfo; 4]>,
    // The set of instructions corresponding to the reload of a spilled value
    pub reloads: SmallVec<[ReloadInfo; 4]>,
    // The set of operands in registers on entry to a given block
    w_entries: FxHashMap<BlockRef, SmallSet<Operand, 4>>,
    // The set of operands in registers on exit from a given block
    w_exits: FxHashMap<BlockRef, SmallSet<Operand, 4>>,
    // The set of operands that have been spilled so far, on exit from a given block
    s_exits: FxHashMap<BlockRef, SmallSet<Operand, 4>>,
}

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
        let dominfo = analysis_manager.get_analysis::<DominanceInfo>()?;
        let loops = analysis_manager.get_analysis::<LoopInfo>()?;
        let liveness = analysis_manager.get_analysis_for::<LivenessAnalysis, Function>()?;

        let body = op.body().as_region_ref();
        let mut w = SmallSet::default();
        let mut s = SmallSet::default();
        self.visit_cfg(
            op.as_operation(),
            &body.borrow(),
            &dominfo,
            &loops,
            &liveness,
            analysis_manager,
            &mut w,
            &mut s,
        )
    }

    fn invalidate(&self, preserved_analyses: &mut crate::pass::PreservedAnalyses) -> bool {
        !preserved_analyses.is_preserved::<LivenessAnalysis>()
    }
}

/// The state of the W and S sets on entry to a given block
#[derive(Debug)]
struct BlockInfo {
    block_id: BlockRef,
    w_entry: SmallSet<Operand, 4>,
    s_entry: SmallSet<Operand, 4>,
}

/// Uniquely identifies a [SplitInfo]
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

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum CfgEdge {
    Local {
        from: BlockRef,
        to: BlockRef,
    },
    Regional {
        op: OperationRef,
        from: RegionBranchPoint,
        to: RegionBranchPoint,
    },
}

/// Metadata about a control flow edge which needs to be split in order to accommodate spills and/or
/// reloads along that edge.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct SplitInfo {
    pub id: Split,
    /// The edge to split
    pub edge: CfgEdge,
    /// The block representing the split, if materialized
    pub split: Option<BlockRef>,
}

impl SplitInfo {
    pub fn new(id: Split, edge: CfgEdge) -> Self {
        Self {
            id,
            edge,
            split: None,
        }
    }
}

/// Uniquely identifies a [SpillInfo]
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

/// An [Operand] is a possibly-aliased [Value], combined with the size of that value on the
/// Miden operand stack. This extra information is used to not only compute whether or not we
/// need to spill values during execution of a function, and how to prioritize those spills;
/// but also to track aliases of a [Value] introduced when we insert reloads of a spilled value.
///
/// Once a spilled value is reloaded, the SSA property of the CFG is broken, as we now have two
/// definitions of the same [Value]. To restore the SSA property, we have to assign the reloaded
/// value a new id, and then update all uses of the reloaded value dominated by that reload to
/// refer to the new [Value]. We use the `alias` field of [Operand] to track distinct reloads of
/// a given [Value] during the initial insertion of reloads.
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Operand {
    /// The SSA value of this operand
    pub value: ValueRef,
    /// When an SSA value is used multiple times by an instruction, each use must be accounted for
    /// on the operand stack in order to properly determine whether a spill is needed or not.
    /// We assign each unique copy an integer id in the register file to ensure this.
    pub alias: u16,
}

impl Operand {
    pub fn new(value: ValueRef) -> Self {
        Self { value, alias: 0 }
    }

    pub fn size(&self) -> usize {
        self.value.borrow().ty().size_in_felts()
    }
}

/// The maximum number of operand stack slots which can be assigned without spills.
const K: usize = 16;

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
    pub fn is_spilled_at(&self, value: ValueRef, pp: impl Into<ProgramPoint>) -> bool {
        let place = match pp.into() {
            ProgramPoint::Block {
                block: split_block, ..
            } => match self.splits.iter().find(|split| split.split == Some(split_block)) {
                Some(split) => Placement::Split(split.id),
                None => Placement::At(ProgramPoint::after(split_block)),
            },
            pp => Placement::At(pp),
        };
        self.spills.iter().any(|info| info.value == value && info.place == place)
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

    /// Same as [SpillAnalysis::spills], but as a mutable reference
    pub fn spills_mut(&mut self) -> &mut [SpillInfo] {
        self.spills.as_mut_slice()
    }

    /// Returns true if `value` is reloaded at some point
    pub fn is_reloaded(&self, value: &ValueRef) -> bool {
        self.reloads.iter().any(|info| &info.value == value)
    }

    /// Returns true if `value` is reloaded at the given program point (i.e. inserted before)
    pub fn is_reloaded_at(&self, value: ValueRef, pp: impl Into<ProgramPoint>) -> bool {
        let place = match pp.into() {
            ProgramPoint::Block {
                block: split_block, ..
            } => match self.splits.iter().find(|split| split.split == Some(split_block)) {
                Some(split) => Placement::Split(split.id),
                None => Placement::At(ProgramPoint::after(split_block)),
            },
            pp => Placement::At(pp),
        };
        self.reloads.iter().any(|info| info.value == value && info.place == place)
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

    /// Same as [SpillAnalysis::reloads], but as a mutable reference
    pub fn reloads_mut(&mut self) -> &mut [ReloadInfo] {
        self.reloads.as_mut_slice()
    }

    /// Returns the operands in W upon entry to `block`
    pub fn w_entry(&self, block: &BlockRef) -> &[Operand] {
        self.w_entries[block].as_slice()
    }

    /// Returns the operands in W upon exit from `block`
    pub fn w_exit(&self, block: &BlockRef) -> &[Operand] {
        self.w_exits[block].as_slice()
    }

    /// Returns the operands in S upon exit from `block`
    pub fn s_exit(&self, block: &BlockRef) -> &[Operand] {
        self.s_exits[block].as_slice()
    }

    pub fn set_materialized_split(&mut self, split: Split, block: BlockRef) {
        self.splits[split.as_usize()].split = Some(block);
    }

    pub fn set_materialized_spill(&mut self, spill: Spill, inst: OperationRef) {
        self.spills[spill.as_usize()].inst = Some(inst);
    }

    pub fn set_materialized_reload(&mut self, reload: Reload, inst: OperationRef) {
        self.reloads[reload.as_usize()].inst = Some(inst);
    }

    fn spill(&mut self, place: Placement, value: ValueRef, span: SourceSpan) -> Spill {
        let id = Spill::new(self.spills.len());
        self.spilled.insert(value);
        self.spills.push(SpillInfo {
            id,
            place,
            value,
            span,
            inst: None,
        });
        id
    }

    fn reload(&mut self, place: Placement, value: ValueRef, span: SourceSpan) -> Reload {
        let id = Reload::new(self.reloads.len());
        self.reloads.push(ReloadInfo {
            id,
            place,
            value,
            span,
            inst: None,
        });
        id
    }

    fn split_local(&mut self, block: BlockRef, predecessor: &BlockOperand) -> Split {
        let id = Split::new(self.splits.len());
        self.splits.push(SplitInfo {
            id,
            edge: CfgEdge::Local {
                from: predecessor.block,
                to: block,
            },
            split: None,
        });
        id
    }

    fn split_regional(
        &mut self,
        op: OperationRef,
        to: RegionBranchPoint,
        predecessor: RegionBranchPoint,
    ) -> Split {
        let id = Split::new(self.splits.len());
        self.splits.push(SplitInfo {
            id,
            edge: CfgEdge::Regional {
                op,
                from: predecessor,
                to,
            },
            split: None,
        });
        id
    }
}

/// Analysis
#[allow(clippy::too_many_arguments)]
impl SpillAnalysis {
    fn visit_operation(
        &mut self,
        op: &Operation,
        liveness: &LivenessAnalysis,
        analysis_manager: &AnalysisManager,
        w: &mut SmallSet<Operand, 4>,
        s: &mut SmallSet<Operand, 4>,
    ) -> Result<(), Report> {
        // If this op is a loop-like operation, we must handle it differently than non-looping
        // region control flow ops.
        if let Some(loop_like) = op.as_trait::<dyn LoopLikeOpInterface>() {
            return self.visit_loop_like_op(loop_like, liveness, analysis_manager, w, s);
        }

        // Handle non-looping region control flow ops
        if let Some(branch) = op.as_trait::<dyn RegionBranchOpInterface>() {
            assert!(
                !branch.has_loop(),
                "expected op to implement LoopLikeOpInterface due to loops in its region control \
                 flow graph"
            );
            return self.visit_region_branch_op(branch, liveness, analysis_manager, w, s);
        }

        // Does this operation have regions? If so, we expect that since it does not implement
        // RegionBranchOpInterface, that it is IsolatedFromAbove, and consists of a single region,
        // e.g. `hir.function`
        if op.has_regions() {
            assert!(op.implements::<dyn IsolatedFromAbove>());
            assert_eq!(op.num_regions(), 1, "expected op to have only a single region");

            let am = analysis_manager.nest(op.as_operation_ref());
            let dominfo = am.get_analysis::<DominanceInfo>()?;
            let loops = am.get_analysis::<LoopInfo>()?;
            return self.visit_cfg(op, &op.region(0), &dominfo, &loops, liveness, am, w, s);
        }

        // This is a simple operation, so `w` and `s` must be available
        self.min(op, w, s, liveness);

        Ok(())
    }

    fn visit_loop_like_op(
        &mut self,
        loop_like: &dyn LoopLikeOpInterface,
        liveness: &LivenessAnalysis,
        analysis_manager: &AnalysisManager,
        w: &mut SmallSet<Operand, 4>,
        s: &mut SmallSet<Operand, 4>,
    ) -> Result<(), Report> {
        let op = loop_like.as_operation();
        let branch = op
            .as_trait::<dyn RegionBranchOpInterface>()
            .expect("loop-like ops must implement RegionBranchOpInterface");

        // We expect loop-like ops to have a single loop header region, which is where we will
        // begin
        let header = loop_like.get_loop_header_region();

        // Visit the region CFG reachable from the header, in reverse post-order, propagating the
        // state of W and S through the loop
        self.visit_region_graph(branch, &header.borrow(), true, liveness, analysis_manager, w, s)?;

        // Unify W and S on exit from the loop. There are two possibilities:
        //
        // 1. Control conditionally enters the loop, in which case we will have two predecessors
        //    to unify, the op itself, and the exit from the loop body.
        // 2. Control unconditionally enters the loop, in which case we will have a single
        //    predecessor from which to derive the W and S sets on exit. A loop-like op with
        //    multiple exits is not currently supported.
        let op = loop_like.as_operation();
        let w_entry = self.compute_w_entry_for_op_exit(op, &*w, liveness);
        let s_entry = self.compute_s_entry_for_op_exit(op, &*s, &w_entry, liveness);

        let preds = liveness
            .solver()
            .get::<PredecessorState, _>(&ProgramPoint::after(op))
            .expect("expected predecessor state to have been computed for `op`");

        for pred in preds.known_predecessors().iter().copied() {
            let pred_inputs = preds.successor_inputs(&pred);
            self.compute_spills_and_reloads_on_op_exit(
                branch,
                pred,
                pred_inputs,
                &w_entry,
                &s_entry,
                &*w,
                &*s,
                liveness,
            );
        }

        *w = w_entry;
        *s = s_entry;

        Ok(())
    }

    fn visit_region_branch_op(
        &mut self,
        branch: &dyn RegionBranchOpInterface,
        liveness: &LivenessAnalysis,
        analysis_manager: &AnalysisManager,
        w: &mut SmallSet<Operand, 4>,
        s: &mut SmallSet<Operand, 4>,
    ) -> Result<(), Report> {
        let op = branch.as_operation();

        // Determine the set of entry regions (if any)
        let mut operands = SmallVec::<[Option<Box<dyn AttributeValue>>; 4]>::with_capacity(
            op.operands().group(0).len(),
        );
        for operand in op.operands().group(0).iter() {
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

        // Visit the region graphs reachable from any of the entry regions, in order to
        // ensure that the contents of W and S are propagated through to all possible
        // predecessor edges at the exit from `op`
        for entry in branch.get_entry_successor_regions(&operands) {
            let Some(entry) = entry.into_successor() else {
                continue;
            };

            self.visit_region_graph(
                branch,
                &entry.borrow(),
                false,
                liveness,
                analysis_manager,
                &*w,
                &*s,
            )?;
        }

        // We've threaded W and S through the regions of `op`, now it is necessary for us to
        // compute W and S at the implicit join point represented by control flow exits from
        // any of those regions (or from before the op to after, in cases where the op has some
        // form of conditional control flow guarding its regions).
        let op = branch.as_operation();
        let w_entry = self.compute_w_entry_for_op_exit(op, &*w, liveness);
        let s_entry = self.compute_s_entry_for_op_exit(op, &*s, &w_entry, liveness);

        let preds = liveness
            .solver()
            .get::<PredecessorState, _>(&ProgramPoint::after(op))
            .expect("expected predecessor state to have been computed for `op`");

        for pred in preds.known_predecessors().iter().copied() {
            let pred_inputs = preds.successor_inputs(&pred);
            self.compute_spills_and_reloads_on_op_exit(
                branch,
                pred,
                pred_inputs,
                &w_entry,
                &s_entry,
                &*w,
                &*s,
                liveness,
            );
        }

        *w = w_entry;
        *s = s_entry;

        Ok(())
    }

    fn visit_region_graph(
        &mut self,
        branch: &dyn RegionBranchOpInterface,
        entry: &Region,
        is_loop_header: bool,
        liveness: &LivenessAnalysis,
        analysis_manager: &AnalysisManager,
        w_op: &SmallSet<Operand, 4>,
        s_op: &SmallSet<Operand, 4>,
    ) -> Result<(), Report> {
        // Compute the reverse post-order traversal of the region graph from `entry`. We will then
        // visit all regions of the graph in order to propagate W and S through them, just like we
        // do for CFGs in a single region (where the graph is blocks rather than regions)
        let mut postorder = Region::postorder_region_graph(entry);

        // If a region has a predecessor which it dominates (i.e. control flow always flows through
        // the region in question before the given predecessor), then we must defer computing spills
        // and reloads for that edge until we have visited the predecessor. This map is used to
        // track deferred edges for each region.
        let mut deferred = Vec::<(RegionRef, SmallVec<[BlockRef; 2]>)>::default();

        let branch_op = branch.as_operation().as_operation_ref();

        let mut visited = SmallSet::<RegionRef, 4>::default();

        while let Some(region_ref) = postorder.pop() {
            if !visited.insert(region_ref) {
                continue;
            }
            let is_entry_region = region_ref == entry.as_region_ref();
            let region = region_ref.borrow();
            let entry = region.entry();
            let entry_ref = entry.as_block_ref();
            let is_executable = liveness.is_block_executable(entry_ref);
            if !is_executable {
                continue;
            }

            // Compute W^entry(R)
            let w_entry = if is_entry_region && is_loop_header {
                self.compute_w_entry_loop_like_op(&region, &entry, liveness)
            } else {
                self.compute_w_entry_normal(branch.as_operation(), &entry, w_op, liveness)
            };
            self.w_entries.entry(entry_ref).or_default().clone_from(&w_entry);

            // Compute S^entry(R)
            let s_entry =
                self.compute_s_entry(branch.as_operation(), &entry, s_op, &w_entry, liveness);

            let mut block_info = BlockInfo {
                block_id: entry_ref,
                w_entry,
                s_entry,
            };

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
            //
            // TODO: How do we actually handle spills on region control flow edges, when multiple
            // edges are present, and the spill/reload is only on one of them? We can't introduce
            // new blocks in those regions in order to split edges. We'll need to evaluate whether
            // this occurs in practice, and if so, determine how best to handle it.
            let pred_state = liveness
                .solver()
                .get::<PredecessorState, _>(&ProgramPoint::at_start_of(entry_ref))
                .expect("expected predecessor state to be available for executable block");
            let predecessors = pred_state.known_predecessors().iter().copied().filter(|pred| {
                if pred == &branch_op {
                    return true;
                }
                let pred_block = pred.borrow().parent().unwrap();
                liveness.is_block_executable(pred_block)
            });
            let mut deferred_preds = SmallVec::<[BlockRef; 2]>::default();
            for pred in predecessors {
                self.compute_region_control_flow_edge_spills_and_reloads(
                    branch,
                    &block_info,
                    pred,
                    pred_state.successor_inputs(&pred),
                    w_op,
                    s_op,
                    &mut deferred_preds,
                    liveness,
                );
            }
            if !deferred_preds.is_empty() {
                deferred.push((region_ref, deferred_preds));
            }

            for op in entry.body() {
                self.visit_operation(
                    &op,
                    liveness,
                    analysis_manager,
                    &mut block_info.w_entry,
                    &mut block_info.s_entry,
                )?;
            }

            self.w_exits.insert(entry_ref, block_info.w_entry);
            self.s_exits.insert(entry_ref, block_info.s_entry);
        }

        // We've visited all regions at least once, now we need to go back and insert spills/reloads
        // along loopback edges, as we skipped those on the first pass
        for (region_ref, deferred_preds) in deferred {
            let region = region_ref.borrow();
            let block = region.entry();

            // W^entry(B)
            let block_ref = block.as_block_ref();
            let w_entry = self.w_entries[&block_ref].clone();

            // Compute S^entry(B)
            let s_entry =
                self.compute_s_entry(branch.as_operation(), &block, s_op, &w_entry, liveness);

            let block_info = BlockInfo {
                block_id: block_ref,
                w_entry,
                s_entry,
            };

            // For each predecessor P of B, insert spills/reloads along the inbound control flow
            // edge as follows:
            //
            // * All variables in W^entry(B) \ W^exit(P) need to be reloaded
            // * All variables in (S^entry(B) \ S^exit(P)) ∩ W^exit(P) need to be spilled
            //
            // If a given predecessor has not been processed yet, skip P, and revisit the edge later
            // after we have processed P.
            let pred_state = liveness
                .solver()
                .get::<PredecessorState, _>(&ProgramPoint::at_start_of(&*block))
                .expect("expected predecessor state to be available for executable block");
            let predecessors = pred_state.known_predecessors().iter().copied().filter(|pred| {
                // The op itself will never have been deferred
                if pred == &branch_op {
                    return false;
                }
                let pred_block = pred.borrow().parent().unwrap();
                // Only visit predecessors that were deferred
                liveness.is_block_executable(pred_block) && deferred_preds.contains(&pred_block)
            });

            let mut _defer = SmallVec::default();
            for pred in predecessors {
                self.compute_region_control_flow_edge_spills_and_reloads(
                    branch,
                    &block_info,
                    pred,
                    pred_state.successor_inputs(&pred),
                    w_op,
                    s_op,
                    &mut _defer,
                    liveness,
                );
            }
        }

        Ok(())
    }

    fn visit_cfg(
        &mut self,
        op: &Operation,
        region: &Region,
        dominfo: &DominanceInfo,
        loops: &LoopInfo,
        liveness: &LivenessAnalysis,
        analysis_manager: AnalysisManager,
        w_op: &mut SmallSet<Operand, 4>,
        s_op: &mut SmallSet<Operand, 4>,
    ) -> Result<(), Report> {
        // Get the analysis data we need for this CFG
        let body = region.as_region_ref();
        let domtree = dominfo.info().dominance(body);
        let loop_forest = loops.get(&body);

        // If a block has a predecessor which it dominates (i.e. control flow always flows through
        // the block in question before the given predecessor), then we must defer computing spills
        // and reloads for that edge until we have visited the predecessor. This map is used to
        // track deferred edges for each block.
        let mut deferred = Vec::<(BlockRef, SmallVec<[BlockRef; 2]>)>::default();

        // Visit blocks in CFG reverse post-order
        let mut block_q = VecDeque::from(domtree.reverse_postorder());
        while let Some(node) = block_q.pop_front() {
            let Some(block_ref) = node.block() else {
                continue;
            };
            let block = block_ref.borrow();

            // Compute W^entry(B)
            let w_entry = self.compute_w_entry(op, &block, w_op, loop_forest, liveness);
            self.w_entries.entry(block_ref).or_default().clone_from(&w_entry);

            // Compute S^entry(B)
            let s_entry = self.compute_s_entry(op, &block, s_op, &w_entry, liveness);

            let mut block_info = BlockInfo {
                block_id: block_ref,
                w_entry,
                s_entry,
            };

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
            for pred in block.predecessors().filter(|p| liveness.is_block_executable(p.block)) {
                // As soon as we need to start inserting spills/reloads, mark the function changed
                self.compute_control_flow_edge_spills_and_reloads(
                    &block_info,
                    &pred,
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
            for op in block.body() {
                self.visit_operation(
                    &op,
                    liveness,
                    &analysis_manager,
                    &mut block_info.w_entry,
                    &mut block_info.s_entry,
                )?;
            }

            self.w_exits.insert(block_ref, block_info.w_entry);
            self.s_exits.insert(block_ref, block_info.s_entry);
        }

        // We've visited all blocks at least once, now we need to go back and insert
        // spills/reloads along loopback edges, as we skipped those on the first pass
        for (block_ref, preds) in deferred {
            let block = block_ref.borrow();

            // W^entry(B)
            let w_entry = self.w_entries[&block_ref].clone();

            // Compute S^entry(B)
            let s_entry = self.compute_s_entry(op, &block, s_op, &w_entry, liveness);

            let block_info = BlockInfo {
                block_id: block_ref,
                w_entry,
                s_entry,
            };

            // For each predecessor P of B, insert spills/reloads along the inbound control flow
            // edge as follows:
            //
            // * All variables in W^entry(B) \ W^exit(P) need to be reloaded
            // * All variables in (S^entry(B) \ S^exit(P)) ∩ W^exit(P) need to be spilled
            //
            // If a given predecessor has not been processed yet, skip P, and revisit the edge later
            // after we have processed P.
            let mut _defer = SmallVec::default();
            for pred in block.predecessors().filter(|p| liveness.is_block_executable(p.block)) {
                // Only visit predecessors that were deferred
                if !preds.contains(&pred.block) {
                    continue;
                }

                self.compute_control_flow_edge_spills_and_reloads(
                    &block_info,
                    &pred,
                    &mut _defer,
                    liveness,
                );
            }
        }

        Ok(())
    }

    /// When computing the contents of W and S upon exit from a region control-flow op, we treat
    /// the program point "after" the op as an implicit join point. As a result, we must compute
    /// the equivalent of W^entry at that program point, similar to how we do so for a block in a
    /// normal CFG that represents a point where control flow joins.
    ///
    /// We then use the resulting W^entry to compute the corresponding S^entry, and these sets are
    /// then used when proceeding to the next op in the containing block.
    fn compute_w_entry_for_op_exit(
        &mut self,
        op: &Operation,
        w_in: &SmallSet<Operand, 4>,
        liveness: &LivenessAnalysis,
    ) -> SmallSet<Operand, 4> {
        let mut freq = SmallMap::<Operand, u8, 4>::default();
        let mut take = SmallSet::<Operand, 4>::default();
        let mut cand = SmallSet::<Operand, 4>::default();

        // Result of `op` are always in W^exit(op) by definition
        for result in op.results().iter().copied() {
            take.insert(Operand::new(result as ValueRef));
        }

        // Not sure how to handle too many results from a region control-flow op, for now we just
        // assert.
        assert!(
            take.iter().map(|o| o.size()).sum::<usize>() <= K,
            "unhandled spills implied by op results"
        );

        // The predecessors of the program point we're computing for, are either:
        //
        // 1. The `op` itself, i.e. it conditionally skips any of its regions and immediately exits
        // 2. One or more exits from within regions of `op`
        //
        // To obtain these predecessors, we're reliant on the results of dead code analysis, which
        // will have computed all known predecessors for this program point.
        let mut num_predecessors = 0usize;
        let after_op = ProgramPoint::after(op);
        let next_uses = liveness.next_uses_at(&after_op).unwrap();
        let pred_state = liveness.solver().get::<PredecessorState, _>(&after_op);
        if let Some(pred_state) = pred_state {
            let op_ref = op.as_operation_ref();
            for pred in pred_state.known_predecessors() {
                num_predecessors += 1;

                // Is `pred` the operation itself? If so, we'll start with what's in `w_in`, since
                // that precisely represents what is in W on entry to `op`.
                if pred == &op_ref {
                    for o in w_in.iter().copied() {
                        if next_uses.is_live(&o.value) {
                            *freq.entry(o).or_insert(0) += 1;
                            cand.insert(o);
                        }
                    }
                    continue;
                }

                // Otherwise, the predecessor is a region within `op`, so we want the `w_exit` of
                // the block containing `pred`, stripped of anything that is not live at the
                // current program point
                let pred_block = pred.borrow().parent().unwrap();
                for o in self.w_exits[&pred_block].iter().copied() {
                    // Do not add candidates which are either:
                    //
                    // 1. Defined within `op`
                    // 2. Are not live after `op`
                    if next_uses.is_live(&o.value) {
                        *freq.entry(o).or_insert(0) += 1;
                        cand.insert(o);
                    }
                }
            }
        }

        for (&v, &count) in freq.iter() {
            if count as usize == num_predecessors {
                cand.remove(&v);
                take.insert(v);
            }
        }

        // We currently do not have a sane way to handle paths to the exit of `op` containing more
        // than K values. We simply bail for now.
        let taken = take.iter().map(|o| o.size()).sum::<usize>();
        assert!(
            taken <= K,
            "implicit operand stack overflow along exiting control flow edges of '{}'",
            op.name()
        );

        // Prefer to select candidates with the smallest next-use distance, otherwise all else being
        // equal, choose to keep smaller values on the operand stack, and spill larger values, thus
        // freeing more space when spills are needed.
        let mut cand = cand.into_vec();
        cand.sort_by(|a, b| {
            next_uses
                .distance(&a.value)
                .cmp(&next_uses.distance(&b.value))
                .then(a.size().cmp(&b.size()))
        });

        let mut available = K - taken;
        let mut cand = cand.into_iter();
        while available > 0 {
            if let Some(candidate) = cand.next() {
                let size = candidate.size();
                if size <= available {
                    take.insert(candidate);
                    available -= size;
                    continue;
                }
            }
            break;
        }

        take
    }

    fn compute_s_entry_for_op_exit(
        &mut self,
        op: &Operation,
        s_in: &SmallSet<Operand, 4>,
        w_entry: &SmallSet<Operand, 4>,
        liveness: &LivenessAnalysis,
    ) -> SmallSet<Operand, 4> {
        let mut s_entry = SmallSet::<Operand, 4>::default();

        let Some(pred_state) =
            liveness.solver().get::<PredecessorState, _>(&ProgramPoint::after(op))
        else {
            return s_entry;
        };

        //let next_uses = liveness.next_uses_at(&ProgramPoint::at_start_of(block)).unwrap();
        let op_ref = op.as_operation_ref();
        for pred in pred_state.known_predecessors() {
            // Is `pred` the operation itself?
            if pred == &op_ref {
                s_entry = s_entry.into_union(s_in);
            } else {
                let pred_block = pred.borrow().parent().unwrap();
                if let Some(s_exitp) = self.s_exits.get(&pred_block) {
                    // Union any spills of values defined above `op`
                    for spilled in s_exitp.iter().copied() {
                        let is_visible =
                            if let Some(defining_op) = spilled.value.borrow().get_defining_op() {
                                defining_op.borrow().is_proper_ancestor_of(op)
                            } else {
                                let defining_region = spilled
                                    .value
                                    .borrow()
                                    .downcast_ref::<BlockArgument>()
                                    .unwrap()
                                    .parent_region()
                                    .unwrap();
                                let defining_op = defining_region.borrow().parent().unwrap();
                                defining_op.borrow().is_ancestor_of(op)
                            };
                        if is_visible {
                            s_entry.insert(spilled);
                        }
                    }
                }
            }
        }

        s_entry.into_intersection(w_entry)
    }

    fn compute_s_entry(
        &mut self,
        op: &Operation,
        block: &Block,
        s_in: &SmallSet<Operand, 4>,
        w_entry: &SmallSet<Operand, 4>,
        liveness: &LivenessAnalysis,
    ) -> SmallSet<Operand, 4> {
        let mut s_entry = SmallSet::<Operand, 4>::default();

        if op.implements::<dyn RegionBranchOpInterface>() && block.is_entry_block() {
            let pred_state =
                liveness.solver().get::<PredecessorState, _>(&ProgramPoint::at_start_of(block));
            if let Some(pred_state) = pred_state {
                //let next_uses = liveness.next_uses_at(&ProgramPoint::at_start_of(block)).unwrap();
                let op_ref = op.as_operation_ref();
                for pred in pred_state.known_predecessors() {
                    // Is `pred` the operation itself?
                    if pred == &op_ref {
                        s_entry = s_entry.into_union(s_in);
                    } else {
                        let pred_block = pred.borrow().parent().unwrap();
                        if let Some(s_exitp) = self.s_exits.get(&pred_block) {
                            // Union any spills of values defined above `op`
                            for spilled in s_exitp.iter().copied() {
                                let is_visible = if let Some(defining_op) =
                                    spilled.value.borrow().get_defining_op()
                                {
                                    defining_op.borrow().is_proper_ancestor_of(op)
                                } else {
                                    let defining_region = spilled
                                        .value
                                        .borrow()
                                        .downcast_ref::<BlockArgument>()
                                        .unwrap()
                                        .parent_region()
                                        .unwrap();
                                    let defining_op = defining_region.borrow().parent().unwrap();
                                    defining_op.borrow().is_ancestor_of(op)
                                };
                                if is_visible {
                                    s_entry.insert(spilled);
                                }
                            }
                        }
                    }
                }
            }
        } else {
            let predecessors =
                block.predecessors().filter(|p| liveness.is_block_executable(p.block));
            for pred in predecessors {
                if let Some(s_exitp) = self.s_exits.get(&pred.block) {
                    s_entry = s_entry.into_union(s_exitp);
                }
            }
        }

        s_entry.into_intersection(w_entry)
    }

    fn compute_w_entry(
        &mut self,
        op: &Operation,
        block: &Block,
        w_in: &SmallSet<Operand, 4>,
        loops: Option<&LoopForest>,
        liveness: &LivenessAnalysis,
    ) -> SmallSet<Operand, 4> {
        // If this is the entry block for an IsolatedFromAbove region, then the operands in w_entry
        // are guaranteed to be equal to the set of block arguments, and thus we don't need to do
        // anything further.
        if block.is_entry_block() {
            let is_isolated_from_above = op.implements::<dyn IsolatedFromAbove>();
            if is_isolated_from_above {
                return block
                    .arguments()
                    .iter()
                    .copied()
                    .map(|arg| Operand::new(arg as ValueRef))
                    .collect();
            }
        }

        if let Some(loops) = loops {
            let block_ref = block.as_block_ref();
            if loops.is_loop_header(block_ref) {
                let block_loop = loops.loop_for(block_ref).unwrap();
                return self.compute_w_entry_loop(block, &block_loop, liveness);
            }
        }

        self.compute_w_entry_normal(op, block, w_in, liveness)
    }

    fn compute_w_entry_normal(
        &mut self,
        op: &Operation,
        block: &Block,
        w_in: &SmallSet<Operand, 4>,
        liveness: &LivenessAnalysis,
    ) -> SmallSet<Operand, 4> {
        let mut freq = SmallMap::<Operand, u8, 4>::default();
        let mut take = SmallSet::<Operand, 4>::default();
        let mut cand = SmallSet::<Operand, 4>::default();

        // Block arguments are always in w_entry by definition
        for arg in block.arguments().iter().copied() {
            take.insert(Operand::new(arg as ValueRef));
        }

        // TODO(pauls): We likely need to account for the implicit spilling that occurs when the
        // operand stack space required by function arguments exceeds K. In such cases, the W set
        // contains the function parameters up to the first parameter that would cause the operand
        // stack to overflow, all subsequent parameters are placed on the advice stack, and are assumed
        // to be moved from the advice stack to locals in the same order as they appear in the function
        // signature as part of the function prologue. Thus, the S set is preloaded with those values
        // which were spilled in this manner.
        //
        // NOTE: It should never be the case that the set of block arguments consumes more than K
        assert!(
            take.iter().map(|o| o.size()).sum::<usize>() <= K,
            "unhandled spills implied by function/block parameter list"
        );

        // The predecessors of this block are either:
        //
        // 1. Blocks in the same region with unstructured control transfer
        // 2. The parent operation and potentially other of its child regions, using structured
        //    control ops
        //
        // For the former, we just examine block operand predecessors. For the latter, we query the
        // results of dead code analysis, which will have computed the known predecessors for entry
        // blocks of region branch ops. The two do not overlap, i.e. a block will not have both
        // unstructured control predecessors and structured control predecessors.
        let mut num_predecessors = 0usize;

        // Unstructured control predecessors
        for pred in block.predecessors() {
            let is_executable = liveness.is_block_executable(pred.block);
            if !is_executable {
                continue;
            }

            num_predecessors += 1;
            let next_uses = liveness.next_uses_at(&ProgramPoint::at_end_of(pred.block)).unwrap();
            for o in self.w_exits[&pred.block].iter().copied() {
                // Do not add candidates which are not live-after the predecessor
                if next_uses.is_live(&o.value) {
                    *freq.entry(o).or_insert(0) += 1;
                    cand.insert(o);
                }
            }
        }

        // Structured control predecessors
        //
        // The predecessors of a region branch op entry block, are one of two things:
        //
        // 1. The op itself
        // 2. The ops implementing RegionBranchTerminatorOp terminating other child regions of `op`
        //
        // We treat both types of predecessors much the same way we do unstructured control
        // predecessors, but we must distinguish them in order to find the appropriate liveness
        // information.
        if op.implements::<dyn RegionBranchOpInterface>() && block.is_entry_block() {
            let pred_state =
                liveness.solver().get::<PredecessorState, _>(&ProgramPoint::at_start_of(block));
            if let Some(pred_state) = pred_state {
                let next_uses = liveness.next_uses_at(&ProgramPoint::at_start_of(block)).unwrap();
                let op_ref = op.as_operation_ref();
                for pred in pred_state.known_predecessors() {
                    num_predecessors += 1;

                    // Is `pred` the operation itself?
                    if pred == &op_ref {
                        for o in w_in.iter().copied() {
                            if next_uses.is_live(&o.value) {
                                *freq.entry(o).or_insert(0) += 1;
                                cand.insert(o);
                            }
                        }
                        continue;
                    }

                    // Otherwise, `pred` is one of the regions of the current op
                    let pred_block = pred.borrow().parent().unwrap();
                    for o in self.w_exits[&pred_block].iter().copied() {
                        if next_uses.is_live(&o.value) {
                            *freq.entry(o).or_insert(0) += 1;
                            cand.insert(o);
                        }
                    }
                }
            }
        }

        for (&v, &count) in freq.iter() {
            if count as usize == num_predecessors {
                cand.remove(&v);
                take.insert(v);
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
        let taken = take.iter().map(|o| o.size()).sum::<usize>();
        assert!(
            taken <= K,
            "implicit operand stack overflow along incoming control flow edges of {}",
            block.id()
        );

        let entry = block.body().front().as_pointer().expect("unexpected empty block");
        let entry_next_uses = liveness.next_uses_at(&ProgramPoint::before(entry)).unwrap();

        // Prefer to select candidates with the smallest next-use distance, otherwise all else being
        // equal, choose to keep smaller values on the operand stack, and spill larger values, thus
        // freeing more space when spills are needed.
        let mut cand = cand.into_vec();
        cand.sort_by(|a, b| {
            entry_next_uses
                .distance(&a.value)
                .cmp(&entry_next_uses.distance(&b.value))
                .then(a.size().cmp(&b.size()))
        });

        let mut available = K - taken;
        let mut cand = cand.into_iter();
        while available > 0 {
            if let Some(candidate) = cand.next() {
                let size = candidate.size();
                if size <= available {
                    take.insert(candidate);
                    available -= size;
                    continue;
                }
            }
            break;
        }

        take
    }

    fn compute_w_entry_loop_like_op(
        &mut self,
        region: &Region,
        header: &Block,
        liveness: &LivenessAnalysis,
    ) -> SmallSet<Operand, 4> {
        // Compute the maximum pressure in the loop-like op's regions
        let mut max_pressure = 0;
        Region::traverse_region_graph(region, |region, visited| {
            // For each unique region in the reachable region graph, compute the maximum pressure
            // in that region's block, taking the max of all regions
            if !visited.contains(&region.as_region_ref()) {
                max_pressure =
                    core::cmp::max(max_pressure, max_block_pressure(&region.entry(), liveness));
            }
            false
        });

        self.compute_w_entry_loop_impl(header, liveness, max_pressure)
    }

    fn compute_w_entry_loop(
        &mut self,
        block: &Block,
        loop_info: &Loop,
        liveness: &LivenessAnalysis,
    ) -> SmallSet<Operand, 4> {
        let max_pressure = max_loop_pressure(loop_info, liveness);
        self.compute_w_entry_loop_impl(block, liveness, max_pressure)
    }

    fn compute_w_entry_loop_impl(
        &mut self,
        block: &Block,
        liveness: &LivenessAnalysis,
        max_pressure_in_loop: usize,
    ) -> SmallSet<Operand, 4> {
        let entry = block.body().front().as_pointer().expect("unexpected empty block");
        let block_start_next_uses =
            liveness.next_uses_at(&ProgramPoint::at_start_of(block)).unwrap();

        let mut alive = block
            .arguments()
            .iter()
            .copied()
            .map(|v| Operand::new(v as ValueRef))
            .collect::<SmallSet<Operand, 4>>();
        alive.extend(block_start_next_uses.live().map(Operand::new));

        // Initial candidates are values live at block entry which are used in the loop body
        let mut cand = alive
            .iter()
            .filter(|o| block_start_next_uses.distance(&o.value) < LOOP_EXIT_DISTANCE)
            .cloned()
            .collect::<SmallSet<Operand, 4>>();

        // Values which are "live through" the loop, are those which are live at entry, but not
        // used within the body of the loop. If we have excess available operand stack capacity,
        // then we can avoid issuing spills/reloads for at least some of these values.
        let live_through = alive.difference(&cand);

        let entry_next_uses = liveness.next_uses_at(&ProgramPoint::before(entry)).unwrap();
        let w_used = cand.iter().map(|o| o.size()).sum::<usize>();
        if w_used < K {
            if let Some(mut free_in_loop) = K.checked_sub(max_pressure_in_loop) {
                let mut live_through = live_through.into_vec();
                live_through.sort_by(|a, b| {
                    entry_next_uses
                        .distance(&a.value)
                        .cmp(&entry_next_uses.distance(&b.value))
                        .then(a.size().cmp(&b.size()))
                });

                let mut live_through = live_through.into_iter();
                while free_in_loop > 0 {
                    if let Some(operand) = live_through.next() {
                        if let Some(new_free) = free_in_loop.checked_sub(operand.size()) {
                            if cand.insert(operand) {
                                free_in_loop = new_free;
                            }
                            continue;
                        }
                    }
                    break;
                }
            }

            cand
        } else {
            // We require the block parameters to be in W on entry
            let mut take = SmallSet::<_, 4>::from_iter(
                block.arguments().iter().copied().map(|v| Operand::new(v as ValueRef)),
            );

            // So remove them from the set of candidates, then sort remaining by next-use and size
            let mut cand = cand.into_vec();
            cand.retain(|o| !block.arguments().iter().any(|arg| *arg as ValueRef == o.value));
            cand.sort_by(|a, b| {
                entry_next_uses
                    .distance(&a.value)
                    .cmp(&entry_next_uses.distance(&b.value))
                    .then(a.size().cmp(&b.size()))
            });

            // Fill `take` with as many of the candidates as we can
            let mut taken = take.iter().map(|o| o.size()).sum::<usize>();
            take.extend(cand.into_iter().take_while(|operand| {
                let size = operand.size();
                let new_size = taken + size;
                if new_size <= K {
                    taken = new_size;
                    true
                } else {
                    false
                }
            }));
            take
        }
    }

    fn compute_spills_and_reloads_on_op_exit(
        &mut self,
        branch: &dyn RegionBranchOpInterface,
        pred: OperationRef,
        pred_inputs: &[ValueRef],
        w_entry: &SmallSet<Operand, 4>,
        s_entry: &SmallSet<Operand, 4>,
        w_in: &SmallSet<Operand, 4>,
        s_in: &SmallSet<Operand, 4>,
        liveness: &LivenessAnalysis,
    ) {
        let op_ref = branch.as_operation().as_operation_ref();
        let predecessor = pred.borrow();
        let pred_block = predecessor.parent().unwrap();
        let (w_exitp, s_exitp) = if pred == op_ref {
            (w_in, s_in)
        } else {
            // We must have W^exit(P) and S^exit(P) by now
            (&self.w_exits[&pred_block], &self.s_exits[&pred_block])
        };

        let mut to_reload = w_entry.difference(w_exitp);
        let mut to_spill = s_entry.difference(s_exitp).into_intersection(w_exitp);

        let next_uses = liveness.next_uses_at(&ProgramPoint::after(op_ref)).unwrap();

        let must_spill = w_exitp.difference(w_entry).into_difference(s_exitp);
        to_spill.extend(must_spill.into_iter().filter(|o| next_uses.is_live(&o.value)));

        for (i, result) in branch.results().iter().copied().enumerate() {
            let result = Operand::new(result as ValueRef);
            to_reload.remove(&result);
            // Match up this result with its source argument, and if the source value is not in
            // W^exit(P), then a reload is needed
            let src = pred_inputs.get(i).copied().expect("index out of range");
            let src = Operand::new(src);
            if !w_exitp.contains(&src) {
                to_reload.insert(src);
            }
        }

        // If there are no reloads or spills needed, we're done
        if to_reload.is_empty() && to_spill.is_empty() {
            return;
        }

        // Otherwise, we need to split the edge from P to B, and place any spills/reloads in the split,
        // S, moving any block arguments for B, to the unconditional branch in S.
        let split = self.split_regional(
            op_ref,
            RegionBranchPoint::Parent,
            if pred == op_ref {
                RegionBranchPoint::Parent
            } else {
                RegionBranchPoint::Child(predecessor.parent_region().unwrap())
            },
        );
        let place = Placement::Split(split);
        let span = predecessor.span();

        assert!(
            to_reload.is_empty(),
            "unexpected reload(s) required on edge from {pred_block} to '{}': {}",
            &branch.name(),
            DisplayValues::new(to_reload.iter().map(|o| &o.value))
        );

        // TODO: Insert spill+reload immediately before predecessor op to ensure that spilled value
        // is spilled in this predecessor. Note that this changes S^exit(P), and therefore may need
        // spills in other successors of P (if any). Ideally, we would be able to split the edge
        // from P to avoid this, but we don't (currently) have a mechanism by which to represent
        // split edges.
        assert!(
            to_spill.is_empty(),
            "unexpected spill(s) required on edge from {pred_block} to '{}': {}",
            &branch.name(),
            DisplayValues::new(to_spill.iter().map(|o| &o.value))
        );

        // Insert spills first, to end the live ranges of as many variables as possible
        for spill in to_spill {
            self.spill(place, spill.value, span);
        }

        // Then insert needed reloads
        for reload in to_reload {
            self.reload(place, reload.value, span);
        }
    }

    /// At join points in the region control flow graph, the set of live and spilled values may, and
    /// likely will, differ depending on which predecessor is taken to reach it. We must ensure that
    /// for any given predecessor:
    ///
    /// * Spills are inserted for any values expected in S upon entry to the successor program point,
    ///   which have not been spilled yet. This occurs when a value is spilled only in a subset of
    ///   predecessors, so we must spill that value in those predecessors where it has not yet been
    ///   spilled, to ensure that spills are unified in the successor.
    /// * Reloads are inserted for any values expected in W upon entry to the successor program
    ///   point, which are not in W yet. This occurs when a value is spilled in a subset of
    ///   predecessors, and hasn't been reloaded again since the spill, but is now required. We must
    ///   reload that value in those predecessors where it is spilled but not yet in W, to ensure
    ///   that the contents of W are unified on entry to the successor.
    ///
    /// NOTE: We are not actually mutating the function and inserting instructions here. Instead, we
    /// are computing what instructions need to be inserted, and where, as part of the analysis. A
    /// rewrite pass can then apply the analysis results to the function, if desired.
    ///
    /// # Differences between region-CFGs and "normal" CFGs
    ///
    /// In a normal block-oriented CFG, spills/reloads are inserted either at the end of a given
    /// predecessor block, or the edge from that predecessor is split (introducing a new block),
    /// and the spills/reloads are placed in the split. The former is done when control flow from
    /// that predecessor is unconditional, so we can freely modify the end of the block to get
    /// things set up the way we want. The latter approach is needed when control flow from the
    /// predecessor is conditional, in such cases we cannot modify the W and S sets in the
    /// predecessor directly, as that will then cause conflicts with other successors of that block.
    ///
    /// With regional control flow, where to place spills/reloads is a bit trickier. For one thing,
    /// predecessors/successors are program points, not blocks. To elaborate on what this means: all
    /// operations have _before_ and _after_ points, representing when control reaches an op, and
    /// when it leaves an op. For structured control flow operations (i.e. those whose regions form
    /// a CFG), these points represent predecessor of the ops regions (on entry), and successor of
    /// the ops regions (on exit). The _before_ point can also be a predecessor of the _after_
    /// point, for ops which only conditionally enter the regions they contain.
    ///
    /// As a result, the edges formed between program points in a region CFG cannot be split the
    /// same way we do with normal CFGs, as we cannot, for example, introduce a new block between
    /// the _before_ and _after_ points of an op, to handle spills/reloads required to unify those
    /// that occur within the regions of the op. There are a few things to note though:
    ///
    /// 1. Values defined within a region are not visible outside the region, even in region CFGs
    ///    where a given region dominates another region.
    /// 2. As a result of 1, spills/reloads of values defined in a region never need to be handled
    ///    by successors of that region.
    /// 3. However, spills/reloads of values defined _above_ a region (i.e. in the region containing
    ///    the op, or one of its ancestors), _do_ need to be handled uniformly by successors of
    ///    a region.
    /// 4. Structured control-flow operations must:
    ///    a. Unconditionally enter its body (i.e. the op entry is never a direct predecessor of the
    ///       op exit). Currently, the only known exception to this is `scf.if` without an `else`
    ///       region, in which case a falsey condition will transfer control from op entry to op
    ///       exit without entering a region. However, this specific case can be handled by
    ///       rewriting the `scf.if` to introduce an empty `else` region. The op then always enters
    ///       one of its regions, and unconditionally exits from both.
    ///    b. Either unconditionally exit from its regions, or conditionally exit from a single
    ///       region. For example, `scf.if` is a case of the former, and `scf.while` is a case of
    ///       the latter.
    fn compute_region_control_flow_edge_spills_and_reloads(
        &mut self,
        branch: &dyn RegionBranchOpInterface,
        block_info: &BlockInfo,
        pred: OperationRef,
        pred_inputs: &[ValueRef],
        w_in: &SmallSet<Operand, 4>,
        s_in: &SmallSet<Operand, 4>,
        deferred: &mut SmallVec<[BlockRef; 2]>,
        liveness: &LivenessAnalysis,
    ) {
        let op_ref = branch.as_operation().as_operation_ref();
        let predecessor = pred.borrow();
        let pred_block = predecessor.parent().unwrap();
        let (w_exitp, s_exitp) = if pred == op_ref {
            (w_in, s_in)
        } else {
            // If we don't have W^exit(P), then P hasn't been processed yet
            let Some(w_exitp) = self.w_exits.get(&pred_block) else {
                deferred.push(pred_block);
                return;
            };
            (w_exitp, &self.s_exits[&pred_block])
        };

        let mut to_reload = block_info.w_entry.difference(w_exitp);
        let mut to_spill = block_info.s_entry.difference(s_exitp).into_intersection(w_exitp);

        let block_next_uses =
            liveness.next_uses_at(&ProgramPoint::at_start_of(block_info.block_id)).unwrap();

        // We need to issue spills for any items in W^exit(P) / W^entry(B) that are not in S^exit(P),
        // but are live-after P.
        //
        // This can occur when B is a loop header, and the computed W^entry(B) does not include values
        // in W^exit(P) that are live-through the loop, typically because of loop pressure within the
        // loop requiring us to place spills of those values outside the loop.
        //
        // NOTE: It must be the case that values live-after P (i.e. live on entry to B), are values
        // in scope for B, i.e. no values that are local to P's region leak into B's region unless
        // P is an ancestor of B.
        let must_spill = w_exitp.difference(&block_info.w_entry).into_difference(s_exitp);
        to_spill.extend(must_spill.into_iter().filter(|o| block_next_uses.is_live(&o.value)));

        // We expect any block parameters present to be in `to_reload` at this point, as they will never
        // be in W^exit(P) (the parameters are not in scope at the end of P). The arguments provided in
        // the predecessor corresponding to the block parameters _must_ be in W^exit(P), and that is
        // checked here.
        //
        // Spills must not be required for region control flow, as that would indicate that the number
        // or size of the region arguments are too large/unrepresentable. A potential solution would
        // be to spill excess to memory, but that is a much more invasive transformation, so we do
        // not do that here. Instead, we simply raise an error if any of the block arguments are not
        // in W, or if those arguments exceed K.
        //
        // Spills _may_ be needed in two cases:
        //
        // 1. The current region has multiple predecessors, and a value that is in scope for all
        //    predecessors (and the current block) is spilled in only a subset of those
        //    predecessors. That will require us to insert a spill in the other predecessors to
        //    ensure it is spilled on all paths to the current block.
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
        //
        // -----
        //
        // Remove block params from `to_reload`, and replace them, as needed, with reloads of the value
        // in the predecessor which was used as the successor argument
        for (i, param) in block_info.block_id.borrow().arguments().iter().copied().enumerate() {
            let param = Operand::new(param as ValueRef);
            to_reload.remove(&param);
            // Match up this parameter with its source argument, and if the source value is not in
            // W^exit(P), then a reload is needed
            let src = pred_inputs.get(i).copied().expect("index out of range");
            let src = Operand::new(src);
            if !w_exitp.contains(&src) {
                to_reload.insert(src);
            }
        }

        // If there are no reloads or spills needed, we're done
        if to_reload.is_empty() && to_spill.is_empty() {
            return;
        }

        // Otherwise, we need to split the edge from P to B, and place any spills/reloads in the split,
        // S, moving any block arguments for B, to the unconditional branch in S.
        let split = self.split_regional(
            op_ref,
            RegionBranchPoint::Child(block_info.block_id.borrow().parent().unwrap()),
            if pred == op_ref {
                RegionBranchPoint::Parent
            } else {
                RegionBranchPoint::Child(predecessor.parent_region().unwrap())
            },
        );
        let place = Placement::Split(split);
        let span = predecessor.span();

        assert!(
            to_reload.is_empty(),
            "unexpected reload(s) required on edge from {pred_block} to {}: {}",
            &block_info.block_id,
            DisplayValues::new(to_reload.iter().map(|o| &o.value))
        );

        // TODO: Insert spill+reload immediately before predecessor op to ensure that spilled value
        // is spilled in this predecessor. Note that this changes S^exit(P), and therefore may need
        // spills in other successors of P (if any). Ideally, we would be able to split the edge
        // from P to avoid this, but we don't (currently) have a mechanism by which to represent
        // split edges.
        assert!(
            to_spill.is_empty(),
            "unexpected spill(s) required on edge from {pred_block} to {}: {}",
            &block_info.block_id,
            DisplayValues::new(to_spill.iter().map(|o| &o.value))
        );

        // Insert spills first, to end the live ranges of as many variables as possible
        for spill in to_spill {
            self.spill(place, spill.value, span);
        }

        // Then insert needed reloads
        for reload in to_reload {
            self.reload(place, reload.value, span);
        }
    }

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
        block_info: &BlockInfo,
        pred: &BlockOperand,
        deferred: &mut SmallVec<[BlockRef; 2]>,
        liveness: &LivenessAnalysis,
    ) {
        // If we don't have W^exit(P), then P hasn't been processed yet
        let Some(w_exitp) = self.w_exits.get(&pred.block) else {
            deferred.push(pred.block);
            return;
        };

        let mut to_reload = block_info.w_entry.difference(w_exitp);
        let mut to_spill = block_info
            .s_entry
            .difference(&self.s_exits[&pred.block])
            .into_intersection(w_exitp);

        let block_next_uses =
            liveness.next_uses_at(&ProgramPoint::at_start_of(block_info.block_id)).unwrap();

        // We need to issue spills for any items in W^exit(P) / W^entry(B) that are not in S^exit(P),
        // but are live-after P.
        //
        // This can occur when B is a loop header, and the computed W^entry(B) does not include values
        // in W^exit(P) that are live-through the loop, typically because of loop pressure within the
        // loop requiring us to place spills of those values outside the loop.
        let must_spill = w_exitp
            .difference(&block_info.w_entry)
            .into_difference(&self.s_exits[&pred.block]);
        to_spill.extend(must_spill.into_iter().filter(|o| block_next_uses.is_live(&o.value)));

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
        let predecesor = pred.owner.borrow();
        let branch = predecesor
            .as_trait::<dyn BranchOpInterface>()
            .expect("expected predecessor op to implement BranchOpInterface");

        let pred_args = branch.get_successor_operands(pred.index as usize);

        // Remove block params from `to_reload`, and replace them, as needed, with reloads of the value
        // in the predecessor which was used as the successor argument
        for (i, param) in block_info.block_id.borrow().arguments().iter().copied().enumerate() {
            let param = Operand::new(param as ValueRef);
            to_reload.remove(&param);
            // Match up this parameter with its source argument, and if the source value is not in
            // W^exit(P), then a reload is needed
            let src = pred_args.get(i).expect("index out of range").into_value_ref().expect(
                "internally-produced successor arguments are not yet supported by this analysis",
            );
            let src = Operand::new(src);
            if !w_exitp.contains(&src) {
                to_reload.insert(src);
            }
        }

        // If there are no reloads or spills needed, we're done
        if to_reload.is_empty() && to_spill.is_empty() {
            return;
        }

        // Otherwise, we need to split the edge from P to B, and place any spills/reloads in the split,
        // S, moving any block arguments for B, to the unconditional branch in S.
        let split = self.split_local(block_info.block_id, pred);
        let place = Placement::Split(split);
        let span = pred.owner.span();

        // Insert spills first, to end the live ranges of as many variables as possible
        for spill in to_spill {
            self.spill(place, spill.value, span);
        }

        // Then insert needed reloads
        for reload in to_reload {
            self.reload(place, reload.value, span);
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
        w: &mut SmallSet<Operand, 4>,
        s: &mut SmallSet<Operand, 4>,
        liveness: &LivenessAnalysis,
    ) {
        let ip = ProgramPoint::before(op);
        let place = Placement::At(ip);
        let span = op.span();

        // A non-branching terminator is either a return, or an unreachable.
        //
        // In the latter case, there are no operands or results, so there is no effect on W or S.
        // In the former case, the operands to the instruction are the "results" from the
        // perspective of the operand stack, so we are simply ensuring that those values are in W by
        // issuing reloads as necessary, all other values are dead, so we do not actually issue any
        // spills.
        //
        // NOTE: We only pull operands from group 0, as other groups are (currently) exclusively
        // used for successor operand groups, and we want to ignore those here.
        let operands = op
            .operands()
            .group(0)
            .iter()
            .map(|operand| operand.borrow().as_value_ref())
            .collect::<SmallVec<[_; 4]>>();
        if op.implements::<dyn Terminator>() && !op.implements::<dyn BranchOpInterface>() {
            w.retain(|o| liveness.is_live_before(o.value, op));
            let to_reload = operands.iter().copied().map(Operand::new);
            for reload in to_reload {
                if w.insert(reload) {
                    self.reload(place, reload.value, span);
                }
            }
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
        let mut to_reload =
            operands.iter().copied().map(Operand::new).collect::<SmallVec<[Operand; 4]>>();

        // Remove the first occurrance of any operand already in W, remaining uses
        // must be considered against the stack usage calculation (but will not
        // actually be reloaded)
        for operand in w.iter() {
            if let Some(pos) = to_reload.iter().position(|o| o == operand) {
                to_reload.swap_remove(pos);
            }
        }

        // Precompute the starting stack usage of W
        let w_used = w.iter().map(|o| o.size()).sum::<usize>();

        // Compute the needed operand stack space for all operands not currently
        // in W, i.e. those which must be reloaded from a spill slot
        let in_needed = to_reload.iter().map(|o| o.size()).sum::<usize>();

        // Compute the needed operand stack space for results of I
        let results = op
            .results()
            .iter()
            .map(|result| *result as ValueRef)
            .collect::<SmallVec<[_; 2]>>();
        let out_needed = results.iter().map(|v| v.borrow().ty().size_in_felts()).sum::<usize>();

        // Compute the amount of operand stack space needed for operands which are
        // not live across the instruction, i.e. which do not consume stack space
        // concurrently with the results.
        let in_consumed = operands
            .iter()
            .filter_map(|v| {
                if liveness.is_live_after(*v, op) {
                    None
                } else {
                    Some(v.borrow().ty().size_in_felts())
                }
            })
            .sum::<usize>();

        // If we have room for operands and results in W, then no spills are needed,
        // otherwise we require two passes to compute the spills we will need to issue
        let mut to_spill = SmallSet::<Operand, 4>::default();

        // First pass: compute spills for entry to I (making room for operands)
        //
        // The max usage in is determined by the size of values currently in W, plus the size
        // of any duplicate operands (i.e. values used as operands more than once), as well as
        // the size of any operands which must be reloaded.
        let max_usage_in = w_used + in_needed;
        if max_usage_in > K {
            // We must spill enough capacity to keep K >= 16
            let mut must_spill = max_usage_in - K;
            // Our initial set of candidates consists of values in W which are not operands
            // of the current instruction.
            let mut candidates = w
                .iter()
                .copied()
                .filter(|o| !operands.contains(&o.value))
                .collect::<SmallVec<[_; 16]>>();
            // We order the candidates such that those whose next-use distance is greatest, are
            // placed last, and thus will be selected first. We further break ties between
            // values with equal next-use distances by ordering them by the
            // effective size on the operand stack, so that larger values are
            // spilled first.
            candidates.sort_by(|a, b| {
                let a_dist = liveness.next_use_after(a.value, op);
                let b_dist = liveness.next_use_after(b.value, op);
                a_dist.cmp(&b_dist).then(a.size().cmp(&b.size()))
            });
            // Spill until we have made enough room
            while must_spill > 0 {
                let candidate = candidates.pop().unwrap_or_else(|| {
                    panic!(
                        "unable to spill sufficient capacity to hold all operands on stack at one \
                         time at {}",
                        op.name()
                    )
                });
                must_spill = must_spill.saturating_sub(candidate.size());
                to_spill.insert(candidate);
            }
        }

        // Second pass: compute spills for exit from I (making room for results)
        let spilled = to_spill.iter().map(|o| o.size()).sum::<usize>();
        // The max usage out is computed by adding the space required for all results of I, to
        // the max usage in, then subtracting the size of all operands which are consumed by I,
        // as well as the size of those values in W which we have spilled.
        let max_usage_out = (max_usage_in + out_needed).saturating_sub(in_consumed + spilled);
        if max_usage_out > K {
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
                    if !operands.contains(&o.value) {
                        // Not an argument, not yet spilled
                        !to_spill.contains(*o)
                    } else {
                        // A spillable argument
                        liveness.is_live_after(o.value, op)
                    }
                })
                .copied()
                .collect::<SmallVec<[_; 16]>>();
            candidates.sort_by(|a, b| {
                let a_dist = liveness.next_use_after(a.value, op);
                let b_dist = liveness.next_use_after(b.value, op);
                a_dist.cmp(&b_dist).then(a.size().cmp(&b.size()))
            });
            while must_spill > 0 {
                let candidate = candidates.pop().unwrap_or_else(|| {
                    panic!(
                        "unable to spill sufficient capacity to hold all operands on stack at one \
                         time at {}",
                        op.name()
                    )
                });
                // If we're spilling an operand of I, we can multiple the amount of space
                // freed by the spill by the number of uses of the spilled value in I
                let num_uses =
                    core::cmp::max(1, operands.iter().filter(|v| *v == &candidate.value).count());
                let freed = candidate.size() * num_uses;
                must_spill = must_spill.saturating_sub(freed);
                to_spill.insert(candidate);
            }
        }

        // Emit spills first, to make space for reloaded values on the operand stack
        for spill in to_spill.iter() {
            if s.insert(*spill) {
                self.spill(place, spill.value, span);
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
                self.reload(place, reload.value, span);
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
        if let Some(branch) = op.as_trait::<dyn BranchOpInterface>() {
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
                w.retain(|o| {
                    op.operands()
                        .group(succ.operand_group as usize)
                        .iter()
                        .any(|arg| arg.borrow().as_value_ref() == o.value)
                        || liveness.is_live_after(o.value, op)
                });
            } else {
                let successor_operand_groups = op
                    .successors()
                    .iter()
                    .filter_map(|s| {
                        let successor = s.block.borrow().block;
                        if liveness.is_block_executable(successor) {
                            Some(s.operand_group as usize)
                        } else {
                            None
                        }
                    })
                    .collect::<SmallVec<[_; 2]>>();
                w.retain(|o| {
                    let is_succ_arg = successor_operand_groups.iter().copied().any(|succ| {
                        op.operands()
                            .group(succ)
                            .iter()
                            .any(|arg| arg.borrow().as_value_ref() == o.value)
                    });
                    is_succ_arg || liveness.is_live_after(o.value, op)
                });
            }
        } else {
            // This is a simple operation
            w.retain(|o| liveness.is_live_after(o.value, op));
            w.extend(results.iter().copied().map(Operand::new));
        }
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

    while let Some(block_ref) = block_q.pop_front() {
        if !visited.insert(block_ref) {
            continue;
        }

        let block = block_ref.borrow();

        block_q.extend(BlockRef::children(block_ref).filter(|b| loop_info.contains_block(*b)));

        let block_max = max_block_pressure(&block, liveness);
        max = core::cmp::max(max, block_max);
    }

    max
}

/// Compute the maximum operand stack pressure for `block`, using `liveness`
fn max_block_pressure(block: &Block, liveness: &LivenessAnalysis) -> usize {
    let mut max_pressure = 0;

    let live_in = liveness.next_uses_at(&ProgramPoint::at_start_of(block)).unwrap();
    for v in live_in.live() {
        max_pressure += v.borrow().ty().size_in_felts();
    }

    let mut operands = SmallVec::<[ValueRef; 8]>::default();
    for op in block.body() {
        operands.clear();
        operands.extend(op.operands().all().iter().map(|v| v.borrow().as_value_ref()));

        let mut live_in_pressure = 0;
        let mut relief = 0usize;
        let live_in = liveness.next_uses_at(&ProgramPoint::before(&*op)).unwrap();
        let live_out = liveness.next_uses_at(&ProgramPoint::after(&*op)).unwrap();
        for live in live_in.live() {
            if operands.contains(&live) {
                continue;
            }
            if live_out.get(&live).is_none_or(|v| !v.is_live()) {
                continue;
            }
            live_in_pressure += live.borrow().ty().size_in_felts();
        }
        for operand in operands.iter() {
            let size = operand.borrow().ty().size_in_felts();
            if live_out.get(operand).is_none_or(|v| !v.is_live()) {
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
    }

    max_pressure
}
