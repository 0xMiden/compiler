use core::fmt;

use smallvec::SmallVec;

use super::*;

/// A pointer to a [Block]
pub type BlockRef = UnsafeIntrusiveEntityRef<Block>;
/// An intrusive, doubly-linked list of [Block]
pub type BlockList = EntityList<Block>;
/// A cursor into a [BlockList]
pub type BlockCursor<'a> = EntityCursor<'a, Block>;
/// A mutable cursor into a [BlockList]
pub type BlockCursorMut<'a> = EntityCursorMut<'a, Block>;
/// An iterator over blocks produced by a depth-first, pre-order visit of the CFG
pub type PreOrderBlockIter = cfg::PreOrderIter<BlockRef>;
/// An iterator over blocks produced by a depth-first, post-order visit of the CFG
pub type PostOrderBlockIter = cfg::PostOrderIter<BlockRef>;

/// The unique identifier for a [Block]
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct BlockId(u32);
impl BlockId {
    pub const fn from_u32(id: u32) -> Self {
        Self(id)
    }

    pub const fn as_u32(&self) -> u32 {
        self.0
    }
}
impl EntityId for BlockId {
    #[inline(always)]
    fn as_usize(&self) -> usize {
        self.0 as usize
    }
}
impl fmt::Debug for BlockId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "block{}", &self.0)
    }
}
impl fmt::Display for BlockId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "block{}", &self.0)
    }
}

/// Represents a basic block in the IR.
///
/// Basic blocks are used in SSA regions to provide the structure of the control-flow graph.
/// Operations within a basic block appear in the order they will be executed.
///
/// A block must have a [traits::Terminator], an operation which transfers control to another block
/// in the same region, or out of the containing operation (e.g. returning from a function).
///
/// Blocks have _predecessors_ and _successors_, representing the inbound and outbound edges
/// (respectively) formed by operations that transfer control between blocks. A block can have
/// zero or more predecessors and/or successors. A well-formed region will generally only have a
/// single block (the entry block) with no predecessors (i.e. no unreachable blocks), and no blocks
/// with both multiple predecessors _and_ multiple successors (i.e. no critical edges). It is valid
/// to have both unreachable blocks and critical edges in the IR, but they must be removed during
/// the course of compilation.
pub struct Block {
    /// The unique id of this block
    id: BlockId,
    /// Flag that indicates whether the ops in this block have a valid ordering
    valid_op_ordering: bool,
    /// The set of uses of this block
    uses: BlockOperandList,
    /// The region this block is attached to.
    ///
    /// This will always be set if this block is attached to a region
    region: Option<RegionRef>,
    /// The list of [Operation]s that comprise this block
    body: OpList,
    /// The parameter list for this block
    arguments: Vec<BlockArgumentRef>,
}
impl fmt::Debug for Block {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Block")
            .field("id", &self.id)
            .field_with("region", |f| match self.region.as_ref() {
                None => f.write_str("None"),
                Some(r) => write!(f, "Some({r:p})"),
            })
            .field_with("arguments", |f| {
                let mut list = f.debug_list();
                for arg in self.arguments.iter() {
                    list.entry_with(|f| f.write_fmt(format_args!("{}", &arg.borrow())));
                }
                list.finish()
            })
            .finish_non_exhaustive()
    }
}

impl Spanned for Block {
    fn span(&self) -> SourceSpan {
        self.body.front().get().map(|op| op.span()).unwrap_or_default()
    }
}

impl Entity for Block {}

impl EntityWithId for Block {
    type Id = BlockId;

    fn id(&self) -> Self::Id {
        self.id
    }
}

impl EntityWithParent for Block {
    type Parent = Region;

    fn on_inserted_into_parent(
        mut this: UnsafeIntrusiveEntityRef<Self>,
        parent: UnsafeIntrusiveEntityRef<Self::Parent>,
    ) {
        this.borrow_mut().region = Some(parent);
    }

    fn on_removed_from_parent(
        mut this: UnsafeIntrusiveEntityRef<Self>,
        _parent: UnsafeIntrusiveEntityRef<Self::Parent>,
    ) {
        this.borrow_mut().region = None;
    }

    fn on_transfered_to_new_parent(
        _from: UnsafeIntrusiveEntityRef<Self::Parent>,
        to: UnsafeIntrusiveEntityRef<Self::Parent>,
        transferred: impl IntoIterator<Item = UnsafeIntrusiveEntityRef<Self>>,
    ) {
        for mut transferred_block in transferred {
            transferred_block.borrow_mut().region = Some(to.clone());
        }
    }
}

impl Usable for Block {
    type Use = BlockOperand;

    #[inline(always)]
    fn uses(&self) -> &BlockOperandList {
        &self.uses
    }

    #[inline(always)]
    fn uses_mut(&mut self) -> &mut BlockOperandList {
        &mut self.uses
    }
}

impl cfg::Graph for Block {
    type ChildEdgeIter = BlockSuccessorEdgesIter;
    type ChildIter = BlockSuccessorIter;
    type Edge = BlockOperandRef;
    type Node = BlockRef;

    fn size(&self) -> usize {
        if let Some(term) = self.terminator() {
            term.borrow().num_successors()
        } else {
            0
        }
    }

    fn entry_node(&self) -> Self::Node {
        self.as_block_ref()
    }

    fn children(parent: Self::Node) -> Self::ChildIter {
        BlockSuccessorIter::new(parent)
    }

    fn children_edges(parent: Self::Node) -> Self::ChildEdgeIter {
        BlockSuccessorEdgesIter::new(parent)
    }

    fn edge_dest(edge: Self::Edge) -> Self::Node {
        edge.borrow().block.clone()
    }
}

impl<'a> cfg::InvertibleGraph for &'a Block {
    type Inverse = cfg::Inverse<&'a Block>;
    type InvertibleChildEdgeIter = BlockPredecessorEdgesIter;
    type InvertibleChildIter = BlockPredecessorIter;

    fn inverse(self) -> Self::Inverse {
        cfg::Inverse::new(self)
    }

    fn inverse_children(parent: Self::Node) -> Self::InvertibleChildIter {
        BlockPredecessorIter::new(parent)
    }

    fn inverse_children_edges(parent: Self::Node) -> Self::InvertibleChildEdgeIter {
        BlockPredecessorEdgesIter::new(parent)
    }
}

impl cfg::Graph for BlockRef {
    type ChildEdgeIter = BlockSuccessorEdgesIter;
    type ChildIter = BlockSuccessorIter;
    type Edge = BlockOperandRef;
    type Node = BlockRef;

    fn size(&self) -> usize {
        if let Some(term) = self.borrow().terminator() {
            term.borrow().num_successors()
        } else {
            0
        }
    }

    fn entry_node(&self) -> Self::Node {
        self.clone()
    }

    fn children(parent: Self::Node) -> Self::ChildIter {
        BlockSuccessorIter::new(parent)
    }

    fn children_edges(parent: Self::Node) -> Self::ChildEdgeIter {
        BlockSuccessorEdgesIter::new(parent)
    }

    fn edge_dest(edge: Self::Edge) -> Self::Node {
        edge.borrow().block.clone()
    }
}

impl cfg::InvertibleGraph for BlockRef {
    type Inverse = cfg::Inverse<Self>;
    type InvertibleChildEdgeIter = BlockPredecessorEdgesIter;
    type InvertibleChildIter = BlockPredecessorIter;

    fn inverse(self) -> Self::Inverse {
        cfg::Inverse::new(self)
    }

    fn inverse_children(parent: Self::Node) -> Self::InvertibleChildIter {
        BlockPredecessorIter::new(parent)
    }

    fn inverse_children_edges(parent: Self::Node) -> Self::InvertibleChildEdgeIter {
        BlockPredecessorEdgesIter::new(parent)
    }
}

#[doc(hidden)]
pub struct BlockSuccessorIter {
    iter: BlockSuccessorEdgesIter,
}
impl BlockSuccessorIter {
    pub fn new(parent: BlockRef) -> Self {
        Self {
            iter: BlockSuccessorEdgesIter::new(parent),
        }
    }
}
impl ExactSizeIterator for BlockSuccessorIter {
    #[inline]
    fn len(&self) -> usize {
        self.iter.len()
    }

    #[inline]
    fn is_empty(&self) -> bool {
        self.iter.is_empty()
    }
}
impl Iterator for BlockSuccessorIter {
    type Item = BlockRef;

    fn next(&mut self) -> Option<Self::Item> {
        self.iter.next().map(|bo| bo.borrow().block.clone())
    }

    #[inline]
    fn collect<B: FromIterator<Self::Item>>(self) -> B
    where
        Self: Sized,
    {
        let Some(terminator) = self.iter.terminator.as_ref() else {
            return B::from_iter([]);
        };
        let terminator = terminator.borrow();
        let successors = terminator.successors();
        B::from_iter(
            successors.all().as_slice()[self.iter.index..self.iter.num_successors]
                .iter()
                .map(|succ| succ.block.borrow().block.clone()),
        )
    }

    fn collect_into<E: Extend<Self::Item>>(self, collection: &mut E) -> &mut E
    where
        Self: Sized,
    {
        let Some(terminator) = self.iter.terminator.as_ref() else {
            return collection;
        };
        let terminator = terminator.borrow();
        let successors = terminator.successors();
        collection.extend(
            successors.all().as_slice()[self.iter.index..self.iter.num_successors]
                .iter()
                .map(|succ| succ.block.borrow().block.clone()),
        );
        collection
    }
}

#[doc(hidden)]
pub struct BlockSuccessorEdgesIter {
    terminator: Option<OperationRef>,
    num_successors: usize,
    index: usize,
}
impl BlockSuccessorEdgesIter {
    pub fn new(parent: BlockRef) -> Self {
        let terminator = parent.borrow().terminator();
        let num_successors = terminator.as_ref().map(|t| t.borrow().num_successors()).unwrap_or(0);
        Self {
            terminator,
            num_successors,
            index: 0,
        }
    }
}
impl ExactSizeIterator for BlockSuccessorEdgesIter {
    #[inline]
    fn len(&self) -> usize {
        self.num_successors.saturating_sub(self.index)
    }

    #[inline]
    fn is_empty(&self) -> bool {
        self.index >= self.num_successors
    }
}
impl Iterator for BlockSuccessorEdgesIter {
    type Item = BlockOperandRef;

    fn next(&mut self) -> Option<Self::Item> {
        if self.index >= self.num_successors {
            return None;
        }

        // SAFETY: We'll never have a none terminator if we have non-zero number of successors
        let terminator = unsafe { self.terminator.as_ref().unwrap_unchecked() };
        let index = self.index;
        self.index += 1;
        Some(terminator.borrow().successor(index).dest.clone())
    }

    fn collect<B: FromIterator<Self::Item>>(self) -> B
    where
        Self: Sized,
    {
        let Some(terminator) = self.terminator.as_ref() else {
            return B::from_iter([]);
        };
        let terminator = terminator.borrow();
        let successors = terminator.successors();
        B::from_iter(
            successors.all().as_slice()[self.index..self.num_successors]
                .iter()
                .map(|succ| succ.block.clone()),
        )
    }

    fn collect_into<E: Extend<Self::Item>>(self, collection: &mut E) -> &mut E
    where
        Self: Sized,
    {
        let Some(terminator) = self.terminator.as_ref() else {
            return collection;
        };
        let terminator = terminator.borrow();
        let successors = terminator.successors();
        collection.extend(
            successors.all().as_slice()[self.index..self.num_successors]
                .iter()
                .map(|succ| succ.block.clone()),
        );
        collection
    }
}

#[doc(hidden)]
pub struct BlockPredecessorIter {
    preds: SmallVec<[BlockRef; 4]>,
    index: usize,
}
impl BlockPredecessorIter {
    pub fn new(child: BlockRef) -> Self {
        let preds = child.borrow().predecessors().map(|bo| bo.block.clone()).collect();
        Self { preds, index: 0 }
    }

    #[inline(always)]
    pub fn into_inner(self) -> SmallVec<[BlockRef; 4]> {
        self.preds
    }
}
impl ExactSizeIterator for BlockPredecessorIter {
    #[inline]
    fn len(&self) -> usize {
        self.preds.len().saturating_sub(self.index)
    }

    #[inline]
    fn is_empty(&self) -> bool {
        self.index >= self.preds.len()
    }
}
impl Iterator for BlockPredecessorIter {
    type Item = BlockRef;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        if self.is_empty() {
            return None;
        }
        let index = self.index;
        self.index += 1;
        Some(self.preds[index].clone())
    }

    fn collect<B: FromIterator<Self::Item>>(self) -> B
    where
        Self: Sized,
    {
        B::from_iter(self.preds)
    }

    fn collect_into<E: Extend<Self::Item>>(self, collection: &mut E) -> &mut E
    where
        Self: Sized,
    {
        collection.extend(self.preds);
        collection
    }
}

#[doc(hidden)]
pub struct BlockPredecessorEdgesIter {
    preds: SmallVec<[BlockOperandRef; 4]>,
    index: usize,
}
impl BlockPredecessorEdgesIter {
    pub fn new(child: BlockRef) -> Self {
        let preds = child
            .borrow()
            .predecessors()
            .map(|bo| unsafe { BlockOperandRef::from_raw(&*bo) })
            .collect();
        Self { preds, index: 0 }
    }
}
impl ExactSizeIterator for BlockPredecessorEdgesIter {
    #[inline]
    fn len(&self) -> usize {
        self.preds.len().saturating_sub(self.index)
    }

    #[inline]
    fn is_empty(&self) -> bool {
        self.index >= self.preds.len()
    }
}
impl Iterator for BlockPredecessorEdgesIter {
    type Item = BlockOperandRef;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        if self.is_empty() {
            return None;
        }
        let index = self.index;
        self.index += 1;
        Some(self.preds[index].clone())
    }

    fn collect<B: FromIterator<Self::Item>>(self) -> B
    where
        Self: Sized,
    {
        B::from_iter(self.preds)
    }

    fn collect_into<E: Extend<Self::Item>>(self, collection: &mut E) -> &mut E
    where
        Self: Sized,
    {
        collection.extend(self.preds);
        collection
    }
}

impl Block {
    pub fn new(id: BlockId) -> Self {
        Self {
            id,
            valid_op_ordering: true,
            uses: Default::default(),
            region: None,
            body: Default::default(),
            arguments: Default::default(),
        }
    }

    #[inline]
    pub fn as_block_ref(&self) -> BlockRef {
        unsafe { BlockRef::from_raw(self) }
    }

    /// Get a handle to the containing [Region] of this block, if it is attached to one
    pub fn parent(&self) -> Option<RegionRef> {
        self.region.clone()
    }

    /// Get a handle to the containing [Operation] of this block, if it is attached to one
    pub fn parent_op(&self) -> Option<OperationRef> {
        self.region.as_ref().and_then(|region| region.borrow().parent())
    }

    /// Get a handle to the ancestor [Block] of this block, if one is present
    pub fn parent_block(&self) -> Option<BlockRef> {
        self.parent_op().and_then(|op| op.borrow().parent())
    }

    /// Returns true if this block is the entry block for its containing region
    pub fn is_entry_block(&self) -> bool {
        if let Some(parent) = self.region.as_ref().map(|r| r.borrow()) {
            core::ptr::addr_eq(&*parent.entry(), self)
        } else {
            false
        }
    }

    /// Get the first operation in the body of this block
    #[inline]
    pub fn front(&self) -> Option<OperationRef> {
        self.body.front().as_pointer()
    }

    /// Get the last operation in the body of this block
    #[inline]
    pub fn back(&self) -> Option<OperationRef> {
        self.body.back().as_pointer()
    }

    /// Get the list of [Operation] comprising the body of this block
    #[inline(always)]
    pub fn body(&self) -> &OpList {
        &self.body
    }

    /// Get a mutable reference to the list of [Operation] comprising the body of this block
    #[inline(always)]
    pub fn body_mut(&mut self) -> &mut OpList {
        &mut self.body
    }
}

/// Arguments
impl Block {
    #[inline]
    pub fn has_arguments(&self) -> bool {
        !self.arguments.is_empty()
    }

    #[inline]
    pub fn num_arguments(&self) -> usize {
        self.arguments.len()
    }

    #[inline(always)]
    pub fn arguments(&self) -> &[BlockArgumentRef] {
        self.arguments.as_slice()
    }

    #[inline(always)]
    pub fn arguments_mut(&mut self) -> &mut Vec<BlockArgumentRef> {
        &mut self.arguments
    }

    #[inline]
    pub fn get_argument(&self, index: usize) -> BlockArgumentRef {
        self.arguments[index].clone()
    }

    /// Erase the block argument at `index`
    ///
    /// Panics if the argument still has uses.
    pub fn erase_argument(&mut self, index: usize) {
        assert!(
            !self.arguments[index].borrow().is_used(),
            "cannot erase block arguments with uses"
        );
        self.arguments.remove(index);
    }

    /// Erase every parameter of this block for which `should_erase` returns true.
    ///
    /// Panics if any argument to be erased still has uses.
    pub fn erase_arguments<F>(&mut self, should_erase: F)
    where
        F: Fn(&BlockArgument) -> bool,
    {
        self.arguments.retain(|arg| {
            let arg = arg.borrow();
            let keep = !should_erase(&arg);
            assert!(keep || !arg.is_used(), "cannot erase block arguments with uses");
            keep
        });
    }
}

/// Placement
impl Block {
    /// Insert this block after `after` in its containing region.
    ///
    /// Panics if this block is already attached to a region, or if `after` is not attached.
    pub fn insert_after(&mut self, after: BlockRef) {
        assert!(
            self.region.is_none(),
            "cannot insert block that is already attached to another region"
        );
        let mut region =
            after.borrow().parent().expect("'after' block is not attached to a region");
        {
            let mut region = region.borrow_mut();
            let region_body = region.body_mut();
            let mut cursor = unsafe { region_body.cursor_mut_from_ptr(after) };
            cursor.insert_after(self.as_block_ref());
        }
        self.region = Some(region);
    }

    /// Insert this block before `before` in its containing region.
    ///
    /// Panics if this block is already attached to a region, or if `before` is not attached.
    pub fn insert_before(&mut self, before: BlockRef) {
        assert!(
            self.region.is_none(),
            "cannot insert block that is already attached to another region"
        );
        let mut region =
            before.borrow().parent().expect("'before' block is not attached to a region");
        {
            let mut region = region.borrow_mut();
            let region_body = region.body_mut();
            let mut cursor = unsafe { region_body.cursor_mut_from_ptr(before) };
            cursor.insert_before(self.as_block_ref());
        }
        self.region = Some(region);
    }

    /// Insert this block at the end of `region`.
    ///
    /// Panics if this block is already attached to a region.
    pub fn insert_at_end(&mut self, mut region: RegionRef) {
        assert!(
            self.region.is_none(),
            "cannot insert block that is already attached to another region"
        );
        {
            let mut region = region.borrow_mut();
            region.body_mut().push_back(self.as_block_ref());
        }
        self.region = Some(region);
    }

    /// Unlink this block from its current region and insert it right before `before`
    pub fn move_before(&mut self, before: BlockRef) {
        self.unlink();
        self.insert_before(before);
    }

    /// Remove this block from its containing region
    fn unlink(&mut self) {
        if let Some(mut region) = self.region.take() {
            let mut region = region.borrow_mut();
            unsafe {
                let mut cursor = region.body_mut().cursor_mut_from_ptr(self.as_block_ref());
                cursor.remove();
            }
        }
    }

    /// Split this block into two blocks before the specified operation
    ///
    /// Note that all operations in the block prior to `before` stay as part of the original block,
    /// and the rest are moved to the new block, including the old terminator. The original block is
    /// thus left without a terminator.
    ///
    /// Returns the newly created block.
    pub fn split_block(&mut self, before: OperationRef) -> BlockRef {
        let this = self.as_block_ref();
        assert!(
            BlockRef::ptr_eq(
                &this,
                &before.borrow().parent().expect("'before' op is not attached to a block")
            ),
            "cannot split block using an operation that does not belong to the block being split"
        );

        // We need the parent op so we can get access to the current Context, but this also tells us
        // that this block is attached to a region and operation.
        let parent = self.parent_op().expect("block is not attached to an operation");
        // Create a new empty block
        let mut new_block = parent.borrow().context().create_block();
        // Insert the block in the same region as `self`, immediately after `self`
        let region = self.region.as_mut().unwrap();
        {
            let mut region_mut = region.borrow_mut();
            let blocks = region_mut.body_mut();
            let mut cursor = unsafe { blocks.cursor_mut_from_ptr(this.clone()) };
            cursor.insert_after(new_block.clone());
        }
        // Split the body of `self` at `before`, and splice everything after `before`, including
        // `before` itself, into the new block we created.
        let mut ops = {
            let mut cursor = unsafe { self.body.cursor_mut_from_ptr(before) };
            cursor.split_before()
        };
        // The split_before method returns the list containing all of the ops before the cursor, but
        // we want the inverse, so we just swap the two lists.
        core::mem::swap(&mut self.body, &mut ops);
        // Visit all of the ops and notify them of the move
        for op in ops.iter() {
            Operation::on_inserted_into_parent(op.as_operation_ref(), new_block.clone());
        }
        new_block.borrow_mut().body = ops;
        new_block
    }

    pub fn clear(&mut self) {
        // Drop all references from within this block
        self.drop_all_references();

        // Drop all operations within this block
        self.body_mut().clear();
    }
}

/// Ancestors
impl Block {
    pub fn is_legal_to_hoist_into(&self) -> bool {
        use crate::traits::{HasSideEffects, ReturnLike};

        // No terminator means the block is under construction, and thus legal to hoist into
        let Some(terminator) = self.terminator() else {
            return true;
        };

        // If the block has no successors, it can never be legal to hoist into it, there is nothing
        // to hoist!
        if self.num_successors() == 0 {
            return false;
        }

        // Instructions should not be hoisted across effectful or return-like terminators. This is
        // typically only exception handling intrinsics, which HIR doesn't really have, but which
        // we may nevertheless want to represent in the future.
        //
        // NOTE: Most return-like terminators would have no successors, but in LLVM, for example,
        // there are instructions like `catch_ret`, which semantically are return-like, but which
        // have a successor block (the landing pad).
        let terminator = terminator.borrow();
        terminator.implements::<dyn HasSideEffects>() || terminator.implements::<dyn ReturnLike>()
    }

    pub fn has_ssa_dominance(&self) -> bool {
        self.parent_op()
            .and_then(|op| {
                op.borrow()
                    .as_trait::<dyn RegionKindInterface>()
                    .map(|rki| rki.has_ssa_dominance())
            })
            .unwrap_or(true)
    }

    /// Walk up the ancestor blocks of `block`, until `f` returns `true` for a block.
    ///
    /// NOTE: `block` is visited before any of its ancestors.
    pub fn traverse_ancestors<F>(block: BlockRef, mut f: F) -> Option<BlockRef>
    where
        F: FnMut(BlockRef) -> bool,
    {
        let mut block = Some(block);
        while let Some(current) = block.take() {
            if f(current.clone()) {
                return Some(current);
            }
            block = current.borrow().parent_block();
        }

        None
    }

    /// Try to get a pair of blocks, starting with the given pair, which live in the same region,
    /// by exploring the relationships of both blocks with respect to their regions.
    ///
    /// The returned block pair will either be the same input blocks, or some combination of those
    /// blocks or their ancestors.
    pub fn get_blocks_in_same_region(a: &BlockRef, b: &BlockRef) -> Option<(BlockRef, BlockRef)> {
        // If both blocks do not live in the same region, we will have to check their parent
        // operations.
        let a_region = a.borrow().parent().unwrap();
        let b_region = b.borrow().parent().unwrap();
        if a_region == b_region {
            return Some((a.clone(), b.clone()));
        }

        // Iterate over all ancestors of `a`, counting the depth of `a`.
        //
        // If one of `a`'s ancestors are in the same region as `b`, then we stop early because we
        // found our nearest common ancestor.
        let mut a_depth = 0;
        let result = Self::traverse_ancestors(a.clone(), |block| {
            a_depth += 1;
            block.borrow().parent().is_some_and(|r| r == b_region)
        });
        if let Some(a) = result {
            return Some((a, b.clone()));
        }

        // Iterate over all ancestors of `b`, counting the depth of `b`.
        //
        // If one of `b`'s ancestors are in the same region as `a`, then we stop early because we
        // found our nearest common ancestor.
        let mut b_depth = 0;
        let result = Self::traverse_ancestors(b.clone(), |block| {
            b_depth += 1;
            block.borrow().parent().is_some_and(|r| r == a_region)
        });
        if let Some(b) = result {
            return Some((a.clone(), b));
        }

        // Otherwise, we found two blocks that are siblings at some level. Walk the deepest one
        // up until we reach the top or find a nearest common ancestor.
        let mut a = Some(a.clone());
        let mut b = Some(b.clone());
        loop {
            use core::cmp::Ordering;

            match a_depth.cmp(&b_depth) {
                Ordering::Greater => {
                    a = a.and_then(|a| a.borrow().parent_block());
                    a_depth -= 1;
                }
                Ordering::Less => {
                    b = b.and_then(|b| b.borrow().parent_block());
                    b_depth -= 1;
                }
                Ordering::Equal => break,
            }
        }

        // If we found something with the same level, then we can march both up at the same time
        // from here on out.
        while let Some(next_a) = a.take() {
            // If they are at the same level, and have the same parent region, then we succeeded.
            let next_a_parent = next_a.borrow().parent();
            let b_parent = b.as_ref().and_then(|b| b.borrow().parent());
            if next_a_parent == b_parent {
                return Some((next_a, b.unwrap()));
            }

            a = next_a_parent
                .and_then(|r| r.borrow().parent())
                .and_then(|op| op.borrow().parent());
            b = b_parent.and_then(|r| r.borrow().parent()).and_then(|op| op.borrow().parent());
        }

        // They don't share a nearest common ancestor, perhaps they are in different modules or
        // something.
        None
    }
}

/// Predecessors and Successors
impl Block {
    /// Returns true if this block has predecessors
    #[inline(always)]
    pub fn has_predecessors(&self) -> bool {
        self.is_used()
    }

    /// Get an iterator over the predecessors of this block
    #[inline(always)]
    pub fn predecessors(&self) -> BlockOperandIter<'_> {
        self.iter_uses()
    }

    /// If this block has exactly one predecessor, return it, otherwise `None`
    ///
    /// NOTE: A predecessor block with multiple edges, e.g. a conditional branch that has this block
    /// as the destination for both true/false branches is _not_ considered a single predecessor by
    /// this function.
    pub fn get_single_predecessor(&self) -> Option<BlockRef> {
        let front = self.uses.front();
        if front.is_null() {
            return None;
        }
        let front = front.as_pointer().unwrap();
        let back = self.uses.back().as_pointer().unwrap();
        if BlockOperandRef::ptr_eq(&front, &back) {
            Some(front.borrow().block.clone())
        } else {
            None
        }
    }

    /// If this block has a unique predecessor, i.e. all incoming edges originate from one block,
    /// return it, otherwise `None`
    pub fn get_unique_predecessor(&self) -> Option<BlockRef> {
        let mut front = self.uses.front();
        let block_operand = front.get()?;
        let block = block_operand.block.clone();
        loop {
            front.move_next();
            if let Some(bo) = front.get() {
                if !BlockRef::ptr_eq(&block, &bo.block) {
                    break None;
                }
            } else {
                break Some(block);
            }
        }
    }

    /// Returns true if this block has any successors
    #[inline]
    pub fn has_successors(&self) -> bool {
        self.num_successors() > 0
    }

    /// Get the number of successors of this block in the CFG
    pub fn num_successors(&self) -> usize {
        self.terminator().map(|op| op.borrow().num_successors()).unwrap_or(0)
    }

    /// Get the `index`th successor of this block's terminator operation
    pub fn get_successor(&self, index: usize) -> BlockRef {
        let op = self.terminator().expect("this block has no terminator");
        op.borrow().successor(index).dest.borrow().block.clone()
    }

    /// This drops all operand uses from operations within this block, which is an essential step in
    /// breaking cyclic dependences between references when they are to be deleted.
    pub fn drop_all_references(&mut self) {
        let mut cursor = self.body.front_mut();
        while let Some(mut op) = cursor.as_pointer() {
            op.borrow_mut().drop_all_references();
            cursor.move_next();
        }
    }

    /// This drops all uses of values defined in this block or in the blocks of nested regions
    /// wherever the uses are located.
    pub fn drop_all_defined_value_uses(&mut self) {
        for arg in self.arguments.iter_mut() {
            let mut arg = arg.borrow_mut();
            arg.uses_mut().clear();
        }
        let mut cursor = self.body.front_mut();
        while let Some(mut op) = cursor.as_pointer() {
            op.borrow_mut().drop_all_defined_value_uses();
            cursor.move_next();
        }
        self.drop_all_uses();
    }

    /// Drop all uses of this block via [BlockOperand]
    #[inline]
    pub fn drop_all_uses(&mut self) {
        self.uses_mut().clear();
    }

    #[inline(always)]
    pub(super) const fn is_op_order_valid(&self) -> bool {
        self.valid_op_ordering
    }

    #[inline(always)]
    pub(super) fn mark_op_order_valid(&mut self) {
        self.valid_op_ordering = true;
    }

    pub(super) fn invalidate_op_order(&mut self) {
        // Validate the current ordering
        assert!(self.verify_op_order());
        self.valid_op_ordering = false;
    }

    /// Returns true if the current operation ordering in this block is valid
    pub(super) fn verify_op_order(&self) -> bool {
        // The order is already known to be invalid
        if !self.valid_op_ordering {
            return false;
        }

        // The order is valid if there are less than 2 operations
        if self.body.is_empty()
            || OperationRef::ptr_eq(
                &self.body.front().as_pointer().unwrap(),
                &self.body.back().as_pointer().unwrap(),
            )
        {
            return true;
        }

        let mut cursor = self.body.front();
        let mut prev = None;
        while let Some(op) = cursor.as_pointer() {
            cursor.move_next();
            if prev.is_none() {
                prev = Some(op);
                continue;
            }

            // The previous operation must have a smaller order index than the next
            let prev_order = prev.take().unwrap().borrow().order();
            let current_order = op.borrow().order().unwrap_or(u32::MAX);
            if prev_order.is_some_and(|o| o >= current_order) {
                return false;
            }
            prev = Some(op);
        }

        true
    }

    /// Get the terminator operation of this block, or `None` if the block does not have one.
    pub fn terminator(&self) -> Option<OperationRef> {
        if !self.has_terminator() {
            None
        } else {
            self.body.back().as_pointer()
        }
    }

    /// Returns true if this block has a terminator
    pub fn has_terminator(&self) -> bool {
        use crate::traits::Terminator;
        !self.body.is_empty()
            && self.body.back().get().is_some_and(|op| op.implements::<dyn Terminator>())
    }
}

pub type BlockOperandRef = UnsafeIntrusiveEntityRef<BlockOperand>;
/// An intrusive, doubly-linked list of [BlockOperand]
pub type BlockOperandList = EntityList<BlockOperand>;
#[allow(unused)]
pub type BlockOperandCursor<'a> = EntityCursor<'a, BlockOperand>;
#[allow(unused)]
pub type BlockOperandCursorMut<'a> = EntityCursorMut<'a, BlockOperand>;
pub type BlockOperandIter<'a> = EntityIter<'a, BlockOperand>;

/// A [BlockOperand] represents a use of a [Block] by an [Operation]
pub struct BlockOperand {
    /// The block value
    pub block: BlockRef,
    /// The owner of this operand, i.e. the operation it is an operand of
    pub owner: OperationRef,
    /// The index of this operand in the set of block operands of the operation
    pub index: u8,
}
impl BlockOperand {
    #[inline]
    pub fn new(block: BlockRef, owner: OperationRef, index: u8) -> Self {
        Self {
            block,
            owner,
            index,
        }
    }

    pub fn block_id(&self) -> BlockId {
        self.block.borrow().id
    }
}
impl fmt::Debug for BlockOperand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BlockOperand")
            .field("block", &self.block.borrow().id())
            .field_with("owner", |f| write!(f, "{:p}", &self.owner))
            .field("index", &self.index)
            .finish()
    }
}
impl StorableEntity for BlockOperand {
    #[inline(always)]
    fn index(&self) -> usize {
        self.index as usize
    }

    unsafe fn set_index(&mut self, index: usize) {
        self.index = index.try_into().expect("too many successors");
    }

    /// Remove this use of `block`
    fn unlink(&mut self) {
        let owner = unsafe { BlockOperandRef::from_raw(self) };
        let mut block = self.block.borrow_mut();
        let uses = block.uses_mut();
        unsafe {
            let mut cursor = uses.cursor_mut_from_ptr(owner);
            cursor.remove();
        }
    }
}
