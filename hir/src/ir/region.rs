mod branch_point;
mod interfaces;
mod invocation_bounds;
mod kind;
mod successor;
mod transforms;

use smallvec::{smallvec, SmallVec};

pub use self::{
    branch_point::RegionBranchPoint,
    interfaces::{
        LoopLikeOpInterface, RegionBranchOpInterface, RegionBranchTerminatorOpInterface,
        RegionKindInterface,
    },
    invocation_bounds::InvocationBounds,
    kind::RegionKind,
    successor::{RegionSuccessor, RegionSuccessorInfo, RegionSuccessorIter},
    transforms::RegionTransformFailed,
};
use super::*;
use crate::{
    adt::SmallSet,
    patterns::RegionSimplificationLevel,
    traits::{SingleBlock, SingleRegion},
    Forward,
};

pub type RegionRef = UnsafeIntrusiveEntityRef<Region>;
/// An intrusive, doubly-linked list of [Region]s
pub type RegionList = EntityList<Region>;
/// A cursor in a [RegionList]
pub type RegionCursor<'a> = EntityCursor<'a, Region>;
/// A mutable cursor in a [RegionList]
pub type RegionCursorMut<'a> = EntityCursorMut<'a, Region>;

/// A region is a container for [Block], in one of two forms:
///
/// * Graph-like, in which the region consists of a single block, and the order of operations in
///   that block does not dictate any specific control flow semantics. It is up to the containing
///   operation to define.
/// * SSA-form, in which the region consists of one or more blocks that must obey the usual rules
///   of SSA dominance, and where operations in a block reflect the order in which those operations
///   are to be executed. Values defined by an operation must dominate any uses of those values in
///   the region.
///
/// The first block in a region is the _entry_ block, and its argument list corresponds to the
/// arguments expected by the region itself.
///
/// A region is only valid when it is attached to an [Operation], whereas the inverse is not true,
/// i.e. an operation without a parent region is a top-level operation, e.g. `Module`.
#[derive(Default)]
pub struct Region {
    /// The list of [Block]s that comprise this region
    body: BlockList,
}

impl Entity for Region {}

impl EntityWithParent for Region {
    type Parent = Operation;
}

impl EntityListItem for Region {}

impl EntityParent<Block> for Region {
    fn offset() -> usize {
        core::mem::offset_of!(Region, body)
    }
}

impl core::fmt::Debug for Region {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        core::fmt::Display::fmt(self, f)
    }
}
impl core::fmt::Display for Region {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let region_number = self.region_number();
        write!(f, "region({region_number})")
    }
}

impl cfg::Graph for Region {
    type ChildEdgeIter = block::BlockSuccessorEdgesIter;
    type ChildIter = block::BlockSuccessorIter;
    type Edge = BlockOperandRef;
    type Node = BlockRef;

    fn is_empty(&self) -> bool {
        self.body.is_empty()
    }

    fn size(&self) -> usize {
        self.body.len()
    }

    fn children(parent: Self::Node) -> Self::ChildIter {
        block::BlockSuccessorIter::new(parent)
    }

    fn children_edges(parent: Self::Node) -> Self::ChildEdgeIter {
        block::BlockSuccessorEdgesIter::new(parent)
    }

    fn edge_dest(edge: Self::Edge) -> Self::Node {
        edge.parent().unwrap()
    }

    fn entry_node(&self) -> Self::Node {
        self.body.front().as_pointer().expect("empty region")
    }
}

impl<'a> cfg::InvertibleGraph for &'a Region {
    type Inverse = cfg::Inverse<&'a Region>;
    type InvertibleChildEdgeIter = block::BlockPredecessorEdgesIter;
    type InvertibleChildIter = block::BlockPredecessorIter;

    fn inverse(self) -> Self::Inverse {
        cfg::Inverse::new(self)
    }

    fn inverse_children(parent: Self::Node) -> Self::InvertibleChildIter {
        block::BlockPredecessorIter::new(parent)
    }

    fn inverse_children_edges(parent: Self::Node) -> Self::InvertibleChildEdgeIter {
        block::BlockPredecessorEdgesIter::new(parent)
    }
}

/// Blocks
impl Region {
    /// Returns true if this region is empty (has no blocks)
    pub fn is_empty(&self) -> bool {
        self.body.is_empty()
    }

    /// Get a handle to the entry block for this region
    pub fn entry(&self) -> EntityRef<'_, Block> {
        self.body.front().into_borrow().unwrap()
    }

    /// Get a mutable handle to the entry block for this region
    pub fn entry_mut(&mut self) -> EntityMut<'_, Block> {
        self.body.front_mut().into_borrow_mut().unwrap()
    }

    /// Get the [BlockRef] of the entry block of this region, if it has one
    #[inline]
    pub fn entry_block_ref(&self) -> Option<BlockRef> {
        self.body.front().as_pointer()
    }

    /// Get the list of blocks comprising the body of this region
    pub fn body(&self) -> &BlockList {
        &self.body
    }

    /// Get a mutable reference to the list of blocks comprising the body of this region
    pub fn body_mut(&mut self) -> &mut BlockList {
        &mut self.body
    }
}

/// Traversal
impl Region {
    pub fn prewalk_all<F>(&self, callback: F)
    where
        F: FnMut(&Operation),
    {
        Walk::<Operation>::prewalk_all::<Forward, _>(self, callback);
    }

    pub fn prewalk<F, B>(&self, callback: F) -> WalkResult<B>
    where
        F: FnMut(&Operation) -> WalkResult<B>,
    {
        Walk::<Operation>::prewalk::<Forward, _, _>(self, callback)
    }

    pub fn postwalk_all<F>(&self, callback: F)
    where
        F: FnMut(&Operation),
    {
        Walk::<Operation>::postwalk_all::<Forward, _>(self, callback);
    }

    pub fn postwalk<F, B>(&self, callback: F) -> WalkResult<B>
    where
        F: FnMut(&Operation) -> WalkResult<B>,
    {
        Walk::<Operation>::postwalk::<Forward, _, _>(self, callback)
    }
}

/// Metadata
impl Region {
    #[inline]
    pub fn as_region_ref(&self) -> RegionRef {
        unsafe { RegionRef::from_raw(self) }
    }

    pub fn region_number(&self) -> usize {
        let op = self.parent().unwrap().borrow();
        op.regions()
            .iter()
            .position(|r| core::ptr::addr_eq(self, &*r))
            .expect("invalid region parent")
    }

    /// Returns true if this region is an ancestor of `other`, i.e. it contains it.
    ///
    /// NOTE: This returns true if `self == other`, see [Self::is_proper_ancestor] if you do not
    /// want this behavior.
    pub fn is_ancestor(&self, other: &RegionRef) -> bool {
        let this = self.as_region_ref();
        &this == other || Self::is_proper_ancestor_of(&this, other)
    }

    /// Returns true if this region is a proper ancestor of `other`, i.e. `other` is contained by it
    ///
    /// NOTE: This returns false if `self == other`, see [Self::is_ancestor] if you do not want this
    /// behavior.
    pub fn is_proper_ancestor(&self, other: &RegionRef) -> bool {
        let this = self.as_region_ref();
        Self::is_proper_ancestor_of(&this, other)
    }

    fn is_proper_ancestor_of(this: &RegionRef, other: &RegionRef) -> bool {
        if this == other {
            return false;
        }

        let mut parent = other.borrow().parent_region();
        while let Some(parent_region) = parent.take() {
            if this == &parent_region {
                return true;
            }
            parent = parent_region.borrow().parent_region();
        }

        false
    }

    /// Returns true if this region may be a graph region without SSA dominance
    pub fn may_be_graph_region(&self) -> bool {
        if let Some(owner) = self.parent() {
            owner
                .borrow()
                .as_trait::<dyn RegionKindInterface>()
                .is_some_and(|rki| rki.has_graph_regions())
        } else {
            true
        }
    }

    /// Returns true if this region has only one block
    pub fn has_one_block(&self) -> bool {
        !self.body.is_empty()
            && BlockRef::ptr_eq(
                &self.body.front().as_pointer().unwrap(),
                &self.body.back().as_pointer().unwrap(),
            )
    }

    /// Get the defining [Operation] for this region, if the region is attached to one.
    pub fn parent(&self) -> Option<OperationRef> {
        self.as_region_ref().parent()
    }

    /// Get the region which contains the parent operation of this region, if there is one.
    pub fn parent_region(&self) -> Option<RegionRef> {
        self.parent().and_then(|op| op.grandparent())
    }
}

/// Region Graph
impl Region {
    /// Traverse the region graph starting at `begin`.
    ///
    /// The traversal is interrupted if `stop` evaluates to `true` for a successor region. The
    /// first argument given to the callback is the successor region, and the second argument is
    /// the set of successor regions visited thus far.
    ///
    /// Returns `true` if traversal was interrupted, otherwise false.
    ///
    /// # Panics
    ///
    /// This function will panic if `begin` is a region that does not belong to an operation, or
    /// if that operation does not implement `RegionBranchOpInterface`.
    pub fn traverse_region_graph<F>(begin: &Self, mut stop: F) -> bool
    where
        F: FnMut(&Region, &SmallSet<RegionRef, 4>) -> bool,
    {
        let op = begin.parent().expect("cannot traverse an orphaned region");
        let op = op.borrow();
        let branch = op
            .as_trait::<dyn RegionBranchOpInterface>()
            .expect("expected parent op to implement RegionBranchOpInterface");

        let mut visited = SmallSet::<RegionRef, 4>::default();
        visited.insert(begin.as_region_ref());

        let mut worklist = SmallVec::<[RegionRef; 4]>::default();
        for successor in
            branch.get_successor_regions(RegionBranchPoint::Child(begin.as_region_ref()))
        {
            if let Some(successor) = successor.into_successor() {
                worklist.push(successor);
            }
        }

        while let Some(next_region_ref) = worklist.pop() {
            let next_region = next_region_ref.borrow();

            if stop(&next_region, &visited) {
                return true;
            }

            if visited.insert(next_region_ref) {
                for successor in
                    branch.get_successor_regions(RegionBranchPoint::Child(next_region_ref))
                {
                    if let Some(successor) = successor.into_successor() {
                        worklist.push(successor);
                    }
                }
            }
        }

        false
    }

    /// Returns true if `self` is reachable from `begin` in the region graph of the containing
    /// operation, which must implement the `RegionBranchOpInterface` trait.
    pub fn is_reachable_from(&self, begin: &Region) -> bool {
        assert_eq!(self.parent(), begin.parent(), "expected both regions to belong to the same op");
        // We interrupted the traversal if we find `self` in the region graph
        Self::traverse_region_graph(begin, |region, _| core::ptr::addr_eq(self, region))
    }

    /// Returns true if `self` is reachable from itself in the region graph of the containing
    /// operation, which must implement the `RegionBranchOpInterface` trait.
    ///
    /// The implication of this returning `true`, is that the region graph contains a loop, and
    /// `self` participates in that loop.
    pub fn is_repetitive_region(&self) -> bool {
        Self::traverse_region_graph(self, |region, _| core::ptr::addr_eq(self, region))
    }

    /// Returns a vector of regions in the region graph rooted at `begin`, following a post-order
    /// traversal of the graph, i.e. successors appear before their predecessors.
    ///
    /// NOTE: Backedges encountered during the traversal are ignored.
    ///
    /// Like [Self::traverse_region_graph], this requires the parent op to implement
    /// [RegionBranchOpInterface].
    pub fn postorder_region_graph(begin: &Self) -> SmallVec<[RegionRef; 4]> {
        struct RegionNode {
            region: RegionRef,
            children: SmallVec<[RegionRef; 2]>,
        }
        impl RegionNode {
            pub fn new(region: RegionRef, branch: &dyn RegionBranchOpInterface) -> Self {
                // Collect unvisited children
                let children = branch
                    .get_successor_regions(RegionBranchPoint::Child(region))
                    .filter_map(|s| s.into_successor())
                    .collect();
                Self { region, children }
            }
        }

        let op = begin.parent().expect("cannot traverse an orphaned region");
        let op = op.borrow();
        let branch = op
            .as_trait::<dyn RegionBranchOpInterface>()
            .expect("expected parent op to implement RegionBranchOpInterface");

        let mut postorder = SmallVec::<[RegionRef; 4]>::default();
        let mut visited = SmallSet::<RegionRef, 4>::default();
        let mut worklist = SmallVec::<[(RegionNode, usize); 4]>::default();

        let root = begin.as_region_ref();
        visited.insert(root);
        let root = RegionNode::new(root, branch);
        worklist.push((root, 0));

        while let Some((node, child_index)) = worklist.last_mut() {
            // If we visited all of the children of this node, "recurse" back up the stack
            if *child_index >= node.children.len() {
                postorder.push(node.region);
                worklist.pop();
            } else {
                // Otherwise, recursively visit the given child
                let index = *child_index;
                *child_index += 1;
                let child = RegionNode::new(node.children[index], branch);
                if worklist.iter().any(|(node, _)| node.region == child.region) {
                    // `child` forms a backedge to a node we're still visiting, so ignore it
                    continue;
                } else if visited.insert(child.region) {
                    worklist.push((child, 0));
                }
            }
        }

        postorder
    }

    /// Returns a vector of regions in the region graph rooted at `root`, following a post-order
    /// traversal of the graph, i.e. successors appear before their predecessors.
    ///
    /// NOTE: Backedges encountered during the traversal are ignored.
    pub fn postorder_region_graph_for(
        root: &dyn RegionBranchOpInterface,
    ) -> SmallVec<[RegionRef; 4]> {
        struct RegionNode {
            region: RegionRef,
            children: SmallVec<[RegionRef; 2]>,
        }
        impl RegionNode {
            pub fn new(region: RegionRef, branch: &dyn RegionBranchOpInterface) -> Self {
                // Collect unvisited children
                let children = branch
                    .get_successor_regions(RegionBranchPoint::Child(region))
                    .filter_map(|s| s.into_successor())
                    .collect();
                Self { region, children }
            }
        }

        let mut postorder = SmallVec::<[RegionRef; 4]>::default();
        let mut visited = SmallSet::<RegionRef, 4>::default();
        let mut worklist = SmallVec::<[(RegionNode, usize); 4]>::default();

        for succ in root.get_successor_regions(RegionBranchPoint::Parent) {
            let Some(region) = succ.into_successor() else {
                continue;
            };

            if visited.insert(region) {
                worklist.push((RegionNode::new(region, root), 0));
            }
        }

        while let Some((node, child_index)) = worklist.last_mut() {
            // If we visited all of the children of this node, "recurse" back up the stack
            if *child_index >= node.children.len() {
                postorder.push(node.region);
                worklist.pop();
            } else {
                // Otherwise, recursively visit the given child
                let index = *child_index;
                *child_index += 1;
                let child = RegionNode::new(node.children[index], root);
                if worklist.iter().any(|(node, _)| node.region == child.region) {
                    // `child` forms a backedge to a node we're still visiting, so ignore it
                    continue;
                } else if visited.insert(child.region) {
                    worklist.push((child, 0));
                }
            }
        }

        postorder
    }
}

/// Mutation
impl Region {
    /// Push `block` to the start of this region
    #[inline]
    pub fn push_front(&mut self, block: BlockRef) {
        self.body.push_front(block);
    }

    /// Push `block` to the end of this region
    #[inline]
    pub fn push_back(&mut self, block: BlockRef) {
        self.body.push_back(block);
    }

    pub fn take_body(&mut self, mut from_region: RegionRef) {
        self.drop_all_references();
        self.body.clear();

        // Take blocks from `from_region`, update the parent of all the blocks, then splice to the
        // end of this region's body
        let blocks = from_region.borrow_mut().body_mut().take();
        self.body.back_mut().splice_after(blocks);
    }

    /// Drop all operand uses from operations within this region, which is an essential step
    /// in breaking cyclic dependencies between references when they are to be deleted.
    pub fn drop_all_references(&mut self) {
        let mut cursor = self.body_mut().front_mut();
        while let Some(mut op) = cursor.as_pointer() {
            op.borrow_mut().drop_all_references();
            cursor.move_next();
        }
    }
}

/// Values
impl Region {
    /// Check if every value in `values` is defined above this region, i.e. they are defined in a
    /// region which is a proper ancestor of `self`.
    pub fn values_are_defined_above(&self, values: &[ValueRef]) -> bool {
        let this = self.as_region_ref();
        for value in values {
            if !value
                .borrow()
                .parent_region()
                .is_some_and(|value_region| Self::is_proper_ancestor_of(&value_region, &this))
            {
                return false;
            }
        }
        true
    }

    /// Replace all uses of `value` with `replacement`, within this region.
    pub fn replace_all_uses_in_region_with(&mut self, _value: ValueRef, _replacement: ValueRef) {
        todo!("RegionUtils.h")
    }

    /// Visit each use of a value in this region (and its descendants), where that value was defined
    /// in an ancestor of `limit`.
    pub fn visit_used_values_defined_above<F>(&self, _limit: &RegionRef, _callback: F)
    where
        F: FnMut(OpOperand),
    {
        todo!("RegionUtils.h")
    }

    /// Visit each use of a value in any of the provided regions (or their descendants), where that
    /// value was defined in an ancestor of that region.
    pub fn visit_used_values_defined_above_any<F>(_regions: &[RegionRef], _callback: F)
    where
        F: FnMut(OpOperand),
    {
        todo!("RegionUtils.h")
    }

    /// Return a vector of values used in this region (and its descendants), and defined in an
    /// ancestor of the `limit` region.
    pub fn get_used_values_defined_above(&self, _limit: &RegionRef) -> SmallVec<[ValueRef; 1]> {
        todo!("RegionUtils.h")
    }

    /// Return a vector of values used in any of the provided regions, but defined in an ancestor.
    pub fn get_used_values_defined_above_any(_regions: &[RegionRef]) -> SmallVec<[ValueRef; 1]> {
        todo!("RegionUtils.h")
    }

    /// Make this region isolated from above.
    ///
    /// * Capture the values that are defined above the region and used within it.
    /// * Append block arguments to the entry block that represent each captured value.
    /// * Replace all uses of the captured values within the region, with the new block arguments
    /// * `clone_into_region` is called with the defining op of a captured value. If it returns
    ///   true, it indicates that the op needs to be cloned into the region. As a result, the
    ///   operands of that operation become part of the captured value set (unless the operations
    ///   that define the operand values themselves are to be cloned). The cloned operations are
    ///   added to the entry block of the region.
    ///
    /// Returns the set of captured values.
    pub fn make_isolated_from_above<R, F>(
        &mut self,
        _rewriter: &mut R,
        _clone_into_region: F,
    ) -> SmallVec<[ValueRef; 1]>
    where
        R: crate::Rewriter,
        F: Fn(&Operation) -> bool,
    {
        todo!("RegionUtils.h")
    }
}

/// Queries
impl Region {
    pub fn find_common_ancestor(ops: &[OperationRef]) -> Option<RegionRef> {
        use bitvec::prelude::*;

        match ops.len() {
            0 => None,
            1 => unsafe { ops.get_unchecked(0) }.borrow().parent_region(),
            num_ops => {
                let (first, rest) = unsafe { ops.split_first().unwrap_unchecked() };
                let mut region = first.borrow().parent_region();
                let mut remaining_ops = bitvec![1; num_ops - 1];
                while let Some(r) = region.take() {
                    while let Some(index) = remaining_ops.first_one() {
                        // Is this op contained in `region`?
                        if r.borrow().find_ancestor_op(rest[index]).is_some() {
                            unsafe {
                                remaining_ops.set_unchecked(index, false);
                            }
                        }
                    }
                    if remaining_ops.not_any() {
                        break;
                    }
                    region = r.borrow().parent_region();
                }
                region
            }
        }
    }

    /// Returns `block` if `block` lies in this region, or otherwise finds the ancestor of `block`
    /// that lies in this region.
    ///
    /// Returns `None` if the latter fails.
    pub fn find_ancestor_block(&self, block: BlockRef) -> Option<BlockRef> {
        let this = self.as_region_ref();
        let mut current = Some(block);
        while let Some(current_block) = current.take() {
            let parent = current_block.parent()?;
            if parent == this {
                return Some(current_block);
            }
            current = parent.grandparent();
        }
        current
    }

    /// Returns `op` if `op` lies in this region, or otherwise finds the ancestor of `op` that lies
    /// in this region.
    ///
    /// Returns `None` if the latter fails.
    pub fn find_ancestor_op(&self, op: OperationRef) -> Option<OperationRef> {
        let this = self.as_region_ref();
        let mut current = Some(op);
        while let Some(current_op) = current.take() {
            let parent = current_op.borrow().parent_region()?;
            if parent == this {
                return Some(current_op);
            }
            current = parent.parent();
        }
        current
    }
}

/// Transforms
impl Region {
    /// Run a set of structural simplifications over the regions in `regions`.
    ///
    /// This includes transformations like unreachable block elimination, dead argument elimination,
    /// as well as some other DCE.
    ///
    /// This function returns `Ok` if any of the regions were simplified, `Err` otherwise.
    ///
    /// The provided rewriter is used to notify callers of operation and block deletion.
    ///
    /// The provided [RegionSimplificationLevel] will be used to determine whether to apply more
    /// aggressive simplifications, namely block merging. Note that when block merging is enabled,
    /// this can lead to merged blocks with extra arguments.
    pub fn simplify_all(
        regions: &[RegionRef],
        rewriter: &mut dyn crate::Rewriter,
        simplification_level: RegionSimplificationLevel,
    ) -> Result<(), RegionTransformFailed> {
        let merge_blocks = matches!(simplification_level, RegionSimplificationLevel::Aggressive);

        log::debug!("running region simplification on {} regions", regions.len());
        log::debug!("  simplification level = {simplification_level:?}");
        log::debug!("  merge_blocks         = {merge_blocks}");

        let eliminated_blocks = Self::erase_unreachable_blocks(regions, rewriter).is_ok();
        let eliminated_ops_or_args = Self::dead_code_elimination(regions, rewriter).is_ok();

        let mut merged_identical_blocks = false;
        let mut dropped_redundant_arguments = false;
        if merge_blocks {
            merged_identical_blocks = Self::merge_identical_blocks(regions, rewriter).is_ok();
            dropped_redundant_arguments = Self::drop_redundant_arguments(regions, rewriter).is_ok();
        }

        if eliminated_blocks
            || eliminated_ops_or_args
            || merged_identical_blocks
            || dropped_redundant_arguments
        {
            Ok(())
        } else {
            Err(RegionTransformFailed)
        }
    }
}

/// Printing
impl Region {
    pub fn print(&self, flags: &OpPrintingFlags) -> crate::formatter::Document {
        use crate::formatter::PrettyPrint;

        let printer = RegionPrinter {
            region: self,
            flags,
        };
        printer.render()
    }
}

struct RegionPrinter<'a> {
    region: &'a Region,
    flags: &'a OpPrintingFlags,
}

impl crate::formatter::PrettyPrint for RegionPrinter<'_> {
    fn render(&self) -> crate::formatter::Document {
        use crate::formatter::*;

        if self.region.is_empty() {
            return const_text("{ }");
        }

        let is_parent_op_single_block_single_region = self.region.parent().is_some_and(|op| {
            let op = op.borrow();
            op.implements::<dyn SingleBlock>() && op.implements::<dyn SingleRegion>()
        });
        self.region.body.iter().fold(Document::Empty, |acc, block| {
            if acc.is_empty() {
                if is_parent_op_single_block_single_region || !self.flags.print_entry_block_headers
                {
                    const_text("{") + indent(4, nl() + block.print(self.flags))
                } else {
                    const_text("{") + nl() + block.print(self.flags)
                }
            } else {
                acc + nl() + block.print(self.flags)
            }
        }) + nl()
            + const_text("}")
    }
}

impl crate::formatter::PrettyPrint for Region {
    fn render(&self) -> crate::formatter::Document {
        let flags = OpPrintingFlags::default();

        self.print(&flags)
    }
}
