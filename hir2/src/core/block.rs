use super::{
    BlockArgument, EntityCursor, EntityCursorMut, EntityHandle, EntityIter, EntityList, OpList,
    Operation, Usable,
};

/// An intrusive, doubly-linked list of [Block]
pub type BlockList = EntityList<Block>;
pub type BlockCursor<'a> = EntityCursor<'a, Block>;
pub type BlockCursorMut<'a> = EntityCursorMut<'a, Block>;

pub struct Block {
    /// The set of uses of this block
    uses: BlockOperandList,
    /// The region this block is attached to.
    ///
    /// If `link.is_linked() == true`, this will always be set to a valid pointer
    region: Option<EntityHandle<Region>>,
    /// The list of [Operation]s that comprise this block
    ops: OpList,
    /// The parameter list for this block
    arguments: Vec<BlockArgument>,
}
impl Usable for Block {
    type Use = BlockOperand;

    fn is_used(&self) -> bool {
        !self.uses.is_empty()
    }

    fn uses(&self) -> BlockOperandIter<'_> {
        self.uses.iter()
    }

    fn first_use(&self) -> BlockOperandCursor<'_> {
        self.uses.front()
    }

    fn first_use_mut(&mut self) -> BlockOperandCursorMut<'_> {
        self.uses.front_mut()
    }
}
impl Block {
    #[inline(always)]
    pub fn has_predecessors(&self) -> bool {
        self.is_used()
    }

    #[inline(always)]
    pub fn predecessors(&self) -> BlockOperandIter<'_> {
        self.uses()
    }
}

/// An intrusive, doubly-linked list of [BlockOperand]
pub type BlockOperandList = EntityList<BlockOperand>;
pub type BlockOperandCursor<'a> = EntityCursor<'a, BlockOperand>;
pub type BlockOperandCursorMut<'a> = EntityCursorMut<'a, BlockOperand>;
pub type BlockOperandIter<'a> = EntityIter<'a, BlockOperand>;

/// A [BlockOperand] represents a use of a [Block] by an [Operation]
pub struct BlockOperand {
    /// The block value
    pub block: Block,
    /// The owner of this operand, i.e. the operation it is an operand of
    pub owner: EntityHandle<Operation>,
    /// The index of this operand in the set of block operands of the operation
    pub index: u8,
}
impl BlockOperand {
    #[inline]
    pub fn new(block: Block, owner: EntityHandle<Operation>, index: u8) -> Self {
        Self {
            block,
            owner,
            index,
        }
    }
}
