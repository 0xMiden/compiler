use alloc::collections::BTreeSet;

use crate::BlockRef;

#[allow(unused_variables)]
pub trait BlockVisitor {
    /// Called when a block is first reached during a depth-first traversal, i.e. called in preorder
    ///
    /// If this function returns `false`, none of `block`'s children will be visited. This can be
    /// used to prune the traversal, e.g. confining a visit to a specific loop in the CFG.
    fn on_block_reached(&mut self, from: Option<&BlockRef>, block: &BlockRef) -> bool {
        true
    }

    /// Called when all children of a block have been visited by the depth-first traversal, i.e.
    /// called in postorder.
    fn on_block_visited(&mut self, block: &BlockRef) {}
}

impl BlockVisitor for () {}

#[repr(transparent)]
pub struct PostOrderBlockIter(BlockIter<()>);
impl PostOrderBlockIter {
    #[inline]
    pub fn new(root: BlockRef) -> Self {
        Self(BlockIter::new(root, ()))
    }
}
impl core::iter::FusedIterator for PostOrderBlockIter {}
impl Iterator for PostOrderBlockIter {
    type Item = BlockRef;

    #[inline(always)]
    fn next(&mut self) -> Option<Self::Item> {
        self.0.next()
    }
}

pub struct BlockIter<V> {
    visited: BTreeSet<BlockRef>,
    // First element is the basic block, second is the index of the next child to visit, third is the number of children
    stack: Vec<(BlockRef, usize, usize)>,
    visitor: V,
}

impl<V: BlockVisitor> BlockIter<V> {
    pub fn new(from: BlockRef, visitor: V) -> Self {
        let mut this = Self {
            visited: Default::default(),
            stack: Default::default(),
            visitor,
        };
        this.insert_edge(None, from.clone());
        let num_successors = from.borrow().num_successors();
        this.stack.push((from, 0, num_successors));
        this.traverse_child();
        this
    }

    /// Returns true if the target of the given edge should be visited.
    ///
    /// Called with `None` for `from` when adding the root node.
    fn insert_edge(&mut self, from: Option<BlockRef>, to: BlockRef) -> bool {
        let should_visit = self.visitor.on_block_reached(from.as_ref(), &to);
        let unvisited = self.visited.insert(to);
        unvisited && should_visit
    }

    fn traverse_child(&mut self) {
        loop {
            let Some((entry, index, max)) = self.stack.last_mut() else {
                break;
            };
            if index == max {
                break;
            }
            let successor = entry.borrow().get_successor(*index);
            *index += 1;
            let entry = entry.clone();
            if self.insert_edge(Some(entry), successor.clone()) {
                // If the block is not visited..
                let num_successors = successor.borrow().num_successors();
                self.stack.push((successor, 0, num_successors));
            }
        }
    }
}

impl<V: BlockVisitor> core::iter::FusedIterator for BlockIter<V> {}
impl<V: BlockVisitor> Iterator for BlockIter<V> {
    type Item = BlockRef;

    fn next(&mut self) -> Option<Self::Item> {
        let (next, ..) = self.stack.pop()?;
        self.visitor.on_block_visited(&next);
        if !self.stack.is_empty() {
            self.traverse_child();
        }
        Some(next)
    }
}
