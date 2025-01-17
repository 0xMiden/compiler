use std::{
    cmp::{self, Ordering},
    collections::{BTreeSet, VecDeque},
    mem,
};

use cranelift_entity::{packed_option::PackedOption, SecondaryMap};
use midenc_hir::{
    pass::{Analysis, AnalysisManager, AnalysisResult, PreservedAnalyses},
    Block, BranchInfo, DataFlowGraph, Function, Inst, ProgramPoint, Value,
};
use midenc_session::Session;
use smallvec::SmallVec;

use super::{BlockPredecessor, ControlFlowGraph};

/// RPO numbers are assigned as multiples of STRIDE to leave room
/// for modifications to the dominator tree.
const STRIDE: u32 = 4;

/// A special RPO number used during `compute_postorder`.
const SEEN: u32 = 1;

/// A node in the dominator tree. Each block has one of these.
#[derive(Clone, Default)]
struct Node {
    /// Number of this node in a reverse post-order traversal of the control-flow graph, starting
    /// from 1.
    ///
    /// This number is monotonic in the reverse post-order but not contiguous, as we leave holes
    /// for localized modifications of the dominator tree after it is initially computed.
    ///
    /// Unreachable nodes get number 0, all others are > 0.
    rpo_number: u32,
    /// The immediate dominator of this block, represented as the instruction at the end of the
    /// dominating block which transfers control to this block.
    ///
    /// This is `None` for unreachable blocks, as well as the entry block, which has no dominators.
    idom: PackedOption<Inst>,
}

/// DFT stack state marker for computing the cfg post-order.
enum Visit {
    First,
    Last,
}

#[derive(Default)]
pub struct DominatorTree {
    nodes: SecondaryMap<Block, Node>,
    /// Post-order of all reachable blocks in the control flow graph
    postorder: Vec<Block>,
    /// Scratch buffer used by `compute_postorder`
    stack: Vec<(Visit, Block)>,
    valid: bool,
}
impl Analysis for DominatorTree {
    type Entity = Function;

    fn analyze(
        function: &Self::Entity,
        analyses: &mut AnalysisManager,
        session: &Session,
    ) -> AnalysisResult<Self> {
        let cfg = analyses.get_or_compute(function, session)?;
        Ok(DominatorTree::with_function(function, &cfg))
    }

    fn is_invalidated(&self, preserved: &PreservedAnalyses) -> bool {
        !preserved.is_preserved::<ControlFlowGraph>()
    }
}
impl DominatorTree {
    /// Allocate a new blank dominator tree. Use `compute` to compute the dominator tree for a
    /// function.
    pub fn new() -> Self {
        Self::default()
    }

    /// Allocate and compute a dominator tree.
    pub fn with_function(func: &Function, cfg: &ControlFlowGraph) -> Self {
        let block_capacity = func.dfg.num_blocks();
        let mut domtree = Self {
            nodes: SecondaryMap::with_capacity(block_capacity),
            postorder: Vec::with_capacity(block_capacity),
            stack: Vec::new(),
            valid: false,
        };
        domtree.compute(func, cfg);
        domtree
    }

    /// Reset and compute a CFG post-order and dominator tree.
    pub fn compute(&mut self, func: &Function, cfg: &ControlFlowGraph) {
        debug_assert!(cfg.is_valid());
        self.compute_postorder(func);
        self.compute_domtree(func, cfg);
        self.valid = true;
    }

    /// Clear the data structures used to represent the dominator tree. This will leave the tree in
    /// a state where `is_valid()` returns false.
    pub fn clear(&mut self) {
        self.nodes.clear();
        self.postorder.clear();
        debug_assert!(self.stack.is_empty());
        self.valid = false;
    }

    /// Check if the dominator tree is in a valid state.
    ///
    /// Note that this doesn't perform any kind of validity checks. It simply checks if the
    /// `compute()` method has been called since the last `clear()`. It does not check that the
    /// dominator tree is consistent with the CFG.
    pub fn is_valid(&self) -> bool {
        self.valid
    }

    /// Is `block` reachable from the entry block?
    pub fn is_reachable(&self, block: Block) -> bool {
        self.nodes[block].rpo_number != 0
    }

    /// Get the blocks in cfg post-order used to compute the dominator tree.
    ///
    /// NOTE: This order is not updated automatically when the control-flow graph is modified,
    /// it is computed from scratch and cached by `compute`.
    pub fn cfg_postorder(&self) -> &[Block] {
        debug_assert!(self.is_valid());
        &self.postorder
    }

    /// Returns the immediate dominator of `block`.
    ///
    /// The immediate dominator of a basic block is the instruction which transfers control to that
    /// block (and implicitly, its enclosing block). This instruction does not have to be the
    /// terminator of its block, though it typically is.
    ///
    /// An instruction "dominates" `block` if all control flow paths from the function entry to
    /// `block` must go through that instruction.
    ///
    /// The "immediate dominator" is the dominator that is closest to `block`. All other dominators
    /// also dominate the immediate dominator.
    ///
    /// This returns `None` if `block` is not reachable from the entry block, or if it is the entry
    /// block which has no dominators.
    pub fn idom(&self, block: Block) -> Option<Inst> {
        self.nodes[block].idom.into()
    }

    /// Compare two blocks relative to the reverse post-order.
    pub fn rpo_cmp_block(&self, a: Block, b: Block) -> Ordering {
        self.nodes[a].rpo_number.cmp(&self.nodes[b].rpo_number)
    }

    /// Compare two program points relative to a reverse post-order traversal of the control-flow
    /// graph.
    ///
    /// Return `Ordering::Less` if `a` comes before `b` in the RPO.
    ///
    /// If `a` and `b` belong to the same block, compare their relative position in the block.
    pub fn rpo_cmp<A, B>(&self, a: A, b: B, dfg: &DataFlowGraph) -> Ordering
    where
        A: Into<ProgramPoint>,
        B: Into<ProgramPoint>,
    {
        let a = a.into();
        let b = b.into();
        self.rpo_cmp_block(dfg.pp_block(a), dfg.pp_block(b))
            .then_with(|| dfg.pp_cmp(a, b))
    }

    /// Returns `true` if `a` dominates `b`.
    ///
    /// The dominance relation requires that every path from the entry block to `b` passes through
    /// `a`. As this function determines _non-strict_ dominance, a block/instruction is considered
    /// to dominate itself. See [DominatorTree::strictly_dominates] for the strict variant of this
    /// relationship.
    ///
    /// Dominance is ill defined for unreachable blocks. If you happen to be querying dominance for
    /// instructions in the same unreachable block, the result is always correct; but for other
    /// pairings, the result will always be `false` if one of them is unreachable.
    pub fn dominates<A, B>(&self, a: A, b: B, dfg: &DataFlowGraph) -> bool
    where
        A: Into<ProgramPoint>,
        B: Into<ProgramPoint>,
    {
        let a = a.into();
        let b = b.into();
        if a == b {
            return true;
        }
        match a {
            ProgramPoint::Block(block_a) => self.last_dominator(block_a, b, dfg).is_some(),
            ProgramPoint::Inst(inst_a) => {
                let block_a =
                    dfg.inst_block(inst_a).expect("instruction is not attached to a block");
                match self.last_dominator(block_a, b, dfg) {
                    Some(last) => dfg.pp_cmp(inst_a, last) != Ordering::Greater,
                    None => false,
                }
            }
        }
    }

    /// Returns `true` if `a` strictly dominates `b`.
    ///
    /// This dominance relation requires that `a != b`, and that every path from the entry block to
    /// `b` passes through `a`. See [DominatorTree::dominates] for the non-strict variant of this
    /// relationship.
    ///
    /// Dominance is ill defined for unreachable blocks. If you happen to be querying dominance for
    /// instructions in the same unreachable block, the result is always correct; but for other
    /// pairings, the result will always be `false` if one of them is unreachable.
    pub fn strictly_dominates<A, B>(&self, a: A, b: B, dfg: &DataFlowGraph) -> bool
    where
        A: Into<ProgramPoint>,
        B: Into<ProgramPoint>,
    {
        let a = a.into();
        let b = b.into();
        if a == b {
            return false;
        }
        match a {
            ProgramPoint::Block(block_a) => self.last_dominator(block_a, b, dfg).is_some(),
            ProgramPoint::Inst(inst_a) => {
                let block_a =
                    dfg.inst_block(inst_a).expect("instruction is not attached to a block");
                match self.last_dominator(block_a, b, dfg) {
                    Some(last) => dfg.pp_cmp(inst_a, last) == Ordering::Less,
                    None => false,
                }
            }
        }
    }

    /// Find the last instruction in `a` that dominates `b`.
    ///
    /// If no instructions in `a` dominate `b`, return `None`.
    pub fn last_dominator<B>(&self, a: Block, b: B, dfg: &DataFlowGraph) -> Option<Inst>
    where
        B: Into<ProgramPoint>,
    {
        let (mut block_b, mut inst_b) = match b.into() {
            ProgramPoint::Block(block) => (block, None),
            ProgramPoint::Inst(inst) => (
                dfg.inst_block(inst).expect("instruction is not attached to a block"),
                Some(inst),
            ),
        };
        let rpo_a = self.nodes[a].rpo_number;

        // Run a finger up the dominator tree from b until we see a.
        // Do nothing if b is unreachable.
        while rpo_a < self.nodes[block_b].rpo_number {
            let idom = self.idom(block_b)?;
            block_b = dfg
                .inst_block(idom)
                .expect("control flow graph has been modified since dominator tree was computed");
            inst_b = Some(idom);
        }
        if a == block_b {
            inst_b
        } else {
            None
        }
    }

    /// Compute the common dominator of two basic blocks.
    ///
    /// Both basic blocks are assumed to be reachable.
    pub fn common_dominator(
        &self,
        mut a: BlockPredecessor,
        mut b: BlockPredecessor,
        dfg: &DataFlowGraph,
    ) -> BlockPredecessor {
        loop {
            match self.rpo_cmp_block(a.block, b.block) {
                Ordering::Less => {
                    // `a` comes before `b` in the RPO. Move `b` up.
                    let idom = self.nodes[b.block].idom.expect("Unreachable basic block?");
                    b = BlockPredecessor::new(
                        dfg.inst_block(idom).expect("Dangling idom instruction"),
                        idom,
                    );
                }
                Ordering::Greater => {
                    // `b` comes before `a` in the RPO. Move `a` up.
                    let idom = self.nodes[a.block].idom.expect("Unreachable basic block?");
                    a = BlockPredecessor::new(
                        dfg.inst_block(idom).expect("Dangling idom instruction"),
                        idom,
                    );
                }
                Ordering::Equal => break,
            }
        }

        debug_assert_eq!(a.block, b.block, "Unreachable block passed to common_dominator?");

        // We're in the same block. The common dominator is the earlier instruction.
        if dfg.pp_cmp(a.inst, b.inst) == Ordering::Less {
            a
        } else {
            b
        }
    }

    /// Reset all internal data structures and compute a post-order of the control flow graph.
    ///
    /// This leaves `rpo_number == 1` for all reachable blocks, 0 for unreachable ones.
    fn compute_postorder(&mut self, func: &Function) {
        self.clear();
        self.nodes.resize(func.dfg.num_blocks());

        // This algorithm is a depth first traversal (DFT) of the control flow graph, computing a
        // post-order of the blocks that are reachable form the entry block. A DFT post-order is not
        // unique. The specific order we get is controlled by the order each node's children are
        // visited.
        //
        // We view the CFG as a graph where each `BlockCall` value of a terminating branch
        // instruction is an edge. A consequence of this is that we visit successor nodes in the
        // reverse order specified by the branch instruction that terminates the basic block.
        // (Reversed because we are using a stack to control traversal, and push the successors in
        // the order the branch instruction specifies -- there's no good reason for this particular
        // order.)
        //
        // During this algorithm only, use `rpo_number` to hold the following state:
        //
        //   0:    block has not yet had its first visit
        //   SEEN: block has been visited at least once, implying that all of its successors are on
        //         the stack
        self.stack.push((Visit::First, func.dfg.entry_block()));

        while let Some((visit, block)) = self.stack.pop() {
            match visit {
                Visit::First => {
                    if self.nodes[block].rpo_number == 0 {
                        // This is the first time we pop the block, so we need to scan its
                        // successors and then revisit it.
                        self.nodes[block].rpo_number = SEEN;
                        self.stack.push((Visit::Last, block));
                        if let Some(inst) = func.dfg.last_inst(block) {
                            // Heuristic: chase the children in reverse. This puts the first
                            // successor block first in the postorder, all other things being
                            // equal, which tends to prioritize loop backedges over out-edges,
                            // putting the edge-block closer to the loop body and minimizing
                            // live-ranges in linear instruction space. This heuristic doesn't have
                            // any effect on the computation of dominators, and is purely for other
                            // consumers of the postorder we cache here.
                            match func.dfg.analyze_branch(inst) {
                                BranchInfo::NotABranch => (),
                                BranchInfo::SingleDest(successor) => {
                                    if self.nodes[successor.destination].rpo_number == 0 {
                                        self.stack.push((Visit::First, successor.destination));
                                    }
                                }
                                BranchInfo::MultiDest(ref successors) => {
                                    for successor in successors.iter().rev() {
                                        if self.nodes[successor.destination].rpo_number == 0 {
                                            self.stack.push((Visit::First, successor.destination));
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                Visit::Last => {
                    // We've finished all this node's successors.
                    self.postorder.push(block);
                }
            }
        }
    }

    /// Build a dominator tree from a control flow graph using Keith D. Cooper's
    /// "Simple, Fast Dominator Algorithm."
    fn compute_domtree(&mut self, func: &Function, cfg: &ControlFlowGraph) {
        // During this algorithm, `rpo_number` has the following values:
        //
        // 0: block is not reachable.
        // 1: block is reachable, but has not yet been visited during the first pass. This is set by
        // `compute_postorder`.
        // 2+: block is reachable and has an assigned RPO number.

        // We'll be iterating over a reverse post-order of the CFG, skipping the entry block.
        let (entry_block, postorder) = match self.postorder.as_slice().split_last() {
            Some((&eb, rest)) => (eb, rest),
            None => return,
        };

        // Do a first pass where we assign RPO numbers to all reachable nodes.
        self.nodes[entry_block].rpo_number = 2 * STRIDE;
        for (rpo_idx, &block) in postorder.iter().rev().enumerate() {
            // Update the current node and give it an RPO number.
            // The entry block got 2, the rest start at 3 by multiples of STRIDE to leave
            // room for future dominator tree modifications.
            //
            // Since `compute_idom` will only look at nodes with an assigned RPO number, the
            // function will never see an uninitialized predecessor.
            //
            // Due to the nature of the post-order traversal, every node we visit will have at
            // least one predecessor that has previously been visited during this RPO.
            self.nodes[block] = Node {
                idom: self.compute_idom(block, cfg, &func.dfg).into(),
                rpo_number: (rpo_idx as u32 + 3) * STRIDE,
            }
        }

        // Now that we have RPO numbers for everything and initial immediate dominator estimates,
        // iterate until convergence.
        //
        // If the function is free of irreducible control flow, this will exit after one iteration.
        let mut changed = true;
        while changed {
            changed = false;
            for &block in postorder.iter().rev() {
                let idom = self.compute_idom(block, cfg, &func.dfg).into();
                if self.nodes[block].idom != idom {
                    self.nodes[block].idom = idom;
                    changed = true;
                }
            }
        }
    }

    // Compute the immediate dominator for `block` using the current `idom` states for the reachable
    // nodes.
    fn compute_idom(&self, block: Block, cfg: &ControlFlowGraph, dfg: &DataFlowGraph) -> Inst {
        // Get an iterator with just the reachable, already visited predecessors to `block`.
        // Note that during the first pass, `rpo_number` is 1 for reachable blocks that haven't
        // been visited yet, 0 for unreachable blocks.
        let mut reachable_preds = cfg
            .pred_iter(block)
            .filter(|&BlockPredecessor { block: pred, .. }| self.nodes[pred].rpo_number > 1);

        // The RPO must visit at least one predecessor before this node.
        let mut idom =
            reachable_preds.next().expect("block node must have one reachable predecessor");

        for pred in reachable_preds {
            idom = self.common_dominator(idom, pred, dfg);
        }

        idom.inst
    }
}

/// Auxiliary structure for `DominatorTree` which provides:
///
/// - Traversal of the dominator tree in pre-order
/// - Ordering of blocks in dominator tree pre-order
/// - Constant-time dominance checks per-block
#[derive(Default)]
pub struct DominatorTreePreorder {
    nodes: SecondaryMap<Block, PreorderNode>,
    stack: Vec<Block>,
}

#[derive(Default, Clone)]
struct PreorderNode {
    /// First child node in the dominator tree
    child: Option<Block>,
    /// Next sibling node in the dominator tree, ordered
    /// according to the control-flow graph reverse post-order.
    sibling: Option<Block>,
    /// Sequence number for this node in a pre-order traversal of the dominator tree
    ///
    /// Unreachable blocks are 0, entry block is 1
    pre_number: u32,
    /// Maximum `pre_number` for the sub-tree of the dominator tree that is rooted at this node.
    ///
    /// This is always greater than or equal to `pre_number`
    pre_max: u32,
}
impl DominatorTreePreorder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_function(domtree: &DominatorTree, function: &Function) -> Self {
        let mut this = Self::new();
        this.compute(domtree, function);
        this
    }

    pub fn compute(&mut self, domtree: &DominatorTree, function: &Function) {
        self.nodes.clear();
        debug_assert_eq!(self.stack.len(), 0);

        // Step 1: Populate the child and sibling links.
        //
        // By following the CFG post-order and pushing to the front of the lists, we make sure that
        // sibling lists are ordered according to the CFG reverse post-order.
        for &block in domtree.cfg_postorder() {
            if let Some(idom_inst) = domtree.idom(block) {
                let idom = function.dfg.inst_block(idom_inst).unwrap();
                let sib = mem::replace(&mut self.nodes[idom].child, Some(block));
                self.nodes[block].sibling = sib;
            } else {
                // The only block without an immediate dominator is the entry.
                self.stack.push(block);
            }
        }

        // Step 2. Assign pre-order numbers from a DFS of the dominator tree.
        debug_assert!(self.stack.len() <= 1);
        let mut n = 0;
        while let Some(block) = self.stack.pop() {
            n += 1;
            let node = &mut self.nodes[block];
            node.pre_number = n;
            node.pre_max = n;
            if let Some(n) = node.sibling {
                self.stack.push(n);
            }
            if let Some(n) = node.child {
                self.stack.push(n);
            }
        }

        // Step 3. Propagate the `pre_max` numbers up the tree.
        // The CFG post-order is topologically ordered w.r.t. dominance so a node comes after all
        // its dominator tree children.
        for &block in domtree.cfg_postorder() {
            if let Some(idom_inst) = domtree.idom(block) {
                let idom = function.dfg.inst_block(idom_inst).unwrap();
                let pre_max = cmp::max(self.nodes[block].pre_max, self.nodes[idom].pre_max);
                self.nodes[idom].pre_max = pre_max;
            }
        }
    }

    /// Get an iterator over the immediate children of `block` in the dominator tree.
    ///
    /// These are the blocks whose immediate dominator is an instruction in `block`, ordered
    /// according to the CFG reverse post-order.
    pub fn children(&self, block: Block) -> ChildIter {
        ChildIter {
            dtpo: self,
            next: self.nodes[block].child,
        }
    }

    /// Fast, constant time dominance check with block granularity.
    ///
    /// This computes the same result as `domtree.dominates(a, b)`, but in guaranteed fast constant
    /// time. This is less general than the `DominatorTree` method because it only works with block
    /// program points.
    ///
    /// A block is considered to dominate itself.
    pub fn dominates(&self, a: Block, b: Block) -> bool {
        let na = &self.nodes[a];
        let nb = &self.nodes[b];
        na.pre_number <= nb.pre_number && na.pre_max >= nb.pre_max
    }

    /// Compare two blocks according to the dominator pre-order.
    pub fn pre_cmp_block(&self, a: Block, b: Block) -> Ordering {
        self.nodes[a].pre_number.cmp(&self.nodes[b].pre_number)
    }

    /// Compare two program points according to the dominator tree pre-order.
    ///
    /// This ordering of program points have the property that given a program point, pp, all the
    /// program points dominated by pp follow immediately and contiguously after pp in the order.
    pub fn pre_cmp<A, B>(&self, a: A, b: B, function: &Function) -> Ordering
    where
        A: Into<ProgramPoint>,
        B: Into<ProgramPoint>,
    {
        let a = a.into();
        let b = b.into();
        self.pre_cmp_block(function.dfg.pp_block(a), function.dfg.pp_block(b))
            .then_with(|| function.dfg.pp_cmp(a, b))
    }
}

/// An iterator that enumerates the direct children of a block in the dominator tree.
pub struct ChildIter<'a> {
    dtpo: &'a DominatorTreePreorder,
    next: Option<Block>,
}

impl Iterator for ChildIter<'_> {
    type Item = Block;

    fn next(&mut self) -> Option<Self::Item> {
        let n = self.next;
        if let Some(block) = n {
            self.next = self.dtpo.nodes[block].sibling;
        }
        n
    }
}

/// Calculates the dominance frontier for every block in a given `DominatorTree`
///
/// The dominance frontier of a block `B` is the set of blocks `DF` where for each block `Y` in `DF`
/// `B` dominates some predecessor of `Y`, but does not strictly dominate `Y`.
///
/// Dominance frontiers are useful in the construction of SSA form, as well as identifying control
/// dependent dataflow (for example, a variable in a program that has a different value depending
/// on what branch of an `if` statement is taken).
///
/// A dominance frontier can also be computed for a set of blocks, by taking the union of the
/// dominance frontiers of each block in the set.
///
/// An iterated dominance frontier is given by computing the dominance frontier for some set `X`,
/// i.e. `DF(X)`, then computing the dominance frontier on that, i.e. `DF(DF(X))`, taking the union
/// of the results, and repeating this process until fixpoint is reached. This is often represented
/// in literature as `DF+(X)`.
///
/// Iterated dominance frontiers are of particular usefulness to us, because they correspond to the
/// set of blocks in which we need to place phi nodes for some variable, in order to properly handle
/// control dependent dataflow for that variable.
///
/// Consider the following example (not in SSA form):
///
///
/// ```text,ignore
/// block0(x):
///   v = 0
///   cond_br x, block1, block2
///
/// block1():
///   v = 1
///   br block3
///
/// block2():
///   v = 2
///   br block3
///
/// block3:
///   ret v
/// ```
///
/// In this example, we have a variable, `v`, which is assigned new values later in the program
/// depending on which path through the program is taken. To transform this program into SSA form,
/// we take the set `V`, containing all of the assignments to `v`, and compute `DF+(V)`. Given
/// the program above, that would give us the set `{block3}`:
///
/// * The dominance frontier of the assignment in `block0` is empty, because `block0` strictly
///   dominates all other blocks in the program.
/// * The dominance frontier of the assignment in `block1` contains `block3`, because `block1`
///   dominates a predecessor of `block3` (itself), but does not strictly dominate that predecessor,
///   because a node cannot strictly dominate itself.
/// * The dominance frontier of the assignment in `block2` contains `block3`, for the same reasons
///   as `block1`.
/// * The dominance frontier of `block3` is empty, because it has no successors and thus cannot
///   dominate any other blocks.
/// * The union of all the dominance frontiers is simply `{block3}`
///
/// So this tells us that we need to place a phi node (a block parameter) at `block3`, and rewrite
/// all uses of `v` strictly dominated by the phi node to use the value associated with the phi
/// instead. In every predecessor of `block3`, we must pass `v` as a new block argument. Lastly, to
/// obtain SSA form, we rewrite assignments to `v` as defining new variables instead, and walk up
/// the dominance tree from each use of `v` until we find the nearest dominating definition for that
/// use, and rewrite the usage of `v` to use the value produced by that definition. Performing these
/// steps gives us the following program:
///
/// ```text,ignore
/// block0(x):
///   v0 = 0
///   cond_br x, block1, block2
///
/// block1():
///   v2 = 1
///   br block3(v2)
///
/// block2():
///   v3 = 2
///   br block3(v3)
///
/// block3(v1):
///   ret v1
/// ```
///
/// This program is in SSA form, and the dataflow for `v` is now explicit. An interesting
/// consequence of the transformation we performed, is that we are able to trivially recognize
/// that the definition of `v` in `block0` is unused, allowing us to eliminate it entirely.
#[derive(Default)]
pub struct DominanceFrontier {
    /// The dominance frontier for each block, as a set of blocks
    dfs: SecondaryMap<Block, BTreeSet<Block>>,
}
impl DominanceFrontier {
    pub fn compute(domtree: &DominatorTree, cfg: &ControlFlowGraph, function: &Function) -> Self {
        let mut dfs = SecondaryMap::<Block, BTreeSet<Block>>::default();

        for id in domtree.cfg_postorder() {
            let id = *id;
            if cfg.num_predecessors(id) < 2 {
                continue;
            }
            let idom = domtree.idom(id).unwrap();
            for BlockPredecessor { block: p, inst: i } in cfg.pred_iter(id) {
                let mut p = p;
                let mut i = i;
                while i != idom {
                    dfs[p].insert(id);
                    let Some(idom_p) = domtree.idom(p) else {
                        break;
                    };
                    i = idom_p;
                    p = function.dfg.inst_block(idom_p).unwrap();
                }
            }
        }

        Self { dfs }
    }

    /// Compute the iterated dominance frontier for `block`
    pub fn iterate(&self, block: &Block) -> BTreeSet<Block> {
        self.iterate_all([*block])
    }

    /// Compute the iterated dominance frontier for `blocks`
    pub fn iterate_all<I>(&self, blocks: I) -> BTreeSet<Block>
    where
        I: IntoIterator<Item = Block>,
    {
        let mut block_q = VecDeque::from_iter(blocks);
        let mut idf = BTreeSet::default();
        while let Some(block) = block_q.pop_front() {
            let added = self.dfs[block].difference(&idf).copied().collect::<SmallVec<[Block; 2]>>();
            if added.is_empty() {
                continue;
            }

            // Extend `df` and add the new blocks to the queue
            for block in added {
                idf.insert(block);
                if !block_q.contains(&block) {
                    block_q.push_back(block);
                }
            }
        }

        idf
    }

    /// Get an iterator over the dominance frontier of `block`
    pub fn iter(&self, block: &Block) -> impl Iterator<Item = Block> + '_ {
        DominanceFrontierIter {
            df: self.dfs.get(*block).map(|set| set.iter().copied()),
        }
    }

    /// Get an iterator over the dominance frontier of `value`
    pub fn iter_by_value(
        &self,
        value: Value,
        function: &Function,
    ) -> impl Iterator<Item = Block> + '_ {
        let defining_block = function.dfg.value_block(value);
        DominanceFrontierIter {
            df: self.dfs.get(defining_block).map(|set| set.iter().copied()),
        }
    }

    /// Get the set of blocks in the dominance frontier of `block`, or `None` if `block` has an
    /// empty dominance frontier.
    #[inline]
    pub fn get(&self, block: &Block) -> Option<&BTreeSet<Block>> {
        self.dfs.get(*block)
    }

    /// Get the set of blocks in the dominance frontier of `value`, or `None` if `value` has an
    /// empty dominance frontier.
    pub fn get_by_value(&self, value: Value, function: &Function) -> Option<&BTreeSet<Block>> {
        let defining_block = function.dfg.value_block(value);
        self.dfs.get(defining_block)
    }
}

struct DominanceFrontierIter<I> {
    df: Option<I>,
}
impl<I> Iterator for DominanceFrontierIter<I>
where
    I: Iterator<Item = Block>,
{
    type Item = Block;

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(i) = self.df.as_mut() {
            i.next()
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use midenc_hir::{
        AbiParam, FunctionBuilder, Immediate, InstBuilder, Signature, SourceSpan, Type,
    };

    use super::*;

    #[test]
    fn domtree_empty() {
        let id = "test::empty".parse().unwrap();
        let function = Function::new(id, Signature::new([], []));
        let entry = function.dfg.entry_block();

        let cfg = ControlFlowGraph::with_function(&function);
        assert!(cfg.is_valid());
        let domtree = DominatorTree::with_function(&function, &cfg);

        assert_eq!(1, domtree.nodes.keys().count());
        assert_eq!(domtree.cfg_postorder(), &[entry]);

        let mut dtpo = DominatorTreePreorder::new();
        dtpo.compute(&domtree, &function);
    }

    #[test]
    fn domtree_unreachable_node() {
        let id = "test::unreachable_node".parse().unwrap();
        let mut function = Function::new(id, Signature::new([AbiParam::new(Type::I32)], []));
        let block0 = function.dfg.entry_block();
        let block1 = function.dfg.create_block();
        let block2 = function.dfg.create_block();
        let trap_block = function.dfg.create_block();
        let v2 = {
            let mut builder = FunctionBuilder::new(&mut function);
            let v0 = {
                let args = builder.block_params(block0);
                args[0]
            };

            builder.switch_to_block(block0);
            let cond = builder.ins().neq_imm(v0, Immediate::I32(0), SourceSpan::UNKNOWN);
            builder.ins().cond_br(cond, block2, &[], trap_block, &[], SourceSpan::UNKNOWN);

            builder.switch_to_block(trap_block);
            builder.ins().unreachable(SourceSpan::UNKNOWN);

            builder.switch_to_block(block1);
            let v1 = builder.ins().i32(1, SourceSpan::UNKNOWN);
            let v2 = builder.ins().add_checked(v0, v1, SourceSpan::UNKNOWN);
            builder.ins().br(block0, &[v2], SourceSpan::UNKNOWN);

            builder.switch_to_block(block2);
            builder.ins().ret(Some(v0), SourceSpan::UNKNOWN);
            v2
        };

        let cfg = ControlFlowGraph::with_function(&function);
        let domtree = DominatorTree::with_function(&function, &cfg);

        // Fall-through-first, prune-at-source DFT:
        //
        // block0 {
        //   brif block2 {
        //     trap
        //     block2 {
        //       return
        //     } block2
        // } block0
        assert_eq!(domtree.cfg_postorder(), &[block2, trap_block, block0]);

        let v2_inst = function.dfg.value_data(v2).unwrap_inst();
        assert!(!domtree.dominates(v2_inst, block0, &function.dfg));
        assert!(!domtree.dominates(block0, v2_inst, &function.dfg));

        let mut dtpo = DominatorTreePreorder::new();
        dtpo.compute(&domtree, &function);
        assert!(dtpo.dominates(block0, block0));
        assert!(!dtpo.dominates(block0, block1));
        assert!(dtpo.dominates(block0, block2));
        assert!(!dtpo.dominates(block1, block0));
        assert!(dtpo.dominates(block1, block1));
        assert!(!dtpo.dominates(block1, block2));
        assert!(!dtpo.dominates(block2, block0));
        assert!(!dtpo.dominates(block2, block1));
        assert!(dtpo.dominates(block2, block2));
    }

    #[test]
    fn domtree_non_zero_entry_block() {
        let id = "test::non_zero_entry".parse().unwrap();
        let mut function = Function::new(id, Signature::new([], []));
        let block0 = function.dfg.entry_block();
        let block1 = function.dfg.create_block();
        let block2 = function.dfg.create_block();
        let block3 = function.dfg.create_block();
        let cond = function.dfg.append_block_param(block3, Type::I1, SourceSpan::UNKNOWN);
        function.dfg.entry = block3;
        function.signature.params.push(AbiParam::new(Type::I1));
        let (br_block3_block1, br_block1_block0_block2) = {
            let mut builder = FunctionBuilder::new(&mut function);

            builder.switch_to_block(block3);
            let br_block3_block1 = builder.ins().br(block1, &[], SourceSpan::UNKNOWN);

            builder.switch_to_block(block1);
            let br_block1_block0_block2 =
                builder.ins().cond_br(cond, block0, &[], block2, &[], SourceSpan::UNKNOWN);

            builder.switch_to_block(block2);
            builder.ins().br(block0, &[], SourceSpan::UNKNOWN);

            (br_block3_block1, br_block1_block0_block2)
        };

        let cfg = ControlFlowGraph::with_function(&function);
        let domtree = DominatorTree::with_function(&function, &cfg);

        // Fall-through-first, prune-at-source DFT:
        //
        // block3 {
        //   block3:jump block1 {
        //     block1 {
        //       block1:brif block0 {
        //         block1:jump block2 {
        //           block2 {
        //             block2:jump block0 (seen)
        //           } block2
        //         } block1:jump block2
        //         block0 {
        //         } block0
        //       } block1:brif block0
        //     } block1
        //   } block3:jump block1
        // } block3

        assert_eq!(domtree.cfg_postorder(), &[block0, block2, block1, block3]);

        assert_eq!(function.dfg.entry_block(), block3);
        assert_eq!(domtree.idom(block3), None);
        assert_eq!(domtree.idom(block1).unwrap(), br_block3_block1);
        assert_eq!(domtree.idom(block2).unwrap(), br_block1_block0_block2);
        assert_eq!(domtree.idom(block0).unwrap(), br_block1_block0_block2);

        assert!(domtree.dominates(br_block1_block0_block2, br_block1_block0_block2, &function.dfg));
        assert!(!domtree.dominates(br_block1_block0_block2, br_block3_block1, &function.dfg));
        assert!(domtree.dominates(br_block3_block1, br_block1_block0_block2, &function.dfg));

        assert_eq!(domtree.rpo_cmp(block3, block3, &function.dfg), Ordering::Equal);
        assert_eq!(domtree.rpo_cmp(block3, block1, &function.dfg), Ordering::Less);
        assert_eq!(domtree.rpo_cmp(block3, br_block3_block1, &function.dfg), Ordering::Less);
        assert_eq!(
            domtree.rpo_cmp(br_block3_block1, br_block1_block0_block2, &function.dfg),
            Ordering::Less
        );
    }

    #[test]
    fn domtree_backwards_layout() {
        let id = "test::backwards_layout".parse().unwrap();
        let mut function = Function::new(id, Signature::new([], []));
        let block0 = function.dfg.entry_block();
        let block1 = function.dfg.create_block();
        let block2 = function.dfg.create_block();
        let (jmp02, trap, jmp21) = {
            let mut builder = FunctionBuilder::new(&mut function);

            builder.switch_to_block(block0);
            let jmp02 = builder.ins().br(block2, &[], SourceSpan::UNKNOWN);

            builder.switch_to_block(block1);
            let trap = builder.ins().unreachable(SourceSpan::UNKNOWN);

            builder.switch_to_block(block2);
            let jmp21 = builder.ins().br(block1, &[], SourceSpan::UNKNOWN);

            (jmp02, trap, jmp21)
        };

        let cfg = ControlFlowGraph::with_function(&function);
        let domtree = DominatorTree::with_function(&function, &cfg);

        assert_eq!(function.dfg.entry_block(), block0);
        assert_eq!(domtree.idom(block0), None);
        assert_eq!(domtree.idom(block1), Some(jmp21));
        assert_eq!(domtree.idom(block2), Some(jmp02));

        assert!(domtree.dominates(block0, block0, &function.dfg));
        assert!(domtree.dominates(block0, jmp02, &function.dfg));
        assert!(domtree.dominates(block0, block1, &function.dfg));
        assert!(domtree.dominates(block0, trap, &function.dfg));
        assert!(domtree.dominates(block0, block2, &function.dfg));
        assert!(domtree.dominates(block0, jmp21, &function.dfg));

        assert!(!domtree.dominates(jmp02, block0, &function.dfg));
        assert!(domtree.dominates(jmp02, jmp02, &function.dfg));
        assert!(domtree.dominates(jmp02, block1, &function.dfg));
        assert!(domtree.dominates(jmp02, trap, &function.dfg));
        assert!(domtree.dominates(jmp02, block2, &function.dfg));
        assert!(domtree.dominates(jmp02, jmp21, &function.dfg));

        assert!(!domtree.dominates(block1, block0, &function.dfg));
        assert!(!domtree.dominates(block1, jmp02, &function.dfg));
        assert!(domtree.dominates(block1, block1, &function.dfg));
        assert!(domtree.dominates(block1, trap, &function.dfg));
        assert!(!domtree.dominates(block1, block2, &function.dfg));
        assert!(!domtree.dominates(block1, jmp21, &function.dfg));

        assert!(!domtree.dominates(trap, block0, &function.dfg));
        assert!(!domtree.dominates(trap, jmp02, &function.dfg));
        assert!(!domtree.dominates(trap, block1, &function.dfg));
        assert!(domtree.dominates(trap, trap, &function.dfg));
        assert!(!domtree.dominates(trap, block2, &function.dfg));
        assert!(!domtree.dominates(trap, jmp21, &function.dfg));

        assert!(!domtree.dominates(block2, block0, &function.dfg));
        assert!(!domtree.dominates(block2, jmp02, &function.dfg));
        assert!(domtree.dominates(block2, block1, &function.dfg));
        assert!(domtree.dominates(block2, trap, &function.dfg));
        assert!(domtree.dominates(block2, block2, &function.dfg));
        assert!(domtree.dominates(block2, jmp21, &function.dfg));

        assert!(!domtree.dominates(jmp21, block0, &function.dfg));
        assert!(!domtree.dominates(jmp21, jmp02, &function.dfg));
        assert!(domtree.dominates(jmp21, block1, &function.dfg));
        assert!(domtree.dominates(jmp21, trap, &function.dfg));
        assert!(!domtree.dominates(jmp21, block2, &function.dfg));
        assert!(domtree.dominates(jmp21, jmp21, &function.dfg));
    }
}
