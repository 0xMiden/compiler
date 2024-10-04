use core::fmt;

use super::*;

/// A pointer to a [Block]
pub type BlockRef = UnsafeIntrusiveEntityRef<Block>;
/// An intrusive, doubly-linked list of [Block]
pub type BlockList = EntityList<Block>;
/// A cursor into a [BlockList]
pub type BlockCursor<'a> = EntityCursor<'a, Block>;
/// A mutable cursor into a [BlockList]
pub type BlockCursorMut<'a> = EntityCursorMut<'a, Block>;

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
            .field("arguments", &self.arguments)
            .finish_non_exhaustive()
    }
}
impl Entity for Block {
    type Id = BlockId;

    fn id(&self) -> Self::Id {
        self.id
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
impl Block {
    pub fn new(id: BlockId) -> Self {
        Self {
            id,
            uses: Default::default(),
            region: None,
            body: Default::default(),
            arguments: Default::default(),
        }
    }

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
            cursor.insert_after(unsafe { BlockRef::from_raw(self) });
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
            cursor.insert_before(unsafe { BlockRef::from_raw(self) });
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
            region.body_mut().push_back(unsafe { BlockRef::from_raw(self) });
        }
        self.region = Some(region);
    }

    /// Get a handle to the containing [Region] of this block, if it is attached to one
    pub fn parent(&self) -> Option<RegionRef> {
        self.region.clone()
    }

    /// Get a handle to the containing [Operation] of this block, if it is attached to one
    pub fn parent_op(&self) -> Option<OperationRef> {
        self.region.as_ref().and_then(|region| region.borrow().parent())
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

    pub fn drop_all_defined_value_uses(&mut self) {
        todo!()
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
