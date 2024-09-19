use core::fmt;

use super::*;

pub type BlockRef = UnsafeIntrusiveEntityRef<Block>;
/// An intrusive, doubly-linked list of [Block]
pub type BlockList = EntityList<Block>;
pub type BlockCursor<'a> = EntityCursor<'a, Block>;
pub type BlockCursorMut<'a> = EntityCursorMut<'a, Block>;

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

    fn is_used(&self) -> bool {
        !self.uses.is_empty()
    }

    #[inline(always)]
    fn uses(&self) -> &BlockOperandList {
        &self.uses
    }

    #[inline(always)]
    fn uses_mut(&mut self) -> &mut BlockOperandList {
        &mut self.uses
    }

    fn iter_uses(&self) -> BlockOperandIter<'_> {
        self.uses.iter()
    }

    fn first_use(&self) -> BlockOperandCursor<'_> {
        self.uses.front()
    }

    fn first_use_mut(&mut self) -> BlockOperandCursorMut<'_> {
        self.uses.front_mut()
    }

    fn insert_use(&mut self, user: BlockOperandRef) {
        self.uses.push_back(user);
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
}

pub type BlockOperandRef = UnsafeIntrusiveEntityRef<BlockOperand>;
/// An intrusive, doubly-linked list of [BlockOperand]
pub type BlockOperandList = EntityList<BlockOperand>;
pub type BlockOperandCursor<'a> = EntityCursor<'a, BlockOperand>;
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
