use std::{
    cmp,
    collections::{BTreeMap, VecDeque},
};

use midenc_hir::{
    self as hir,
    pass::{Analysis, AnalysisManager, AnalysisResult, PreservedAnalyses},
    Block as BlockId, Inst as InstId, Value as ValueId, *,
};
use midenc_session::Session;

use super::{ControlFlowGraph, DominatorTree, LoopAnalysis};

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
struct BlockInfo {
    max_register_pressure: u16,
    max_operand_stack_pressure: u16,
}

/// This data structure is the result of computing liveness information over the
/// control flow graph of an HIR function. It uses a somewhat novel approach to
/// liveness that is ideally suited to optimal register allocation over programs
/// in SSA form, as described in _Register Spilling and Live-Range Splitting for
/// SSA-Form Programs_, by Matthias Braun and Sebastian Hack.
///
/// In short, rather than simply tracking what values are live at a particular
/// program point, we instead track the global next-use distance for all values
/// which are live at a given program point. Just like the more typical liveness
/// analysis, the next-use analysis computes next-use distances for a block by
/// taking the next-use distances at the exit of the block, and working backwards
/// to compute the next-use distances at the entry of the block. When computing
/// the next-use distances at the exit of a block with multiple successors, the
/// join function takes the minimum next-use distances of all variables live across
/// some edge.
///
/// By default, variables introduced by an instruction (i.e. their definition) are
/// assigned the next-use distance at the next instruction (or the block exit). If
/// no use of the variable has been observed, it is assigned a next-use distance of
/// infinity (here represented as `u32::MAX`). Each time we step backwards up through
/// the block, we increment the distance of all values observed so far by 1. If we
/// encounter a use of a variable at the current instruction, its next-use distance
/// is set to 0.
///
/// When we calculate the next-use distances for the exit of a block, the set is
/// initialized by taking the join of the next-use distances at the entry of all
/// successors of that block, or the empty set if there are no successors. However,
/// if the current block is inside a loop, and any successor is outside that loop,
/// then all next-use distances obtained from that successor are incremented by a
/// large constant (1000 in our case).
///
/// The algorithm follows the standard dataflow analysis approach of working until
/// a fixpoint is reached. We start by visiting the control flow graph in reverse
/// postorder, and revisit any blocks whose results change since the last time we
/// saw that block.
///
/// The resulting data structure is ideal for register allocation, but the real benefit
/// is that it allows us to be smart about how we spill values, since it can tell us
/// how "hot" a value is, allowing us to prioritize such values over those which may
/// not be used for awhile.
#[derive(Debug, Default)]
pub struct LivenessAnalysis {
    // Liveness/global next-uses at a given program point
    live_in: FxHashMap<ProgramPoint, NextUseSet>,
    // Liveness/global next-uses after a given program point
    live_out: FxHashMap<ProgramPoint, NextUseSet>,
    // Maximum pressures for each block
    per_block_info: FxHashMap<BlockId, BlockInfo>,
}
impl Analysis for LivenessAnalysis {
    type Entity = Function;

    fn analyze(
        function: &Self::Entity,
        analyses: &mut AnalysisManager,
        session: &Session,
    ) -> AnalysisResult<Self> {
        let cfg = analyses.get_or_compute(function, session)?;
        let domtree = analyses.get_or_compute(function, session)?;
        let loops = analyses.get_or_compute(function, session)?;
        Ok(LivenessAnalysis::compute(function, &cfg, &domtree, &loops))
    }

    fn is_invalidated(&self, preserved: &PreservedAnalyses) -> bool {
        !preserved.is_preserved::<ControlFlowGraph>()
            || !preserved.is_preserved::<DominatorTree>()
            || !preserved.is_preserved::<LoopAnalysis>()
    }
}
impl LivenessAnalysis {
    /// Computes liveness for the given function, using the provided analyses
    pub fn compute(
        function: &Function,
        cfg: &ControlFlowGraph,
        domtree: &DominatorTree,
        loops: &LoopAnalysis,
    ) -> Self {
        let mut liveness = Self::default();
        liveness.recompute(function, cfg, domtree, loops);
        liveness
    }

    /// Recomputes liveness for the given function, using the provided analyses
    pub fn recompute(
        &mut self,
        function: &Function,
        cfg: &ControlFlowGraph,
        domtree: &DominatorTree,
        loops: &LoopAnalysis,
    ) {
        self.clear();
        compute_liveness(self, function, cfg, domtree, loops)
    }

    /// Clear all computed liveness information, without releasing the memory we allocated
    pub fn clear(&mut self) {
        self.live_in.clear();
        self.live_out.clear();
        self.per_block_info.clear();
    }

    /// Returns true if `value` is live at the given program point.
    ///
    /// To be live "at" means that the value is live entering that point of
    /// the program, but does not imply that it is live after that point.
    pub fn is_live_at(&self, value: &ValueId, pp: ProgramPoint) -> bool {
        self.live_in[&pp].is_live(value)
    }

    /// Returns true if `value` is live after the given program point.
    ///
    /// To be live "after" means that the value is live exiting that point of
    /// the program, but does not imply that it is live before that point.
    pub fn is_live_after(&self, value: &ValueId, pp: ProgramPoint) -> bool {
        self.live_out[&pp].is_live(value)
    }

    /// Returns the next-use distance of `value` at the given program point.
    pub fn next_use(&self, value: &ValueId, pp: ProgramPoint) -> u32 {
        self.live_in[&pp].distance(value)
    }

    /// Returns the next-use distance of `value` after the given program point.
    pub fn next_use_after(&self, value: &ValueId, pp: ProgramPoint) -> u32 {
        self.live_out[&pp].distance(value)
    }

    /// Returns the global next-use distances at the given program point
    pub fn next_uses(&self, pp: ProgramPoint) -> &NextUseSet {
        &self.live_in[&pp]
    }

    /// Returns the global next-use distances at the given program point
    pub fn next_uses_after(&self, pp: ProgramPoint) -> &NextUseSet {
        &self.live_out[&pp]
    }

    /// Returns an iterator over values which are live at the given program point
    #[inline]
    pub fn live_at(&self, pp: ProgramPoint) -> impl Iterator<Item = ValueId> + '_ {
        self.live_in[&pp].live()
    }

    /// Returns an iterator over values which are live after the given program point
    #[inline]
    pub fn live_after(&self, pp: ProgramPoint) -> impl Iterator<Item = ValueId> + '_ {
        self.live_out[&pp].live()
    }

    /// Returns the maximum register pressure in the given block
    pub fn max_register_pressure(&self, block: &BlockId) -> usize {
        self.per_block_info[block].max_register_pressure as usize
    }

    /// Returns the maximum estimated operand stack pressure in the given block
    pub fn max_operand_stack_pressure(&self, block: &BlockId) -> usize {
        self.per_block_info[block].max_operand_stack_pressure as usize
    }

    /// Returns the chromatic number of the interference graph implicit in the
    /// liveness data represented here, i.e. number of colors required to color
    /// the graph such that no two adjacent nodes share the same color, i.e. the
    /// minimum number of registers needed to perform register allocation over
    /// the function without spills.
    ///
    /// In a more practical sense, this returns the maximum number of live values
    /// at any one point in the analyzed function.
    ///
    /// # Explanation
    ///
    /// Because the interference graphs of SSA-form programs are chordal graphs,
    /// and chordal graphs are "perfect", this makes certain properties of the interference
    /// graph easy to derive. In particular, the chromatic number of a perfect graph is
    /// equal to the size of its largest clique (group of nodes which form a complete graph,
    /// i.e. have edges to each other).
    pub fn chromatic_number(&self) -> usize {
        let mut max = 0;
        for (_pp, next_used) in self.live_in.iter() {
            max = cmp::max(next_used.iter().filter(|(_, d)| *d < &u32::MAX).count(), max);
        }
        max
    }
}

/// This data structure is used to maintain a mapping from variables to
/// their next-use distance at a specific program point.
///
/// If a value is not in the set, we have not observed its definition, or
/// any uses at the associated program point.
#[derive(Default, Debug, Clone, PartialEq, Eq)]
pub struct NextUseSet(BTreeMap<ValueId, u32>);
impl FromIterator<(ValueId, u32)> for NextUseSet {
    fn from_iter<T>(iter: T) -> Self
    where
        T: IntoIterator<Item = (ValueId, u32)>,
    {
        let mut set = Self::default();
        for (k, v) in iter.into_iter() {
            set.insert(k, v);
        }
        set
    }
}
impl NextUseSet {
    /// Inserts `value` in this set with the given `distance`.
    ///
    /// A distance of `u32::MAX` signifies infinite distance, which is
    /// equivalent to saying that `value` is not live.
    ///
    /// If `value` is already in this set, the distance is updated to be the
    /// lesser of the two distances, e.g. if the previous distance was `u32::MAX`,
    /// and `distance` was `1`, the entry is updated to have a distance of `1` after
    /// this function returns.
    pub fn insert(&mut self, value: ValueId, distance: u32) {
        use std::collections::btree_map::Entry;
        match self.0.entry(value) {
            Entry::Vacant(entry) => {
                entry.insert(distance);
            }
            Entry::Occupied(mut entry) => {
                let prev_distance = entry.get_mut();
                *prev_distance = std::cmp::min(*prev_distance, distance);
            }
        }
    }

    /// Returns `true` if `value` is live in this set
    #[inline]
    pub fn is_live(&self, value: &ValueId) -> bool {
        self.distance(value) < u32::MAX
    }

    /// Returns the distance to the next use of `value` as an integer.
    ///
    /// If `value` is not live, or the distance is unknown, returns `u32::MAX`
    pub fn distance(&self, value: &ValueId) -> u32 {
        self.0.get(value).copied().unwrap_or(u32::MAX)
    }

    /// Returns `true` if `value` is in this set
    #[inline]
    pub fn contains(&self, value: &ValueId) -> bool {
        self.0.contains_key(value)
    }

    /// Gets the distance associated with the given `value`, if known
    #[inline]
    pub fn get(&self, value: &ValueId) -> Option<&u32> {
        self.0.get(value)
    }

    /// Gets a mutable reference to the distance associated with the given `value`, if known
    #[inline]
    pub fn get_mut(&mut self, value: &ValueId) -> Option<&mut u32> {
        self.0.get_mut(value)
    }

    pub fn entry(
        &mut self,
        value: ValueId,
    ) -> std::collections::btree_map::Entry<'_, ValueId, u32> {
        self.0.entry(value)
    }

    /// Removes the entry for `value` from this set
    pub fn remove(&mut self, value: &ValueId) -> Option<u32> {
        self.0.remove(value)
    }

    /// Returns a new set containing the union of `self` and `other`.
    ///
    /// The resulting set will preserve the minimum distances from both sets.
    pub fn union(&self, other: &Self) -> Self {
        let mut result = self.clone();
        for (k, v) in other.iter() {
            result.insert(*k, *v);
        }
        result
    }

    /// Returns a new set containing the intersection of `self` and `other`.
    ///
    /// The resulting set will preserve the minimum distances from both sets.
    pub fn intersection(&self, other: &Self) -> Self {
        let mut result = Self::default();
        for (k, v1) in self.iter() {
            match other.get(k) {
                None => continue,
                Some(v2) => {
                    result.0.insert(*k, core::cmp::min(*v1, *v2));
                }
            }
        }
        result
    }

    /// Returns a new set containing the symmetric difference of `self` and `other`,
    /// i.e. the values that are in `self` or `other` but not in both.
    pub fn symmetric_difference(&self, other: &Self) -> Self {
        let mut result = Self::default();
        for (k, v) in self.iter() {
            if !other.0.contains_key(k) {
                result.0.insert(*k, *v);
            }
        }
        for (k, v) in other.iter() {
            if !self.0.contains_key(k) {
                result.0.insert(*k, *v);
            }
        }
        result
    }

    pub fn iter(&self) -> impl Iterator<Item = (&ValueId, &u32)> {
        self.0.iter()
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = (&ValueId, &mut u32)> {
        self.0.iter_mut()
    }

    pub fn keys(&self) -> impl Iterator<Item = ValueId> + '_ {
        self.0.keys().copied()
    }

    /// Returns an iterator over the values in this set with a finite next-use distance
    pub fn live(&self) -> impl Iterator<Item = ValueId> + '_ {
        self.0
            .iter()
            .filter_map(|(v, dist)| if *dist < u32::MAX { Some(*v) } else { None })
    }

    /// Remove the value in this set which is closest compared to the others
    ///
    /// If this set is empty, returns `None`.
    ///
    /// If more than one value have the same distance, this returns the value with
    /// the lowest id first.
    #[inline]
    pub fn pop_first(&mut self) -> Option<(ValueId, u32)> {
        self.0.pop_first()
    }

    /// Remove the value in this set which is furthest away compared to the others
    ///
    /// If this set is empty, returns `None`.
    ///
    /// If more than one value have the same distance, this returns the value with
    /// the highest id first.
    #[inline]
    pub fn pop_last(&mut self) -> Option<(ValueId, u32)> {
        self.0.pop_last()
    }
}
impl<'b> std::ops::BitOr<&'b NextUseSet> for &NextUseSet {
    type Output = NextUseSet;

    #[inline]
    fn bitor(self, rhs: &'b NextUseSet) -> Self::Output {
        self.union(rhs)
    }
}
impl<'b> std::ops::BitAnd<&'b NextUseSet> for &NextUseSet {
    type Output = NextUseSet;

    #[inline]
    fn bitand(self, rhs: &'b NextUseSet) -> Self::Output {
        self.intersection(rhs)
    }
}
impl<'b> std::ops::BitXor<&'b NextUseSet> for &NextUseSet {
    type Output = NextUseSet;

    #[inline]
    fn bitxor(self, rhs: &'b NextUseSet) -> Self::Output {
        self.symmetric_difference(rhs)
    }
}

// The distance penalty applied to an edge which exits a loop
pub const LOOP_EXIT_DISTANCE: u32 = 100_000;

/// This function computes global next-use distances/liveness until a fixpoint is reached.
///
/// The resulting data structure associates two [NextUseSet]s with every block and instruction in
/// `function`. One set provides next-use distances _at_ a given program point, the other _after_ a
/// given program point. Intuitively, these are basically what they sound like. If a value is used
/// "at" a given instruction, it's next-use distance will be 0, and if it is never used after that,
/// it won't be present in the "after" set. However, if it used again later, it's distance in the
/// "after" set will be the distance from the instruction following the current one (or the block
/// exit if the current one is a terminator).
fn compute_liveness(
    liveness: &mut LivenessAnalysis,
    function: &Function,
    cfg: &ControlFlowGraph,
    domtree: &DominatorTree,
    loops: &LoopAnalysis,
) {
    use hashbrown::hash_map::Entry;

    let mut worklist = VecDeque::from_iter(domtree.cfg_postorder().iter().copied());

    // Track the analysis sensitivity of each block
    //
    // The liveness analysis, being a dataflow analysis, is run to a fixpoint. However, the
    // analysis results for linear control flow will never change, so recomputing liveness
    // multiple times for those blocks is unnecessary. For large functions, this could add up to
    // a non-trivial amount of work.
    //
    // Instead, we track whether or not the analysis of each block is flow-sensitive, and propagate
    // that information to predecessors of that block - if a block has a successor which is
    // flow-sensitive, then it is also by definition flow-sensitive, since the inputs to its
    // liveness analysis is subject to change. We then only re-analyze flow-sensitive blocks.
    let mut flow_sensitive = BTreeMap::<Block, bool>::default();
    for block in worklist.iter().copied() {
        let terminator = function.dfg.last_inst(block).unwrap();
        match function.dfg.analyze_branch(terminator) {
            // This block terminates with either a return from the enclosing function, or
            // with a trap of some kind, e.g. unreachable. By definition, liveness analysis
            // of this block cannot be flow-sensitive, as liveness is computed bottom-up.
            BranchInfo::NotABranch => {
                flow_sensitive.insert(block, false);
            }
            BranchInfo::SingleDest(succ) => {
                // If the successor is flow-sensitive, by definition so must the predecessor.
                //
                // If the successor's sensitivity is not yet known, then that means control can
                // reach `succ` before it reaches `block`, which implies that both must be flow-
                // sensitive, as the results of liveness analysis in `block` are dependent on
                // `succ`, and vice versa.
                //
                // Blocks which are part of loops are presumed to be flow-sensitive, with the only
                // exception being exit nodes of the loop whose successors are known to be flow-
                // insensitive. This flow-insensitivity can propagate to successors of those blocks
                // so long as they are exclusively predecessors of flow-insensitive blocks.
                //
                // Putting this all together - it must be the case that we either know that `succ`
                // is flow-insensitive, or we know that it is flow-sensitive, either explicitly or
                // by implication.
                flow_sensitive
                    .insert(block, flow_sensitive.get(&succ.destination).copied().unwrap_or(true));
            }
            BranchInfo::MultiDest(succs) => {
                // Must like the single-successor case, we derive flow-sensitivity for predecessors
                // from their successors.
                //
                // The primary difference in this situation, is that the only possible way for
                // `block` to be flow-insensitive, is if all successors are explicitly flow-
                // insensitive.
                let is_flow_sensitive = succs
                    .iter()
                    .any(|succ| flow_sensitive.get(&succ.destination).copied().unwrap_or(true));
                flow_sensitive.insert(block, is_flow_sensitive);
            }
        }
    }

    // Compute liveness and global next-uses for each block, bottom-up
    //
    // For each block, liveness is determined by the next-use distance for each variable, where a
    // distance of `u32::MAX` is our representation of infinity, and infinity is the distance
    // assigned a value which is dead. All values implicitly start with an infinite distance, and
    // the distance to next use is calculated by taking the length of control flow edges (0 for
    // regular edges, LOOP_EXIT_DISTANCE for edges which leave a loop) and adding that to the
    // next-use distances of values which are live over the edge.
    //
    // The next-use distance for a value within a block, is the distance between that use and it's
    // definition in terms of the number of instructions in the block which precede it. As such,
    // we visit instructions in a block bottom-up, where at each program point, we record the
    // following next-use information:
    //
    // * Values defined by the current instruction are given the next-use distances observed thus
    // far at that instruction, e.g. if no use of a value is observed, that distance is `u32::MAX`.
    // If the value is used by the next instruction, the distance  is `1`, and so on. For all
    // preceding instructions, the value is removed from the next-use set.
    // * Values used by the current instruction are given a next-use distance of `0`
    // * All other values known at the current program point have their next-use distances
    // incremented by 1
    while let Some(block_id) = worklist.pop_front() {
        let block = &function.dfg.blocks[block_id];
        let block_loop = loops.innermost_loop(block_id);
        let mut max_register_pressure = 0;
        let mut max_operand_stack_pressure = 0;
        let mut inst_cursor = block.insts.back();
        while let Some(inst_data) = inst_cursor.get() {
            let inst = inst_data.key;
            let pp = ProgramPoint::Inst(inst);
            let branch_info = inst_data.analyze_branch(&function.dfg.value_lists);
            let mut operand_stack_pressure = 0;

            // Compute the initial next-use set of this program point
            //
            // * For terminators which are branches, next-use distances are given by the next-use
            // distances live over the control-flow graph edges. Multiple successors are handled by
            // joining the next-use sets of all edges. In both cases, block arguments of successors
            // are removed from the set, unless the successor dominates the instruction.
            // * For all other terminators (i.e. `ret`), the next-use set is empty.
            // * For regular instructions, the next-use set is initialized using the next-use set of
            // the succeeding instruction, with all distances incremented by 1, but with all values
            // defined by the successor removed from the set.
            //
            let (inst_next_uses, inst_next_uses_after) = match branch_info {
                // This is either a non-branching terminator, or a regular instruction
                BranchInfo::NotABranch => {
                    let succ_cursor = inst_cursor.peek_next();
                    if let Some(succ) = succ_cursor.get() {
                        // Start with the next-use set of the next instruction
                        let mut inst_next_uses = liveness
                            .live_in
                            .entry(ProgramPoint::Inst(succ.key))
                            .or_default()
                            .clone();

                        // Increment the next-use distance for all inherited next uses
                        for (_value, dist) in inst_next_uses.iter_mut() {
                            *dist = dist.saturating_add(1);
                        }

                        // The next uses up to this point forms the next-use set after `inst`
                        let mut inst_next_uses_after = inst_next_uses.clone();

                        // Add uses by this instruction to the live-in set, with a distance of 0
                        let args = inst_data.arguments(&function.dfg.value_lists);
                        for arg in args.iter().copied() {
                            inst_next_uses.insert(arg, 0);
                            operand_stack_pressure +=
                                function.dfg.value_type(arg).size_in_felts() as u16;
                        }

                        // Remove all instruction results from the live-in set, and ensure they have
                        // a default distance set in the live-out set
                        let mut operand_stack_pressure_out = operand_stack_pressure;
                        let results = function.dfg.inst_results(inst);
                        for result in results {
                            inst_next_uses.remove(result);
                            inst_next_uses_after.entry(*result).or_insert(u32::MAX);
                            operand_stack_pressure_out +=
                                function.dfg.value_type(*result).size_in_felts() as u16;
                        }

                        // Compute operand stack pressure on entry and exit to instruction, and
                        // take the maximum of the two as the max operand stack pressure for this
                        // instruction.
                        //
                        // On entry, pressure is applied by the union of the sets of live-in values
                        // and instruction arguments.
                        //
                        // On exit, pressure is given by taking the entry pressure, and subtracting
                        // the size of values which are not live-across the instruction, i.e. are
                        // consumed by the instruction; and adding the size of the instruction
                        // results
                        for (value, next_use_dist) in inst_next_uses.iter() {
                            let is_argument = args.contains(value);
                            let live_in = *next_use_dist != u32::MAX;
                            // Values which are not live-in are ignored (includes inst results)
                            if !live_in {
                                continue;
                            }
                            let live_after = inst_next_uses_after.is_live(value);
                            let value_size = function.dfg.value_type(*value).size_in_felts() as u16;
                            // Arguments are already accounted for, but we want to deduct the size
                            // of arguments which are not live-after the instruction from the max
                            // pressure on exit.
                            if is_argument {
                                if !live_after {
                                    operand_stack_pressure_out -= value_size;
                                }
                            } else {
                                // For live-in non-arguments, add them to the max pressure on entry
                                operand_stack_pressure += value_size;
                                // For live-in non-arguments which are also live-after, add them to
                                // the max pressure on exit
                                if live_after {
                                    operand_stack_pressure_out += value_size;
                                }
                            }
                        }
                        operand_stack_pressure =
                            cmp::max(operand_stack_pressure, operand_stack_pressure_out);

                        (inst_next_uses, inst_next_uses_after)
                    } else {
                        // This must be a non-branching terminator, i.e. the `ret` instruction
                        // Thus, the next-use set after `inst` is empty initially
                        assert!(inst_data.opcode().is_terminator());

                        // Add uses by this instruction to the live-in set, with a distance of 0
                        let mut inst_next_uses = NextUseSet::default();
                        for arg in inst_data.arguments(&function.dfg.value_lists).iter().copied() {
                            inst_next_uses.insert(arg, 0);
                            operand_stack_pressure +=
                                function.dfg.value_type(arg).size_in_felts() as u16;
                        }

                        (inst_next_uses, NextUseSet::default())
                    }
                }
                // This is a branch instruction, so get the next-use set at the entry of each
                // successor, increment the distances in those sets based on the distance of the
                // edge, and then take the join of those sets as the initial next-use set for `inst`
                BranchInfo::SingleDest(SuccessorInfo {
                    destination: succ,
                    args: succ_args,
                }) => {
                    let mut inst_next_uses = liveness
                        .live_in
                        .get(&ProgramPoint::Block(succ))
                        .cloned()
                        .unwrap_or_default();

                    // Increment the next-use distance for all inherited next uses
                    //
                    // If this block is in a loop, make sure we add the loop exit distance for
                    // edges leaving the loop
                    let use_loop_exit_distance = block_loop
                        .map(|block_loop| {
                            loops
                                .innermost_loop(succ)
                                .map(|l| block_loop != l && loops.is_child_loop(block_loop, l))
                                .unwrap_or(true)
                        })
                        .unwrap_or(false);
                    let distance = if use_loop_exit_distance {
                        LOOP_EXIT_DISTANCE
                    } else {
                        1
                    };
                    for (_value, dist) in inst_next_uses.iter_mut() {
                        *dist = dist.saturating_add(distance);
                    }

                    // The next uses up to this point forms the initial next-use set after `inst`
                    let mut inst_next_uses_after = inst_next_uses.clone();

                    // Add uses by this instruction to the live-in set, with a distance of 0
                    let args = inst_data.arguments(&function.dfg.value_lists);
                    for arg in args.iter().copied().chain(succ_args.iter().copied()) {
                        inst_next_uses.insert(arg, 0);
                        operand_stack_pressure +=
                            function.dfg.value_type(arg).size_in_felts() as u16;
                    }

                    // Remove the successor block arguments from live-in/live-out, as those values
                    // cannot be live before they are defined, and even if the destination block
                    // dominates the current block (via loop), those values must be dead at this
                    // instruction as we're providing new definitions for them.
                    for value in function.dfg.block_args(succ) {
                        // Only remove them from live-in if they are not actually used though,
                        // since, for example, a loopback branch to a loop header could pass
                        // a value that was defined via that header's block parameters.
                        if !succ_args.contains(value) && !args.contains(value) {
                            inst_next_uses.remove(value);
                        }
                        inst_next_uses_after.remove(value);
                    }

                    // Add all live-in values other than arguments to the operand stack pressure
                    for (value, next_use_dist) in inst_next_uses.iter() {
                        if args.contains(value) || succ_args.contains(value) {
                            continue;
                        }
                        if *next_use_dist != u32::MAX {
                            operand_stack_pressure +=
                                function.dfg.value_type(*value).size_in_felts() as u16;
                        }
                    }

                    (inst_next_uses, inst_next_uses_after)
                }
                // Same as above
                //
                // NOTE: We additionally assert here that all critical edges in the control flow
                // graph have been split, as we cannot proceed correctly otherwise. It is expected
                // that either no critical edges exist, or that they have been split by a prior
                // transformation.
                BranchInfo::MultiDest(succs) => {
                    let mut inst_next_uses = NextUseSet::default();
                    let mut inst_next_uses_after = NextUseSet::default();

                    // Instruction arguments are shared across all successors
                    let args = inst_data.arguments(&function.dfg.value_lists);
                    for arg in args.iter().copied() {
                        inst_next_uses.insert(arg, 0);
                        operand_stack_pressure +=
                            function.dfg.value_type(arg).size_in_felts() as u16;
                    }

                    let mut max_branch_operand_stack_pressure = operand_stack_pressure;
                    for SuccessorInfo {
                        destination,
                        args: succ_args,
                    } in succs.iter()
                    {
                        let destination = *destination;
                        // If the successor block has multiple predecessors, this is a critical
                        // edge, as by definition this instruction means the
                        // current block has multiple successors
                        assert_eq!(
                            cfg.num_predecessors(destination),
                            1,
                            "expected all critical edges of {} to have been split!",
                            destination,
                        );
                        let mut jt_next_uses = liveness
                            .live_in
                            .get(&ProgramPoint::Block(destination))
                            .cloned()
                            .unwrap_or_default();

                        // Increment the next-use distance for all inherited next uses
                        //
                        // If this block is in a loop, make sure we add the loop exit distance for
                        // edges leaving the loop
                        let use_loop_exit_distance = block_loop
                            .map(|block_loop| {
                                loops
                                    .innermost_loop(destination)
                                    .map(|l| block_loop != l && loops.is_child_loop(block_loop, l))
                                    .unwrap_or(true)
                            })
                            .unwrap_or(false);
                        let distance = if use_loop_exit_distance {
                            LOOP_EXIT_DISTANCE
                        } else {
                            1
                        };
                        for (_value, dist) in jt_next_uses.iter_mut() {
                            *dist = dist.saturating_add(distance);
                        }

                        // The next uses up to this point forms the initial next-use set after
                        // `inst`
                        let mut jt_next_uses_after = jt_next_uses.clone();

                        // Add uses by this successor's arguments to the live-in set, with a
                        // distance of 0
                        let mut jt_operand_stack_pressure = operand_stack_pressure;
                        for arg in succ_args.iter().copied() {
                            jt_next_uses.insert(arg, 0);
                            jt_operand_stack_pressure +=
                                function.dfg.value_type(arg).size_in_felts() as u16;
                        }

                        // Remove the successor block arguments from live-in/live-out, as those
                        // values cannot be live before they are defined, and even if the
                        // destination block dominates the current block (via loop), those values
                        // must be dead at this instruction as we're providing new definitions for
                        // them.
                        for value in function.dfg.block_args(destination) {
                            // Only remove them from live-in if they are not actually used though,
                            // since, for example, a loopback branch to a loop header could pass
                            // a value that was defined via that header's block parameters.
                            if !succ_args.contains(value) {
                                jt_next_uses.remove(value);
                            }
                            jt_next_uses_after.remove(value);
                        }

                        // Add non-argument live-in values to the max operand stack pressure
                        for (value, next_use_dist) in jt_next_uses.iter() {
                            if args.contains(value) || succ_args.contains(value) {
                                continue;
                            }
                            if *next_use_dist != u32::MAX {
                                jt_operand_stack_pressure +=
                                    function.dfg.value_type(*value).size_in_felts() as u16;
                            }
                        }

                        inst_next_uses = inst_next_uses.union(&jt_next_uses);
                        inst_next_uses_after = inst_next_uses_after.union(&jt_next_uses_after);
                        max_branch_operand_stack_pressure =
                            cmp::max(max_branch_operand_stack_pressure, jt_operand_stack_pressure);
                    }

                    // The max operand stack pressure for this instruction is the greatest pressure
                    // across all successors
                    operand_stack_pressure = max_branch_operand_stack_pressure;

                    (inst_next_uses, inst_next_uses_after)
                }
            };

            // The maximum register pressure for this block is the greatest number of live-in values
            // at any point within the block.
            max_register_pressure = cmp::max(
                max_register_pressure,
                u16::try_from(inst_next_uses.iter().filter(|(_, d)| *d < &u32::MAX).count())
                    .expect("too many live values"),
            );

            // The maximum operand stack pressure for this block is the greatest pressure at any
            // point within the block.
            max_operand_stack_pressure =
                cmp::max(max_operand_stack_pressure, operand_stack_pressure);

            // Record the next-use distances for this program point
            liveness.live_in.insert(pp, inst_next_uses);
            liveness.live_out.insert(pp, inst_next_uses_after);

            // Move to the instruction preceding this one
            inst_cursor.move_prev();
        }

        // Handle the block header
        let pp = ProgramPoint::Block(block_id);
        // The block header derives it's next-use distances from the live-in set of it's first
        // instruction
        let first_inst = block.insts.front().get().unwrap().key;
        let mut block_next_uses = liveness.live_in[&ProgramPoint::Inst(first_inst)].clone();
        // For each block argument, make sure a default next-use distance (u32::MAX) is set
        // if a distance is not found in the live-in set of the first instruction
        for arg in block.params.as_slice(&function.dfg.value_lists).iter().copied() {
            block_next_uses.entry(arg).or_insert(u32::MAX);
        }
        // For blocks, the "after" set corresponds to the next-use set "after" the block
        // terminator. This makes it easy to query liveness at entry and exit to a block.
        let last_inst = block.insts.back().get().unwrap().key;
        let block_next_uses_after = liveness.live_out[&ProgramPoint::Inst(last_inst)].clone();
        // Run the analysis to a fixpoint
        match liveness.live_in.entry(pp) {
            Entry::Vacant(entry) => {
                entry.insert(block_next_uses);
                liveness.live_out.insert(pp, block_next_uses_after);
                // Always revisit flow-sensitive blocks at least once
                if flow_sensitive[&block_id] {
                    worklist.push_back(block_id);
                }
            }
            Entry::Occupied(mut entry) => {
                let prev = entry.get();
                let has_changed = prev != &block_next_uses;
                entry.insert(block_next_uses);
                liveness.live_out.insert(pp, block_next_uses_after);
                // If this block has changed, make sure we revisit predecessors of this block, as
                // their liveness inputs have changed as a result.
                if !has_changed {
                    continue;
                }
                for pred in cfg.pred_iter(block_id) {
                    worklist.push_back(pred.block);
                }
            }
        }
        liveness.per_block_info.insert(
            block_id,
            BlockInfo {
                max_register_pressure,
                max_operand_stack_pressure,
            },
        );
    }
}

impl hir::Decorator for &LivenessAnalysis {
    type Display<'a>
        = DisplayLiveness<'a>
    where
        Self: 'a;

    fn decorate_block<'a, 'l: 'a>(&'l self, block: BlockId) -> Self::Display<'a> {
        DisplayLiveness {
            pp: ProgramPoint::Block(block),
            lr: self,
        }
    }

    fn decorate_inst<'a, 'l: 'a>(&'l self, inst: InstId) -> Self::Display<'a> {
        DisplayLiveness {
            pp: ProgramPoint::Inst(inst),
            lr: self,
        }
    }
}

struct NextUse<'a> {
    value: &'a ValueId,
    distance: &'a u32,
}
impl core::fmt::Display for NextUse<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        write!(f, "{}:{}", self.value, self.distance)
    }
}

#[doc(hidden)]
pub struct DisplayLiveness<'a> {
    pp: ProgramPoint,
    lr: &'a LivenessAnalysis,
}
impl core::fmt::Display for DisplayLiveness<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        let live = self
            .lr
            .next_uses(self.pp)
            .iter()
            .map(|(value, distance)| NextUse { value, distance });
        write!(f, " ; next_used=[{}]", DisplayValues::new(live),)
    }
}

#[cfg(test)]
mod tests {
    use midenc_hir::testing::TestContext;
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::Loop;

    /// In this test, we're limiting the liveness analysis to a single basic block, and checking
    /// for the following properties:
    ///
    /// * Values are only live up to and including their last use
    /// * Next-use distances are 0 for instruction arguments on entry to the instruction
    /// * Next-use distances are u32::MAX for unused instruction results on exit from the
    ///   instruction
    /// * Next-use distances are accurate for used instruction results on exit from the instruction
    /// * Instruction results are not present in the live-in set of the defining instruction
    ///
    /// The following HIR is constructed for this test:
    ///
    /// * `in=[v0:0,..]` indicates the set of live-in values and their next-use distance
    /// * `out=[v0:0,..]` indicates the set of live-out values and their next-use distance
    ///
    /// ```text,ignore
    /// (func (export #liveness) (param (ptr u8)) (result u32)
    ///   (block 0 (param v0 (ptr u8))
    ///     (let (v1 u32) (ptrtoint v0))
    ///     (let (v2 u32) (add v1 32))
    ///     (let (v3 (ptr u128)) (inttoptr v2))
    ///     (let (v4 u128) (load v3))
    ///     (let (v5 u32) (add v1 64))
    ///     (ret v5))
    /// )
    /// ```
    #[test]
    fn liveness_intra_block() -> AnalysisResult<()> {
        let context = TestContext::default();
        let id = "test::liveness".parse().unwrap();
        let mut function = Function::new(
            id,
            Signature::new(
                [AbiParam::new(Type::Ptr(Box::new(Type::U8)))],
                [AbiParam::new(Type::U32)],
            ),
        );

        let v4;
        let ret;
        {
            let mut builder = FunctionBuilder::new(&mut function);
            let entry = builder.current_block();
            let v0 = {
                let args = builder.block_params(entry);
                args[0]
            };

            // entry
            let v1 = builder.ins().ptrtoint(v0, Type::U32, SourceSpan::UNKNOWN);
            let v2 = builder.ins().add_imm_unchecked(v1, Immediate::U32(32), SourceSpan::UNKNOWN);
            let v3 =
                builder.ins().inttoptr(v2, Type::Ptr(Box::new(Type::U128)), SourceSpan::UNKNOWN);
            v4 = builder.ins().load(v3, SourceSpan::UNKNOWN);
            let v5 = builder.ins().add_imm_unchecked(v1, Immediate::U32(64), SourceSpan::UNKNOWN);
            ret = builder.ins().ret(Some(v5), SourceSpan::UNKNOWN);
        }

        let mut analyses = AnalysisManager::default();
        let liveness = analyses.get_or_compute::<LivenessAnalysis>(&function, &context.session)?;

        let block0 = Block::from_u32(0);

        // v0 should be live-in at the function entry, and since it is used by the first instruction
        // in the block, it should have a next-use distance of 0
        let v0 = Value::from_u32(0);
        assert!(liveness.is_live_at(&v0, ProgramPoint::Block(block0)));
        assert_eq!(liveness.next_use(&v0, ProgramPoint::Block(block0)), 0);
        let inst0 = Inst::from_u32(0);
        assert!(liveness.is_live_at(&v0, ProgramPoint::Inst(inst0)));
        assert_eq!(liveness.next_use(&v0, ProgramPoint::Inst(inst0)), 0);

        // The live range of v0 should end immediately after inst0
        assert!(!liveness.is_live_after(&v0, ProgramPoint::Inst(inst0)));
        assert_eq!(liveness.next_use_after(&v0, ProgramPoint::Inst(inst0)), u32::MAX);

        // v1 is the result of inst0, but should not be live-in at inst0, only live-after,
        // where its next-use distance, being the next instruction in the block, should be 1
        let v1 = Value::from_u32(1);
        assert!(!liveness.is_live_at(&v1, ProgramPoint::Inst(inst0)));
        assert_eq!(liveness.next_use(&v1, ProgramPoint::Inst(inst0)), u32::MAX);
        assert!(liveness.is_live_after(&v1, ProgramPoint::Inst(inst0)));
        assert_eq!(liveness.next_use_after(&v1, ProgramPoint::Inst(inst0)), 1);

        // v1 is also used later in the block, so ensure that the next-use distance after
        // inst1 reflects that usage
        let inst1 = Inst::from_u32(1);
        assert!(liveness.is_live_at(&v1, ProgramPoint::Inst(inst1)));
        assert_eq!(liveness.next_use(&v1, ProgramPoint::Inst(inst1)), 0);
        assert!(liveness.is_live_after(&v1, ProgramPoint::Inst(inst1)));
        assert_eq!(liveness.next_use_after(&v1, ProgramPoint::Inst(inst1)), 3);

        // v4 is never used after inst3, its defining instruction, ensure that the
        // liveness analysis reflects that
        let inst3 = Inst::from_u32(3);
        assert!(!liveness.is_live_after(&v4, ProgramPoint::Inst(inst3)));
        assert_eq!(liveness.next_use_after(&v4, ProgramPoint::Inst(inst3)), u32::MAX);
        // It should obviously not appear live-in at inst4
        let inst4 = Inst::from_u32(4);
        assert!(!liveness.is_live_at(&v4, ProgramPoint::Inst(inst4)));
        assert_eq!(liveness.next_use(&v4, ProgramPoint::Inst(inst4)), u32::MAX);

        // Because this block terminates with a return from the function, the only value
        // live-in at the return should be the returned value, and the block should have
        // an empty live-after set
        let v5 = Value::from_u32(5);
        assert!(liveness.is_live_at(&v5, ProgramPoint::Inst(ret)));
        assert_eq!(liveness.next_use(&v5, ProgramPoint::Inst(ret)), 0);
        assert!(!liveness.is_live_after(&v5, ProgramPoint::Inst(ret)));
        assert_eq!(liveness.next_use_after(&v5, ProgramPoint::Inst(ret)), u32::MAX);
        assert!(!liveness.is_live_after(&v5, ProgramPoint::Block(block0)));
        assert_eq!(liveness.next_use_after(&v5, ProgramPoint::Block(block0)), u32::MAX);

        Ok(())
    }

    /// In this test, we're extending the liveness analysis beyond a single basic block, to a set
    /// of four blocks that represent a very common form of control flow: conditional branches,
    /// specifically a typical if/else scenario. Here, we are concerned with the following
    /// properties of liveness analysis:
    ///
    /// * Propagating liveness up the dominator tree works correctly
    /// * Liveness of block arguments is not propagated into predecessor blocks
    /// * When unioning liveness across successors of a conditional branch, the minimum next-use
    ///   distance always wins, i.e. if a value is live on one path, it is always live in the
    ///   predecessor
    ///
    /// The following HIR is constructed for this test:
    ///
    /// * `in=[v0:0,..]` indicates the set of live-in values and their next-use distance
    /// * `out=[v0:0,..]` indicates the set of live-out values and their next-use distance
    ///
    /// ```text,ignore
    /// (func (export #liveness) (param (ptr u8)) (result u32)
    ///   (block 0 (param v0 (ptr u8))
    ///     (let (v1 u32) (ptrtoint v0))
    ///     (let (v2 u32) (add v1 32))
    ///     (let (v3 (ptr u128)) (inttoptr v2))
    ///     (let (v4 u128) (load v3))
    ///     (let (v5 u32) (add v1 64)) ;; v5 unused in this block, but used later; v1 used again later
    ///     (let (v6 i1) (eq v5 128))
    ///     (cond_br v6 (block 1) (block 2)))
    ///
    ///   (block 1 ; in this block, v4 is used first, v5 second
    ///     (let (v7 u128) (const.u128 1))
    ///     (let (v8 u128) (add v4 v7)) ;; unused
    ///     (let (v9 u32) (add v5 8))
    ///     (br (block 3 v9)))
    ///
    ///   (block 2 ; in this block, v5 is used first, v4 second
    ///     (let (v10 u128) (const.u128 2))
    ///     (let (v11 u32) (add v5 16))
    ///     (let (v12 u128) (add v4 v10)) ;; unused
    ///     (br (block 3 v11)))
    ///
    ///   (block 3 (param v13 u32)
    ///     (let (v14 u32) (add v1 v13))
    ///     (ret v14))
    /// )
    /// ```
    #[test]
    fn liveness_conditional_control_flow() -> AnalysisResult<()> {
        let context = TestContext::default();
        let id = "test::liveness".parse().unwrap();
        let mut function = Function::new(
            id,
            Signature::new(
                [AbiParam::new(Type::Ptr(Box::new(Type::U8)))],
                [AbiParam::new(Type::U32)],
            ),
        );

        let v4;
        let v5;
        let br0;
        let br1;
        let br2;
        {
            let mut builder = FunctionBuilder::new(&mut function);
            let entry = builder.current_block();
            let v0 = {
                let args = builder.block_params(entry);
                args[0]
            };

            let block1 = builder.create_block();
            let block2 = builder.create_block();
            let block3 = builder.create_block();

            // entry
            let v1 = builder.ins().ptrtoint(v0, Type::U32, SourceSpan::UNKNOWN);
            let v2 = builder.ins().add_imm_unchecked(v1, Immediate::U32(32), SourceSpan::UNKNOWN);
            let v3 =
                builder.ins().inttoptr(v2, Type::Ptr(Box::new(Type::U128)), SourceSpan::UNKNOWN);
            v4 = builder.ins().load(v3, SourceSpan::UNKNOWN);
            v5 = builder.ins().add_imm_unchecked(v1, Immediate::U32(64), SourceSpan::UNKNOWN);
            let v6 = builder.ins().eq_imm(v5, Immediate::U32(128), SourceSpan::UNKNOWN);
            br0 = builder.ins().cond_br(v6, block1, &[], block2, &[], SourceSpan::UNKNOWN);

            builder.switch_to_block(block1);
            let v7 = builder.ins().u128(1, SourceSpan::UNKNOWN);
            let _v8 = builder.ins().add_unchecked(v4, v7, SourceSpan::UNKNOWN);
            let v9 = builder.ins().add_imm_unchecked(v5, Immediate::U32(8), SourceSpan::UNKNOWN);
            br1 = builder.ins().br(block3, &[v9], SourceSpan::UNKNOWN);

            builder.switch_to_block(block2);
            let v10 = builder.ins().u128(2, SourceSpan::UNKNOWN);
            let v11 = builder.ins().add_imm_unchecked(v5, Immediate::U32(16), SourceSpan::UNKNOWN);
            let _v12 = builder.ins().add_unchecked(v4, v10, SourceSpan::UNKNOWN);
            br2 = builder.ins().br(block3, &[v11], SourceSpan::UNKNOWN);

            let v13 = builder.append_block_param(block3, Type::U32, SourceSpan::UNKNOWN);
            builder.switch_to_block(block3);
            let v14 = builder.ins().add_unchecked(v1, v13, SourceSpan::UNKNOWN);
            builder.ins().ret(Some(v14), SourceSpan::UNKNOWN);
        }

        let mut analyses = AnalysisManager::default();
        let liveness = analyses.get_or_compute::<LivenessAnalysis>(&function, &context.session)?;

        let block0 = Block::from_u32(0);
        let block1 = Block::from_u32(1);
        let block2 = Block::from_u32(2);
        let block3 = Block::from_u32(3);

        // We expect v13 to be live-in at block3, with a next-use distance of 0
        let v13 = Value::from_u32(13);
        assert!(liveness.is_live_at(&v13, ProgramPoint::Block(block3)));
        assert_eq!(liveness.next_use(&v13, ProgramPoint::Block(block3)), 0);

        // However, it should _not_ be live-after block1 or block2, nor their terminators
        assert!(!liveness.is_live_after(&v13, ProgramPoint::Block(block1)));
        assert!(!liveness.is_live_after(&v13, ProgramPoint::Inst(br1)));
        assert!(!liveness.is_live_after(&v13, ProgramPoint::Block(block2)));
        assert!(!liveness.is_live_after(&v13, ProgramPoint::Inst(br2)));
        // Also, definitely shouldn't be live-in at the terminators either
        assert!(!liveness.is_live_at(&v13, ProgramPoint::Inst(br1)));
        assert!(!liveness.is_live_at(&v13, ProgramPoint::Inst(br2)));

        // v1 should be live-in at block3 with a next-use distance of zero
        let v1 = Value::from_u32(1);
        assert!(liveness.is_live_at(&v1, ProgramPoint::Block(block3)));
        assert_eq!(liveness.next_use(&v1, ProgramPoint::Block(block3)), 0);

        // Both block1 and block2 should see v1 in their live-after set with a next-use distance of
        // 1
        assert!(liveness.is_live_after(&v1, ProgramPoint::Block(block1)));
        assert_eq!(liveness.next_use_after(&v1, ProgramPoint::Block(block1)), 1);
        assert!(liveness.is_live_after(&v1, ProgramPoint::Inst(br1)));
        assert_eq!(liveness.next_use_after(&v1, ProgramPoint::Inst(br1)), 1);
        assert!(liveness.is_live_after(&v1, ProgramPoint::Block(block2)));
        assert_eq!(liveness.next_use_after(&v1, ProgramPoint::Block(block2)), 1);
        assert!(liveness.is_live_after(&v1, ProgramPoint::Inst(br2)));
        assert_eq!(liveness.next_use_after(&v1, ProgramPoint::Inst(br2)), 1);

        // Both block1 and block2 should see v1 in their live-in set with a next-use distance of 4
        assert!(liveness.is_live_at(&v1, ProgramPoint::Block(block1)));
        assert_eq!(liveness.next_use(&v1, ProgramPoint::Block(block1)), 4);
        assert!(liveness.is_live_at(&v1, ProgramPoint::Block(block2)));
        assert_eq!(liveness.next_use(&v1, ProgramPoint::Block(block2)), 4);

        // The entry block should see v1, v4 and v5 in the live-after set with a next-use distance
        // of 5 for v1, 2 for v4 and v5 (the min distance from the union of both block1 and block2)
        assert!(liveness.is_live_after(&v1, ProgramPoint::Block(block0)));
        assert!(liveness.is_live_after(&v4, ProgramPoint::Block(block0)));
        assert!(liveness.is_live_after(&v5, ProgramPoint::Block(block0)));
        assert!(liveness.is_live_after(&v1, ProgramPoint::Inst(br0)));
        assert!(liveness.is_live_after(&v4, ProgramPoint::Inst(br0)));
        assert!(liveness.is_live_after(&v5, ProgramPoint::Inst(br0)));
        assert!(liveness.is_live_at(&v1, ProgramPoint::Inst(br0)));
        assert!(liveness.is_live_at(&v4, ProgramPoint::Inst(br0)));
        assert!(liveness.is_live_at(&v5, ProgramPoint::Inst(br0)));
        assert_eq!(liveness.next_use_after(&v1, ProgramPoint::Block(block0)), 5);
        assert_eq!(liveness.next_use_after(&v4, ProgramPoint::Block(block0)), 2);
        assert_eq!(liveness.next_use_after(&v5, ProgramPoint::Block(block0)), 2);
        assert_eq!(liveness.next_use_after(&v1, ProgramPoint::Inst(br0)), 5);
        assert_eq!(liveness.next_use_after(&v4, ProgramPoint::Inst(br0)), 2);
        assert_eq!(liveness.next_use_after(&v5, ProgramPoint::Inst(br0)), 2);
        assert_eq!(liveness.next_use(&v1, ProgramPoint::Inst(br0)), 5);
        assert_eq!(liveness.next_use(&v4, ProgramPoint::Inst(br0)), 2);
        assert_eq!(liveness.next_use(&v5, ProgramPoint::Inst(br0)), 2);

        // Ensure that the next-use distance for v1 and v5 is correct at the defining inst
        let v5_inst = function.dfg.value_data(v5).unwrap_inst();
        assert!(liveness.is_live_at(&v1, ProgramPoint::Inst(v5_inst)));
        assert!(!liveness.is_live_at(&v5, ProgramPoint::Inst(v5_inst)));
        assert!(liveness.is_live_after(&v1, ProgramPoint::Inst(v5_inst)));
        assert!(liveness.is_live_after(&v5, ProgramPoint::Inst(v5_inst)));
        assert_eq!(liveness.next_use(&v1, ProgramPoint::Inst(v5_inst)), 0);
        assert_eq!(liveness.next_use(&v5, ProgramPoint::Inst(v5_inst)), u32::MAX);
        assert_eq!(liveness.next_use_after(&v1, ProgramPoint::Inst(v5_inst)), 7);
        assert_eq!(liveness.next_use_after(&v5, ProgramPoint::Inst(v5_inst)), 1);

        Ok(())
    }

    /// In this test, we're verifying the behavior of liveness analysis when cycles in the control
    /// flow graph are present, i.e. loops. Cycles aren't always representative of loops, but we're
    /// primarily concerned with the following properties:
    ///
    /// * Values which are live across a loop have their liveness correctly propagated
    /// * Values live-across a loop should have next-use distances reflecting the loop distance when
    ///   their next-use is outside the loop
    /// * Values in the loop header have the correct liveness information, even when used as
    ///   arguments on a loopback edge to that same header
    ///
    /// The following HIR is constructed for this test:
    ///
    /// * `in=[v0:0,..]` indicates the set of live-in values and their next-use distance
    /// * `out=[v0:0,..]` indicates the set of live-out values and their next-use distance
    ///
    /// ```text,ignore
    /// (func (export #liveness) (param (ptr u8)) (result u32)
    ///   (block 0 (param v0 (ptr u8))
    ///     (let (v1 u32) (ptrtoint v0))
    ///     (let (v2 u32) (add v1 32))
    ///     (let (v3 (ptr u128)) (inttoptr v2))
    ///     (let (v4 u128) (load v3))
    ///     (let (v5 u32) (add v1 64)) ;; v5 unused in this block, but used later; v1 used again later
    ///     (let (v6 i1) (eq v5 128))
    ///     (cond_br v6 (block 1) (block 5)))
    ///
    ///   (block 1 ; split edge
    ///     ; the natural way of expressing this loop would be to have block0 branch to the loop
    ///     ; header, but that results in a critical edge between block0 and the loop header, which
    ///     ; we are compelled to split. this block splits the edge
    ///     (br (block 2 v4 v5)))
    ///
    ///   (block 2 (param v7 u128) (param v8 u32); loop header+body
    ///     (let (v9 u128) (const.u128 1))
    ///     (let (v10 u128) (add v7 v9)) ;; unused
    ///     (let (v11 u32) (add v8 8))
    ///     (let (v12 i1) (eq v11 128))
    ///     (cond_br v12 (block 3) (block 4)))
    ///
    ///   (block 3 ; split edge
    ///     ; the conditional branch at the end of block2, if routed directly to itself, introduces
    ///     ; a critical edge from block2 to itself. We split the edge using this block
    ///     (br (block 2 v7 v11)))
    ///
    ///   (block 4 ; split edge
    ///     (br (block 6 v11)))
    ///
    ///   (block 5 ; in this block, v5 is used first, v4 second
    ///     (let (v13 u128) (const.u128 2))
    ///     (let (v14 u32) (add v5 16))
    ///     (let (v15 u128) (add v4 v13)) ;; unused
    ///     (br (block 6 v14)))
    ///
    ///   (block 6 (param v16 u32)
    ///     (let (v17 u32) (add v1 v16))
    ///     (ret v17))
    /// )
    /// ```
    #[test]
    fn liveness_loop_simple() -> AnalysisResult<()> {
        let context = TestContext::default();
        let id = "test::liveness".parse().unwrap();
        let mut function = Function::new(
            id,
            Signature::new(
                [AbiParam::new(Type::Ptr(Box::new(Type::U8)))],
                [AbiParam::new(Type::U32)],
            ),
        );

        let v4;
        let v5;
        let br0;
        {
            let mut builder = FunctionBuilder::new(&mut function);
            let entry = builder.current_block();
            let v0 = {
                let args = builder.block_params(entry);
                args[0]
            };

            let block1 = builder.create_block();
            let block2 = builder.create_block();
            let block3 = builder.create_block();
            let block4 = builder.create_block();
            let block5 = builder.create_block();
            let block6 = builder.create_block();

            // entry
            let v1 = builder.ins().ptrtoint(v0, Type::U32, SourceSpan::UNKNOWN);
            let v2 = builder.ins().add_imm_unchecked(v1, Immediate::U32(32), SourceSpan::UNKNOWN);
            let v3 =
                builder.ins().inttoptr(v2, Type::Ptr(Box::new(Type::U128)), SourceSpan::UNKNOWN);
            v4 = builder.ins().load(v3, SourceSpan::UNKNOWN);
            v5 = builder.ins().add_imm_unchecked(v1, Immediate::U32(64), SourceSpan::UNKNOWN);
            let v6 = builder.ins().eq_imm(v5, Immediate::U32(128), SourceSpan::UNKNOWN);
            br0 = builder.ins().cond_br(v6, block1, &[], block5, &[], SourceSpan::UNKNOWN);

            // block1 - split edge (loop path)
            builder.switch_to_block(block1);
            builder.ins().br(block2, &[v4, v5], SourceSpan::UNKNOWN);

            // block2 - loop header+body
            let v7 = builder.append_block_param(block2, Type::U128, SourceSpan::UNKNOWN);
            let v8 = builder.append_block_param(block2, Type::U32, SourceSpan::UNKNOWN);
            builder.switch_to_block(block2);
            let v9 = builder.ins().u128(1, SourceSpan::UNKNOWN);
            let _v10 = builder.ins().add_unchecked(v7, v9, SourceSpan::UNKNOWN);
            let v11 = builder.ins().add_imm_unchecked(v8, Immediate::U32(8), SourceSpan::UNKNOWN);
            let v12 = builder.ins().eq_imm(v11, Immediate::U32(128), SourceSpan::UNKNOWN);
            builder.ins().cond_br(v12, block3, &[], block4, &[v11], SourceSpan::UNKNOWN);

            // block3 - split edge
            builder.switch_to_block(block3);
            builder.ins().br(block2, &[v7, v11], SourceSpan::UNKNOWN);

            // block4 - split edge
            builder.switch_to_block(block4);
            builder.ins().br(block6, &[v11], SourceSpan::UNKNOWN);

            // block5 - non-loop path
            builder.switch_to_block(block5);
            let v13 = builder.ins().u128(2, SourceSpan::UNKNOWN);
            let v14 = builder.ins().add_imm_unchecked(v5, Immediate::U32(16), SourceSpan::UNKNOWN);
            let _v15 = builder.ins().add_unchecked(v4, v13, SourceSpan::UNKNOWN);
            builder.ins().br(block6, &[v14], SourceSpan::UNKNOWN);

            // block6 - join point
            let v16 = builder.append_block_param(block6, Type::U32, SourceSpan::UNKNOWN);
            builder.switch_to_block(block6);
            let v17 = builder.ins().add_unchecked(v1, v16, SourceSpan::UNKNOWN);
            builder.ins().ret(Some(v17), SourceSpan::UNKNOWN);
        }

        let mut analyses = AnalysisManager::default();
        let liveness = analyses.get_or_compute::<LivenessAnalysis>(&function, &context.session)?;

        let block0 = Block::from_u32(0);
        let block1 = Block::from_u32(1);
        let block2 = Block::from_u32(2);
        let block3 = Block::from_u32(3);
        let block4 = Block::from_u32(4);
        let block5 = Block::from_u32(5);

        // * v1 should be live-after block0 with normal distance (min distance)
        let v1 = Value::from_u32(1);
        assert!(liveness.is_live_after(&v1, ProgramPoint::Block(block0)));
        assert_eq!(liveness.next_use_after(&v1, ProgramPoint::Block(block0)), 5);
        // * v1 should be live-in and live-after block1 with loop distance, as it is not used in the
        // loop that it is the header of
        assert!(liveness.is_live_at(&v1, ProgramPoint::Block(block1)));
        assert!(liveness.is_live_after(&v1, ProgramPoint::Block(block1)));
        assert_eq!(liveness.next_use(&v1, ProgramPoint::Block(block1)), 6 + LOOP_EXIT_DISTANCE);
        assert_eq!(
            liveness.next_use_after(&v1, ProgramPoint::Block(block1)),
            6 + LOOP_EXIT_DISTANCE
        );
        // * v1 should be live-in and live-after block2 with loop distance, as it is not used in the
        // loop that it is the body of
        assert!(liveness.is_live_at(&v1, ProgramPoint::Block(block2)));
        assert!(liveness.is_live_after(&v1, ProgramPoint::Block(block2)));
        assert_eq!(liveness.next_use(&v1, ProgramPoint::Block(block2)), 5 + LOOP_EXIT_DISTANCE);
        assert_eq!(
            liveness.next_use_after(&v1, ProgramPoint::Block(block2)),
            1 + LOOP_EXIT_DISTANCE
        );
        // * v1 should be live-in and live-after block3 with loop distance + distance through block2
        // as the next use requires another full iteration of the loop to reach the exit
        assert!(liveness.is_live_at(&v1, ProgramPoint::Block(block3)));
        assert!(liveness.is_live_after(&v1, ProgramPoint::Block(block3)));
        assert_eq!(liveness.next_use(&v1, ProgramPoint::Block(block3)), 6 + LOOP_EXIT_DISTANCE);
        assert_eq!(
            liveness.next_use_after(&v1, ProgramPoint::Block(block3)),
            6 + LOOP_EXIT_DISTANCE
        );
        // * v1 should be live-in and live-after block4 with normal distance, as we have exited
        // the loop along this edge
        assert!(liveness.is_live_at(&v1, ProgramPoint::Block(block4)));
        assert!(liveness.is_live_after(&v1, ProgramPoint::Block(block4)));
        assert_eq!(liveness.next_use(&v1, ProgramPoint::Block(block4)), 1);
        assert_eq!(liveness.next_use_after(&v1, ProgramPoint::Block(block4)), 1);
        // * v1 should be live-in and live-after block5 with normal distance
        assert!(liveness.is_live_at(&v1, ProgramPoint::Block(block5)));
        assert!(liveness.is_live_after(&v1, ProgramPoint::Block(block5)));
        assert_eq!(liveness.next_use(&v1, ProgramPoint::Block(block5)), 4);
        assert_eq!(liveness.next_use_after(&v1, ProgramPoint::Block(block5)), 1);

        // v4 and v5 should have next-use distances at end of block0 of 1 (min distance
        // across all successors of block0),
        assert!(liveness.is_live_at(&v4, ProgramPoint::Inst(br0)));
        assert!(liveness.is_live_after(&v4, ProgramPoint::Inst(br0)));
        assert!(liveness.is_live_at(&v5, ProgramPoint::Inst(br0)));
        assert!(liveness.is_live_after(&v5, ProgramPoint::Inst(br0)));
        assert_eq!(liveness.next_use(&v4, ProgramPoint::Inst(br0)), 1);
        assert_eq!(liveness.next_use_after(&v4, ProgramPoint::Inst(br0)), 1);
        assert_eq!(liveness.next_use(&v5, ProgramPoint::Inst(br0)), 1);
        assert_eq!(liveness.next_use_after(&v5, ProgramPoint::Inst(br0)), 1);

        // v7 and v8 should not have liveness information at end of block1
        let v7 = Value::from_u32(7);
        let v8 = Value::from_u32(8);
        let block1_term = function.dfg.last_inst(block1).unwrap();
        assert!(!liveness.is_live_after(&v7, ProgramPoint::Block(block1)));
        assert!(!liveness.is_live_after(&v8, ProgramPoint::Block(block1)));
        assert!(!liveness.is_live_after(&v7, ProgramPoint::Inst(block1_term)));
        assert!(!liveness.is_live_after(&v8, ProgramPoint::Inst(block1_term)));
        assert!(!liveness.is_live_at(&v7, ProgramPoint::Inst(block1_term)));
        assert!(!liveness.is_live_at(&v8, ProgramPoint::Inst(block1_term)));
        assert_eq!(liveness.next_use_after(&v7, ProgramPoint::Block(block1)), u32::MAX);
        assert_eq!(liveness.next_use_after(&v8, ProgramPoint::Block(block1)), u32::MAX);
        assert_eq!(liveness.next_use(&v7, ProgramPoint::Inst(block1_term)), u32::MAX);
        assert_eq!(liveness.next_use(&v8, ProgramPoint::Inst(block1_term)), u32::MAX);

        // v7 should be live at end of block2 with next-use distance of 1
        let block2_term = function.dfg.last_inst(block2).unwrap();
        assert!(liveness.is_live_at(&v7, ProgramPoint::Inst(block2_term)));
        assert!(liveness.is_live_after(&v7, ProgramPoint::Inst(block2_term)));
        assert!(liveness.is_live_after(&v7, ProgramPoint::Block(block2)));
        assert_eq!(liveness.next_use(&v7, ProgramPoint::Inst(block2_term)), 1);
        assert_eq!(liveness.next_use_after(&v7, ProgramPoint::Inst(block2_term)), 1);
        assert_eq!(liveness.next_use_after(&v7, ProgramPoint::Block(block2)), 1);

        // v8 should _not_ be live at end of block2
        assert!(!liveness.is_live_at(&v8, ProgramPoint::Inst(block2_term)));
        assert_eq!(liveness.next_use(&v8, ProgramPoint::Inst(block2_term)), u32::MAX);
        assert!(!liveness.is_live_after(&v8, ProgramPoint::Block(block2)));
        assert_eq!(liveness.next_use_after(&v8, ProgramPoint::Block(block2)), u32::MAX);

        // v7 should be live-in at end of block3 with next-use distance of 0
        let block3_term = function.dfg.last_inst(block3).unwrap();
        assert!(liveness.is_live_at(&v7, ProgramPoint::Inst(block3_term)));
        assert_eq!(liveness.next_use(&v7, ProgramPoint::Inst(block3_term)), 0);

        // v8 should not be live at entry of block3
        assert!(!liveness.is_live_at(&v8, ProgramPoint::Block(block3)));
        assert_eq!(liveness.next_use(&v8, ProgramPoint::Block(block3)), u32::MAX);

        // neither v7 nor v8 should be live after end of block3
        assert!(!liveness.is_live_after(&v7, ProgramPoint::Block(block3)));
        assert_eq!(liveness.next_use_after(&v7, ProgramPoint::Block(block3)), u32::MAX);
        assert!(!liveness.is_live_after(&v8, ProgramPoint::Block(block3)));
        assert_eq!(liveness.next_use_after(&v8, ProgramPoint::Block(block3)), u32::MAX);

        // next-use distance of v7 and v8 at entry to block2 should be accurate
        assert!(liveness.is_live_at(&v7, ProgramPoint::Block(block2)));
        assert_eq!(liveness.next_use(&v7, ProgramPoint::Block(block2)), 1);
        assert!(liveness.is_live_at(&v8, ProgramPoint::Block(block2)));
        assert_eq!(liveness.next_use(&v8, ProgramPoint::Block(block2)), 2);

        // v9 should be dead after last use in block2
        let v9 = Value::from_u32(9);
        let v10_inst = function.dfg.value_data(Value::from_u32(10)).unwrap_inst();
        assert!(liveness.is_live_at(&v9, ProgramPoint::Inst(v10_inst)));
        assert_eq!(liveness.next_use(&v9, ProgramPoint::Inst(v10_inst)), 0);
        assert!(!liveness.is_live_after(&v9, ProgramPoint::Inst(v10_inst)));
        assert_eq!(liveness.next_use_after(&v9, ProgramPoint::Inst(v10_inst)), u32::MAX);

        Ok(())
    }

    /// In this test, we're validating that all the properties of previous tests hold even when we
    /// increase the loop depth. Additionally, we expect to see the following:
    ///
    /// * Values which are live across the outer loop, reflect the cumulative distance of the loop
    ///   nest, not just the outer loop.
    /// * Values which are live across the inner loop, reflect the loop distance
    /// * Both inner and outer loop header parameters have their live ranges end when the
    ///   corresponding loop is continued or exited
    ///
    /// The following HIR is constructed for this test:
    ///
    /// * `in=[v0:0,..]` indicates the set of live-in values and their next-use distance
    /// * `out=[v0:0,..]` indicates the set of live-out values and their next-use distance
    ///
    /// ```text,ignore
    /// (func (export #liveness) (param (ptr u64)) (param u32) (param u32) (result u64)
    ///   (block 0 (param v0 (ptr u64)) (param v1 u32) (param v2 u32)
    ///     (let (v3 u32) (const.u32 0))
    ///     (let (v4 u32) (const.u32 0))
    ///     (let (v5 u64) (const.u64 0))
    ///     (br (block 1 v3 v4 v5)))
    ///
    ///   (block 1 (param v6 u32) (param v7 u32) (param v8 u64)) ; outer loop
    ///     (let (v9 i1) (eq v6 v1))
    ///     (cond_br v9 (block 2) (block 3)))
    ///
    ///   (block 2 ; exit outer loop, return from function
    ///     (ret v8))
    ///
    ///   (block 3 ; split edge
    ///     (br (block 4 v7 v8)))
    ///
    ///   (block 4 (param v10 u32) (param v11 u64) ; inner loop
    ///     (let (v12 i1) (eq v10 v2))
    ///     (cond_br v12 (block 5) (block 6)))
    ///
    ///   (block 5 ; increment row count, continue outer loop
    ///     (let (v13 u32) (add v6 1))
    ///     (br (block 1 v13 v10 v11)))
    ///
    ///   (block 6 ; load value at v0[row][col], add to sum, increment col, continue inner loop
    ///     (let (v14 u32) (sub.saturating v6 1)) ; row_offset := ROW_SIZE * row.saturating_sub(1)
    ///     (let (v15 u32) (mul v14 v2))
    ///     (let (v16 u32) (add v10 v15))         ; offset := col + row_offset
    ///     (let (v17 u32) (ptrtoint v0))         ; ptr := (v0 as u32 + offset) as *u64
    ///     (let (v18 u32) (add v17 v16))
    ///     (let (v19 (ptr u64)) (ptrtoint v18))
    ///     (let (v20 u64) (load v19))            ; sum += *ptr
    ///     (let (v21 u64) (add v11 v20))
    ///     (let (v22 u32) (add v10 1))           ; col++
    ///     (br (block 4 v22 v21)))
    /// )
    /// ```
    #[test]
    fn liveness_loop_nest() -> AnalysisResult<()> {
        let context = TestContext::default();
        let id = "test::liveness".parse().unwrap();
        let mut function = Function::new(
            id,
            Signature::new(
                [
                    AbiParam::new(Type::Ptr(Box::new(Type::U8))),
                    AbiParam::new(Type::U32),
                    AbiParam::new(Type::U32),
                ],
                [AbiParam::new(Type::U32)],
            ),
        );

        {
            let mut builder = FunctionBuilder::new(&mut function);
            let entry = builder.current_block();
            let (v0, v1, v2) = {
                let args = builder.block_params(entry);
                (args[0], args[1], args[2])
            };

            let block1 = builder.create_block();
            let block2 = builder.create_block();
            let block3 = builder.create_block();
            let block4 = builder.create_block();
            let block5 = builder.create_block();
            let block6 = builder.create_block();

            // entry
            let v3 = builder.ins().u32(0, SourceSpan::UNKNOWN);
            let v4 = builder.ins().u32(0, SourceSpan::UNKNOWN);
            let v5 = builder.ins().u64(0, SourceSpan::UNKNOWN);
            builder.ins().br(block1, &[v3, v4, v5], SourceSpan::UNKNOWN);

            // block1 - outer loop header
            let v6 = builder.append_block_param(block1, Type::U32, SourceSpan::UNKNOWN);
            let v7 = builder.append_block_param(block1, Type::U32, SourceSpan::UNKNOWN);
            let v8 = builder.append_block_param(block1, Type::U64, SourceSpan::UNKNOWN);
            builder.switch_to_block(block1);
            let v9 = builder.ins().eq(v6, v1, SourceSpan::UNKNOWN);
            builder.ins().cond_br(v9, block2, &[], block3, &[], SourceSpan::UNKNOWN);

            // block2 - outer exit
            builder.switch_to_block(block2);
            builder.ins().ret(Some(v8), SourceSpan::UNKNOWN);

            // block3 - split edge
            builder.switch_to_block(block3);
            builder.ins().br(block4, &[v7, v8], SourceSpan::UNKNOWN);

            // block4 - inner loop
            let v10 = builder.append_block_param(block4, Type::U32, SourceSpan::UNKNOWN);
            let v11 = builder.append_block_param(block4, Type::U64, SourceSpan::UNKNOWN);
            builder.switch_to_block(block4);
            let v12 = builder.ins().eq(v10, v2, SourceSpan::UNKNOWN);
            builder.ins().cond_br(v12, block5, &[], block6, &[], SourceSpan::UNKNOWN);

            // block5 - inner latch
            builder.switch_to_block(block5);
            let v13 = builder.ins().add_imm_unchecked(v6, Immediate::U32(1), SourceSpan::UNKNOWN);
            builder.ins().br(block1, &[v13, v10, v11], SourceSpan::UNKNOWN);

            // block6 - inner body
            builder.switch_to_block(block6);
            let v14 = builder.ins().add_imm_unchecked(v6, Immediate::U32(1), SourceSpan::UNKNOWN);
            let v15 = builder.ins().mul_unchecked(v14, v2, SourceSpan::UNKNOWN);
            let v16 = builder.ins().add_unchecked(v10, v15, SourceSpan::UNKNOWN);
            let v17 = builder.ins().ptrtoint(v0, Type::U32, SourceSpan::UNKNOWN);
            let v18 = builder.ins().add_unchecked(v17, v16, SourceSpan::UNKNOWN);
            let v19 =
                builder.ins().inttoptr(v18, Type::Ptr(Box::new(Type::U64)), SourceSpan::UNKNOWN);
            let v20 = builder.ins().load(v19, SourceSpan::UNKNOWN);
            let v21 = builder.ins().add_unchecked(v11, v20, SourceSpan::UNKNOWN);
            let v22 = builder.ins().add_imm_unchecked(v10, Immediate::U32(1), SourceSpan::UNKNOWN);
            builder.ins().br(block4, &[v22, v21], SourceSpan::UNKNOWN);
        }

        let mut analyses = AnalysisManager::default();
        let liveness = analyses.get_or_compute::<LivenessAnalysis>(&function, &context.session)?;
        let loops = analyses.get_or_compute::<LoopAnalysis>(&function, &context.session)?;

        dbg!(&liveness);

        let block0 = Block::from_u32(0);
        let block1 = Block::from_u32(1);
        let block2 = Block::from_u32(2);
        let block3 = Block::from_u32(3);
        let block4 = Block::from_u32(4);
        let block5 = Block::from_u32(5);
        let block6 = Block::from_u32(6);

        // Sanity check structure of the loop nest
        let loop0 = Loop::from_u32(0);
        let loop1 = Loop::from_u32(1);
        assert_eq!(loops.loops().len(), 2);
        assert!(loops.is_child_loop(loop1, loop0));
        assert_eq!(loops.innermost_loop(block0), None);
        assert_eq!(loops.innermost_loop(block1), Some(loop0));
        assert_eq!(loops.innermost_loop(block2), None);
        assert_eq!(loops.innermost_loop(block3), Some(loop0));
        assert_eq!(loops.innermost_loop(block4), Some(loop1));
        assert_eq!(loops.innermost_loop(block5), Some(loop0));
        assert_eq!(loops.innermost_loop(block6), Some(loop1));
        assert!(loops.is_in_loop(block1, loop0));
        assert!(!loops.is_in_loop(block2, loop0));
        assert!(loops.is_in_loop(block3, loop0));
        assert!(!loops.is_in_loop(block3, loop1));
        assert!(loops.is_in_loop(block4, loop0));
        assert!(loops.is_in_loop(block4, loop1));
        assert!(loops.is_in_loop(block5, loop0));
        assert!(!loops.is_in_loop(block5, loop1));
        assert!(loops.is_in_loop(block6, loop0));
        assert!(loops.is_in_loop(block6, loop1));
        assert_eq!(loops.is_loop_header(block1), Some(loop0));
        assert_eq!(loops.is_loop_header(block4), Some(loop1));

        // v0's first usage occurs inside the inner loop, but that usage is reached without
        // crossing any loop exits, so the next-use distance should not include any loop exit
        // distance
        let v0 = Value::from_u32(0);
        assert!(liveness.is_live_after(&v0, ProgramPoint::Block(block0)));
        assert_eq!(liveness.next_use_after(&v0, ProgramPoint::Block(block0)), 9);

        // v0's next usage from the perspective of the inner loop header should reflect the
        // number of instructions from the header to its use in the inner loop body, _not_
        // the longer distance given by exiting out to the outer loop, then re-entering the
        // inner loop
        assert!(liveness.is_live_at(&v0, ProgramPoint::Block(block4)));
        assert_eq!(liveness.next_use(&v0, ProgramPoint::Block(block4)), 5);
        assert!(liveness.is_live_after(&v0, ProgramPoint::Block(block4)));
        assert_eq!(liveness.next_use_after(&v0, ProgramPoint::Block(block4)), 4);

        // The same is true at the end of the inner loop body - the next usage _may_ occur
        // across an exit (out to the outer loop, then back in to the inner loop); but its
        // minimum next-use distance is another iteration of the inner loop, so there should
        // be no loop exit distance included
        let block6_term = function.dfg.last_inst(block6).unwrap();
        assert!(liveness.is_live_at(&v0, ProgramPoint::Inst(block6_term)));
        assert_eq!(liveness.next_use(&v0, ProgramPoint::Inst(block6_term)), 6);
        assert!(liveness.is_live_after(&v0, ProgramPoint::Block(block6)));
        assert_eq!(liveness.next_use_after(&v0, ProgramPoint::Block(block6)), 6);

        // v1 is similar to v0, except rather than being used in the inner loop, it is used
        // in the outer loop, and is unused in (but live-through) the inner loop. Thus we
        // would expect v1 to have a normal distance from block0, and a loop-exit distance
        // from blocks of the inner loop - we also expect v1 to be considered live in blocks
        // of the inner loop, even though it is not used _in_ the inner loop
        let v1 = Value::from_u32(1);
        assert!(liveness.is_live_after(&v1, ProgramPoint::Block(block0)));
        assert_eq!(liveness.next_use_after(&v1, ProgramPoint::Block(block0)), 1);
        assert!(liveness.is_live_at(&v1, ProgramPoint::Block(block1)));
        assert_eq!(liveness.next_use(&v1, ProgramPoint::Block(block1)), 0);

        // The next nearest use of v1 after the use _in_ block1, requires entering the inner loop
        // header and then exiting it almost immediately (i.e. not passing through block6)
        assert!(liveness.is_live_after(&v1, ProgramPoint::Block(block1)));
        assert_eq!(
            liveness.next_use_after(&v1, ProgramPoint::Block(block1)),
            5 + LOOP_EXIT_DISTANCE
        );

        // Naturally, v1 must be live at all blocks which are on the path back to block1:
        //
        // block3 -> block4 -> block5 -> block1
        //                  -> block6 ->
        assert!(liveness.is_live_at(&v1, ProgramPoint::Block(block3)));
        assert!(liveness.is_live_after(&v1, ProgramPoint::Block(block3)));
        assert!(liveness.is_live_at(&v1, ProgramPoint::Block(block4)));
        assert!(liveness.is_live_after(&v1, ProgramPoint::Block(block4)));
        assert!(liveness.is_live_at(&v1, ProgramPoint::Block(block5)));
        assert!(liveness.is_live_after(&v1, ProgramPoint::Block(block5)));
        assert!(liveness.is_live_at(&v1, ProgramPoint::Block(block6)));
        assert!(liveness.is_live_after(&v1, ProgramPoint::Block(block6)));

        Ok(())
    }
}
