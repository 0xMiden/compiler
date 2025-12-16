use alloc::collections::VecDeque;

use super::*;
use crate::{
    BlockArgument, BlockRef, ValueRef,
    adt::{SmallDenseMap, SmallSet},
};

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
    dfs: SmallDenseMap<BlockRef, SmallSet<BlockRef, 2>, 8>,
}

impl DominanceFrontier {
    pub fn new(domtree: &DominanceTree) -> Self {
        let mut this = Self::default();

        for node in domtree.postorder() {
            let Some(node_block) = node.block() else {
                continue;
            };

            let block = node_block.borrow();
            let has_multiple_predecessors = block.predecessors().enumerate().any(|(i, _)| i > 1);
            if !has_multiple_predecessors {
                continue;
            }

            let idom = node
                .idom()
                .expect("expected immediate dominator for block with multiple predecessors");
            let idom_block = idom.block().unwrap();
            for pred in block.predecessors() {
                let mut p = pred.predecessor();
                while p != idom_block {
                    this.dfs.entry(p).or_default().insert(node_block);
                    let node_p = domtree.get(Some(p)).unwrap();
                    let Some(idom_p) = node_p.idom() else {
                        break;
                    };
                    p = idom_p.block().unwrap();
                }
            }
        }

        this
    }

    /// Compute the iterated dominance frontier for `block`
    pub fn iterate(&self, block: BlockRef) -> SmallSet<BlockRef, 4> {
        self.iterate_all([block])
    }

    /// Compute the iterated dominance frontier for `blocks`
    pub fn iterate_all<I>(&self, blocks: I) -> SmallSet<BlockRef, 4>
    where
        I: IntoIterator<Item = BlockRef>,
    {
        let mut block_q = VecDeque::default();
        let mut idf = SmallSet::<_, 4>::default();

        let mut visit_block = |block: BlockRef, block_q: &mut VecDeque<BlockRef>| {
            // If `block` has an empty dominance frontier, there is nothing to add.
            let Some(df) = self.dfs.get(&block) else {
                return;
            };

            let added = df.difference(&idf);
            if added.is_empty() {
                return;
            }

            // Extend `idf` and add the new blocks to the queue
            for block in added {
                idf.insert(block);
                if !block_q.contains(&block) {
                    block_q.push_back(block);
                }
            }
        };

        // Process the initial set of blocks
        for block in blocks {
            visit_block(block, &mut block_q);
        }

        // Process any newly queued blocks
        while let Some(block) = block_q.pop_front() {
            visit_block(block, &mut block_q);
        }

        idf
    }

    /// Get an iterator over the dominance frontier of `block`
    pub fn iter(&self, block: BlockRef) -> impl Iterator<Item = BlockRef> + '_ {
        DominanceFrontierIter {
            df: self.dfs.get(&block).map(|set| set.iter().copied()),
        }
    }

    /// Get an iterator over the dominance frontier of `value`
    pub fn iter_by_value(&self, value: ValueRef) -> impl Iterator<Item = BlockRef> + '_ {
        let v = value.borrow();
        let defining_block = match v.get_defining_op() {
            Some(op) => op.parent().unwrap(),
            None => v.downcast_ref::<BlockArgument>().unwrap().owner(),
        };
        DominanceFrontierIter {
            df: self.dfs.get(&defining_block).map(|set| set.iter().copied()),
        }
    }

    /// Get the set of blocks in the dominance frontier of `block`, or `None` if `block` has an
    /// empty dominance frontier.
    #[inline]
    pub fn get(&self, block: &BlockRef) -> Option<&SmallSet<BlockRef, 2>> {
        self.dfs.get(block)
    }

    /// Get the set of blocks in the dominance frontier of `value`, or `None` if `value` has an
    /// empty dominance frontier.
    pub fn get_by_value(&self, value: ValueRef) -> Option<&SmallSet<BlockRef, 2>> {
        let v = value.borrow();
        let defining_block = match v.get_defining_op() {
            Some(op) => op.parent().unwrap(),
            None => v.downcast_ref::<BlockArgument>().unwrap().owner(),
        };
        self.dfs.get(&defining_block)
    }
}

struct DominanceFrontierIter<I> {
    df: Option<I>,
}
impl<I> Iterator for DominanceFrontierIter<I>
where
    I: Iterator<Item = BlockRef>,
{
    type Item = BlockRef;

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(i) = self.df.as_mut() {
            i.next()
        } else {
            None
        }
    }
}
