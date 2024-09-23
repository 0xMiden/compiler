use core::fmt;

use crate::{BlockOperandRef, OpOperand};

/// TODO:
///
/// * Replace usage of OpSuccessor with BlockOperand
/// * Store OpSuccessor operands in OpOperandStorage in groups per BlockOperand

/// An [OpSuccessor] is a BlockOperand + OpOperands for that block, attached to an Operation
#[derive(Clone)]
pub struct OpSuccessor {
    pub block: BlockOperandRef,
    pub args: smallvec::SmallVec<[OpOperand; 1]>,
}
impl fmt::Debug for OpSuccessor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OpSuccessor")
            .field("block", &self.block.borrow().block_id())
            .field("args", &self.args)
            .finish()
    }
}
