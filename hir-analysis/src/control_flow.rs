use cranelift_bforest as bforest;
use cranelift_entity::SecondaryMap;
use midenc_hir::{
    pass::{Analysis, AnalysisManager, AnalysisResult},
    Block, DataFlowGraph, Function, Inst, Instruction,
};
use midenc_session::Session;

/// Represents the predecessor of the current basic block.
///
/// A predecessor in this context is both the instruction which transfers control to
/// the current block, and the block which encloses that instruction.
#[derive(Debug, PartialEq, Eq, Copy, Clone)]
pub struct BlockPredecessor {
    pub block: Block,
    pub inst: Inst,
}
impl BlockPredecessor {
    #[inline]
    pub fn new(block: Block, inst: Inst) -> Self {
        Self { block, inst }
    }
}

/// A node in the control flow graph, which contains the successors and predecessors of a given
/// `Block`.
#[derive(Clone, Default)]
struct Node {
    /// Instructions which transfer control to this block
    pub predecessors: bforest::Map<Inst, Block>,
    /// Set of blocks that are targets of branches/jumps in this block.
    pub successors: bforest::Set<Block>,
}

/// The control flow graph maps all blocks in a function to their predecessor and successor blocks.
pub struct ControlFlowGraph {
    data: SecondaryMap<Block, Node>,
    pred_forest: bforest::MapForest<Inst, Block>,
    succ_forest: bforest::SetForest<Block>,
    valid: bool,
}
impl Clone for ControlFlowGraph {
    fn clone(&self) -> Self {
        let mut data = SecondaryMap::<Block, Node>::with_capacity(self.data.capacity());
        let mut pred_forest = bforest::MapForest::new();
        let mut succ_forest = bforest::SetForest::new();

        for (k, v) in self.data.iter() {
            let node = &mut data[k];
            for (pk, pv) in v.predecessors.iter(&self.pred_forest) {
                node.predecessors.insert(pk, pv, &mut pred_forest, &());
            }
            for succ in v.successors.iter(&self.succ_forest) {
                node.successors.insert(succ, &mut succ_forest, &());
            }
        }

        Self {
            data,
            pred_forest,
            succ_forest,
            valid: self.valid,
        }
    }
}
impl Default for ControlFlowGraph {
    fn default() -> Self {
        Self {
            data: SecondaryMap::default(),
            pred_forest: bforest::MapForest::new(),
            succ_forest: bforest::SetForest::new(),
            valid: false,
        }
    }
}
impl Analysis for ControlFlowGraph {
    type Entity = Function;

    fn analyze(
        function: &Self::Entity,
        _analyses: &mut AnalysisManager,
        _session: &Session,
    ) -> AnalysisResult<Self> {
        Ok(ControlFlowGraph::with_function(function))
    }
}
impl ControlFlowGraph {
    pub fn new() -> Self {
        Self::default()
    }

    /// Reset this control flow graph to its initial state for reuse
    pub fn clear(&mut self) {
        self.data.clear();
        self.pred_forest.clear();
        self.succ_forest.clear();
        self.valid = false;
    }

    /// Obtain a control flow graph computed over `func`.
    pub fn with_function(func: &Function) -> Self {
        let mut cfg = Self::new();
        cfg.compute(&func.dfg);
        cfg
    }

    /// Compute the control flow graph for `dfg`.
    ///
    /// NOTE: This will reset the current state of this graph.
    pub fn compute(&mut self, dfg: &DataFlowGraph) {
        self.clear();
        self.data.resize(dfg.num_blocks());

        for (block, _) in dfg.blocks() {
            self.compute_block(dfg, block);
        }

        self.valid = true;
    }

    /// Recompute the control flow graph of `block`.
    ///
    /// This is for use after modifying instructions within a block. It recomputes all edges
    /// from `block` while leaving edges to `block` intact. It performs a restricted version of
    /// `compute` which allows us to avoid recomputing the graph for all blocks, only those which
    /// are modified by a specific set of changes.
    pub fn recompute_block(&mut self, dfg: &DataFlowGraph, block: Block) {
        debug_assert!(self.is_valid());
        self.invalidate_block_successors(block);
        self.compute_block(dfg, block);
    }

    /// Similar to `recompute_block`, this recomputes all edges from `block` as if they had been
    /// removed, while leaving edges to `block` intact. It is expected that predecessor blocks
    /// will have `recompute_block` subsequently called on them so that `block` is fully removed
    /// from the CFG.
    pub fn detach_block(&mut self, block: Block) {
        debug_assert!(self.is_valid());
        self.invalidate_block_successors(block);
    }

    /// Return the number of predecessors for `block`
    pub fn num_predecessors(&self, block: Block) -> usize {
        self.data[block].predecessors.iter(&self.pred_forest).count()
    }

    /// Return the number of successors for `block`
    pub fn num_successors(&self, block: Block) -> usize {
        self.data[block].successors.iter(&self.succ_forest).count()
    }

    /// Get an iterator over the CFG predecessors to `block`.
    pub fn pred_iter(&self, block: Block) -> PredIter {
        PredIter(self.data[block].predecessors.iter(&self.pred_forest))
    }

    /// Get an iterator over the CFG successors to `block`.
    pub fn succ_iter(&self, block: Block) -> SuccIter {
        debug_assert!(self.is_valid());
        self.data[block].successors.iter(&self.succ_forest)
    }

    /// Check if the CFG is in a valid state.
    ///
    /// Note that this doesn't perform any kind of validity checks. It simply checks if the
    /// `compute()` method has been called since the last `clear()`. It does not check that the
    /// CFG is consistent with the function.
    pub fn is_valid(&self) -> bool {
        self.valid
    }

    fn compute_block(&mut self, dfg: &DataFlowGraph, block: Block) {
        visit_block_succs(dfg, block, |inst, dest, _| {
            self.add_edge(block, inst, dest);
        });
    }

    fn invalidate_block_successors(&mut self, block: Block) {
        use core::mem;

        let mut successors = mem::take(&mut self.data[block].successors);
        for succ in successors.iter(&self.succ_forest) {
            self.data[succ]
                .predecessors
                .retain(&mut self.pred_forest, |_, &mut e| e != block);
        }
        successors.clear(&mut self.succ_forest);
    }

    fn add_edge(&mut self, from: Block, from_inst: Inst, to: Block) {
        self.data[from].successors.insert(to, &mut self.succ_forest, &());
        self.data[to].predecessors.insert(from_inst, from, &mut self.pred_forest, &());
    }
}

/// An iterator over block predecessors. The iterator type is `BlockPredecessor`.
///
/// Each predecessor is an instruction that branches to the block.
pub struct PredIter<'a>(bforest::MapIter<'a, Inst, Block>);

impl Iterator for PredIter<'_> {
    type Item = BlockPredecessor;

    fn next(&mut self) -> Option<BlockPredecessor> {
        self.0.next().map(|(i, e)| BlockPredecessor::new(e, i))
    }
}

/// An iterator over block successors. The iterator type is `Block`.
pub type SuccIter<'a> = bforest::SetIter<'a, Block>;

/// Visit all successors of a block with a given visitor closure. The closure
/// arguments are the branch instruction that is used to reach the successor,
/// the successor block itself, and a flag indicating whether the block is
/// branched to via a table entry.
pub(crate) fn visit_block_succs<F: FnMut(Inst, Block, bool)>(
    dfg: &DataFlowGraph,
    block: Block,
    mut visit: F,
) {
    use midenc_hir::{Br, CondBr, Switch};

    if let Some(inst) = dfg.last_inst(block) {
        match &dfg[inst] {
            Instruction::Br(Br { successor, .. }) => {
                visit(inst, successor.destination, false);
            }

            Instruction::CondBr(CondBr {
                then_dest,
                else_dest,
                ..
            }) => {
                visit(inst, then_dest.destination, false);
                visit(inst, else_dest.destination, false);
            }

            Instruction::Switch(Switch {
                ref arms,
                default: default_succ,
                ..
            }) => {
                visit(inst, default_succ.destination, false);

                for arm in arms.as_slice() {
                    visit(inst, arm.successor.destination, true);
                }
            }

            inst => debug_assert!(!inst.opcode().is_branch()),
        }
    }
}

#[cfg(test)]
mod tests {
    use midenc_hir::*;

    use super::*;

    #[test]
    fn cfg_empty() {
        let dfg = DataFlowGraph::default();

        let mut cfg = ControlFlowGraph::new();
        cfg.compute(&dfg);
    }

    #[test]
    fn cfg_no_predecessors() {
        let mut dfg = DataFlowGraph::default();

        let _block0 = dfg.create_block();
        let _block1 = dfg.create_block();
        let _block2 = dfg.create_block();

        let mut cfg = ControlFlowGraph::new();
        cfg.compute(&dfg);

        let mut blocks = dfg.blocks().map(|(blk, _)| blk);
        for block in dfg.blocks().map(|(blk, _)| blk) {
            assert_eq!(block, blocks.next().unwrap());
            assert_eq!(cfg.pred_iter(block).count(), 0);
            assert_eq!(cfg.succ_iter(block).count(), 0);
        }
    }

    #[test]
    fn cfg_branches_and_jumps() {
        let diagnostics = diagnostics::DiagnosticsHandler::default();

        // Define the 'test' module
        let mut builder = ModuleBuilder::new("test");

        // Declare the `fib` function, with the appropriate type signature
        let sig = Signature {
            params: vec![AbiParam::new(Type::I32)],
            results: vec![AbiParam::new(Type::I32)],
            cc: CallConv::SystemV,
            linkage: Linkage::External,
        };
        let mut fb =
            builder.function("branches_and_jumps", sig).expect("unexpected symbol conflict");

        let block0 = fb.entry_block();
        let cond = {
            let args = fb.block_params(block0);
            args[0]
        };

        let block1 = fb.create_block();
        let block2 = fb.create_block();

        let cond = fb.ins().trunc(cond, Type::I1, SourceSpan::default());
        let br_block0_block2_block1 =
            fb.ins().cond_br(cond, block2, &[], block1, &[], SourceSpan::default());
        fb.switch_to_block(block1);
        let br_block1_block1_block2 =
            fb.ins().cond_br(cond, block1, &[], block2, &[], SourceSpan::default());

        let id = fb
            .build(&diagnostics)
            .expect("unexpected validation error, see diagnostics output");

        let mut module = builder.build();
        let mut function = module.unlink(id.function);

        let mut cfg = ControlFlowGraph::with_function(&function);

        {
            let block0_predecessors = cfg.pred_iter(block0).collect::<Vec<_>>();
            let block1_predecessors = cfg.pred_iter(block1).collect::<Vec<_>>();
            let block2_predecessors = cfg.pred_iter(block2).collect::<Vec<_>>();

            let block0_successors = cfg.succ_iter(block0).collect::<Vec<_>>();
            let block1_successors = cfg.succ_iter(block1).collect::<Vec<_>>();
            let block2_successors = cfg.succ_iter(block2).collect::<Vec<_>>();

            assert_eq!(block0_predecessors.len(), 0);
            assert_eq!(block1_predecessors.len(), 2);
            assert_eq!(block2_predecessors.len(), 2);

            assert!(block1_predecessors
                .contains(&BlockPredecessor::new(block0, br_block0_block2_block1)));
            assert!(block1_predecessors
                .contains(&BlockPredecessor::new(block1, br_block1_block1_block2)));
            assert!(block2_predecessors
                .contains(&BlockPredecessor::new(block0, br_block0_block2_block1)));
            assert!(block2_predecessors
                .contains(&BlockPredecessor::new(block1, br_block1_block1_block2)));

            assert_eq!(block0_successors, [block1, block2]);
            assert_eq!(block1_successors, [block1, block2]);
            assert_eq!(block2_successors, []);
        }

        // Add a new block to hold a return instruction
        let ret_block;
        {
            let mut builder = FunctionBuilder::new(&mut function);
            ret_block = builder.create_block();
            builder.switch_to_block(ret_block);
            builder.ins().ret(None, SourceSpan::default());
        }

        // Change some instructions and recompute block0 and ret_block
        function.dfg.replace(br_block0_block2_block1).cond_br(
            cond,
            block1,
            &[],
            ret_block,
            &[],
            SourceSpan::default(),
        );
        cfg.recompute_block(&function.dfg, block0);
        cfg.recompute_block(&function.dfg, ret_block);
        let br_block0_block1_ret_block = br_block0_block2_block1;

        {
            let block0_predecessors = cfg.pred_iter(block0).collect::<Vec<_>>();
            let block1_predecessors = cfg.pred_iter(block1).collect::<Vec<_>>();
            let block2_predecessors = cfg.pred_iter(block2).collect::<Vec<_>>();

            let block0_successors = cfg.succ_iter(block0);
            let block1_successors = cfg.succ_iter(block1);
            let block2_successors = cfg.succ_iter(block2);

            assert_eq!(block0_predecessors.len(), 0);
            assert_eq!(block1_predecessors.len(), 2);
            assert_eq!(block2_predecessors.len(), 1);

            assert!(block1_predecessors
                .contains(&BlockPredecessor::new(block0, br_block0_block1_ret_block)),);
            assert!(block1_predecessors
                .contains(&BlockPredecessor::new(block1, br_block1_block1_block2)),);
            assert!(!block2_predecessors
                .contains(&BlockPredecessor::new(block0, br_block0_block1_ret_block)),);
            assert!(block2_predecessors
                .contains(&BlockPredecessor::new(block1, br_block1_block1_block2)),);

            assert_eq!(block0_successors.collect::<Vec<_>>(), [block1, ret_block]);
            assert_eq!(block1_successors.collect::<Vec<_>>(), [block1, block2]);
            assert_eq!(block2_successors.collect::<Vec<_>>(), []);
        }
    }
}
