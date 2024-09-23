use core::fmt;

use crate::{BlockRef, OperationRef};

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Insert {
    Before,
    After,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct InsertionPoint {
    pub at: ProgramPoint,
    pub action: Insert,
}
impl InsertionPoint {
    #[inline]
    pub const fn new(at: ProgramPoint, action: Insert) -> Self {
        Self { at, action }
    }

    #[inline]
    pub const fn before(at: ProgramPoint) -> Self {
        Self {
            at,
            action: Insert::Before,
        }
    }

    #[inline]
    pub const fn after(at: ProgramPoint) -> Self {
        Self {
            at,
            action: Insert::After,
        }
    }

    pub fn block(&self) -> BlockRef {
        self.at.block().expect("cannot insert relative to detached operation")
    }
}

/// A `ProgramPoint` represents a position in a function where the live range of an SSA value can
/// begin or end. It can be either:
///
/// 1. An instruction or
/// 2. A block header.
///
/// This corresponds more or less to the lines in the textual form of the IR.
#[derive(PartialEq, Eq, Clone, Hash)]
pub enum ProgramPoint {
    /// An operation
    Op(OperationRef),
    /// A block header.
    Block(BlockRef),
}
impl ProgramPoint {
    /// Get the operation we know is inside.
    pub fn unwrap_op(self) -> OperationRef {
        use crate::Entity;
        match self {
            Self::Op(x) => x,
            Self::Block(x) => panic!("expected operation, but got {}", x.borrow().id()),
        }
    }

    /// Get the block associated with this program point
    ///
    /// Returns `None` if the program point is a detached operation.
    pub fn block(&self) -> Option<BlockRef> {
        match self {
            Self::Op(op) => op.borrow().parent(),
            Self::Block(block) => Some(block.clone()),
        }
    }
}
impl From<OperationRef> for ProgramPoint {
    fn from(op: OperationRef) -> Self {
        Self::Op(op)
    }
}
impl From<BlockRef> for ProgramPoint {
    fn from(block: BlockRef) -> Self {
        Self::Block(block)
    }
}
impl fmt::Display for ProgramPoint {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        use crate::Entity;
        match self {
            Self::Op(x) => write!(f, "{}", x.borrow().name()),
            Self::Block(x) => write!(f, "{}", x.borrow().id()),
        }
    }
}
impl fmt::Debug for ProgramPoint {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_tuple("ProgramPoint").field_with(|f| write!(f, "{}", self)).finish()
    }
}
