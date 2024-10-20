use core::fmt;

use crate::{BlockRef, OperationRef};

/// Represents the placement of inserted items relative to a [ProgramPoint]
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Insert {
    /// New items will be inserted before the current program point
    Before,
    /// New items will be inserted after the current program point
    After,
}

/// Represents a cursor within a region where new operations will be inserted.
///
/// The `placement` field determines how new operations will be inserted relative to `at`:
///
/// * If `at` is a block:
///   * `Insert::Before` will always inserts new operations as the first operation in the block,
///     i.e. every insert pushes to the front of the list of operations.
///   * `Insert::After` will always insert new operations at the end of the block, i.e. every insert
///     pushes to the back of the list of operations.
/// * If `at` is an operation:
///   * `Insert::Before` will always insert new operations directly preceding the `at` operation.
///   * `Insert::After` will always insert new operations directly following the `at` operation.
///
/// If a builder/rewriter wishes to insert new operations starting at some point in the middle of a
/// block, but then move the insertion point forward as new operations are inserted, the builder
/// must call [move_next] (or [move_prev]) after each insertion.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct InsertionPoint {
    pub at: ProgramPoint,
    pub placement: Insert,
}
impl InsertionPoint {
    /// Create a new insertion point with the specified placement, at the given program point.
    #[inline]
    pub const fn new(at: ProgramPoint, placement: Insert) -> Self {
        Self { at, placement }
    }

    /// Create a new insertion point at the given program point, which will place new operations
    /// "before" that point.
    ///
    /// See [Insert::Before] for what the semantics of "before" means with regards to the different
    /// kinds of program point.
    #[inline]
    pub fn before(at: impl Into<ProgramPoint>) -> Self {
        Self {
            at: at.into(),
            placement: Insert::Before,
        }
    }

    /// Create a new insertion point at the given program point, which will place new operations
    /// "after" that point.
    ///
    /// See [Insert::After] for what the semantics of "after" means with regards to the different
    /// kinds of program point.
    #[inline]
    pub fn after(at: impl Into<ProgramPoint>) -> Self {
        Self {
            at: at.into(),
            placement: Insert::After,
        }
    }

    /// Moves the insertion point to the previous operation relative to the current point.
    ///
    /// If there is no operation before the current point, this has no effect.
    ///
    /// If the current point is a [ProgramPoint::Block], and `self.placement` is `Insert::After`,
    /// then this moves the insertion point to the operation immediately preceding the last
    /// operation in the block, _if_ there are at least two operations in the block. In other words,
    /// `self.at` becomes a [ProgramPoint::Op].
    pub fn move_prev(&mut self) {
        match &mut self.at {
            ProgramPoint::Op(ref mut current) => {
                let prev = current.prev();
                if let Some(prev) = prev {
                    *current = prev;
                }
            }
            ProgramPoint::Block(ref block) => {
                if matches!(self.placement, Insert::After) {
                    if let Some(prev) =
                        block.borrow().body().back().as_pointer().and_then(|current| current.prev())
                    {
                        self.at = ProgramPoint::Op(prev);
                    }
                }
            }
        }
    }

    /// Moves the insertion point to the next operation relative to the current point.
    ///
    /// If there is no operation after the current point, this has no effect.
    ///
    /// If the current point is a [ProgramPoint::Block], and `self.placement` is `Insert::Before`,
    /// then this moves the insertion point to the operation immediately following the first
    /// operation in the block, _if_ there are at least two operations in the block. In other words,
    /// `self.at` becomes a [ProgramPoint::Op].
    pub fn move_next(&mut self) {
        match &mut self.at {
            ProgramPoint::Op(ref mut current) => {
                let next = current.next();
                if let Some(next) = next {
                    *current = next;
                }
            }
            ProgramPoint::Block(ref block) => {
                if matches!(self.placement, Insert::Before) {
                    if let Some(next) = block
                        .borrow()
                        .body()
                        .front()
                        .as_pointer()
                        .and_then(|current| current.next())
                    {
                        self.at = ProgramPoint::Op(next);
                    }
                }
            }
        }
    }

    /// Get a pointer to the [crate::Operation] on which this insertion point is positioned.
    ///
    /// Returns `None` if the insertion point is positioned in an empty block.
    pub fn op(&self) -> Option<OperationRef> {
        match self.at {
            ProgramPoint::Op(ref op) => Some(op.clone()),
            ProgramPoint::Block(ref block) => match self.placement {
                Insert::Before => block.borrow().front(),
                Insert::After => block.borrow().back(),
            },
        }
    }

    /// Get a pointer to the [crate::Block] in which this insertion point is positioned.
    ///
    /// Panics if the current program point is an operation detached from any block.
    pub fn block(&self) -> BlockRef {
        self.at
            .block()
            .expect("invalid insertion point: operation is detached from any block")
    }

    /// Returns true if this insertion point is positioned at the end of the containing block
    pub fn is_at_block_end(&self) -> bool {
        let block = self.block().borrow();
        if block.body().is_empty() {
            matches!(self.at, ProgramPoint::Block(_))
        } else if matches!(self.placement, Insert::Before) {
            false
        } else {
            match &self.at {
                ProgramPoint::Block(_) => true,
                ProgramPoint::Op(ref op) => &block.back().unwrap() == op,
            }
        }
    }
}

/// A `ProgramPoint` represents a position in a region where the live range of an SSA value can
/// begin or end. It can be either:
///
/// 1. An operation
/// 2. A block
///
/// This corresponds more or less to the lines in the textual form of the IR.
#[derive(PartialEq, Eq, Clone, Hash)]
pub enum ProgramPoint {
    /// An operation
    Op(OperationRef),
    /// A block
    Block(BlockRef),
}
impl ProgramPoint {
    /// Unwrap this program point as an [OperationRef], or panic if this is not a [ProgramPoint::Op]
    pub fn unwrap_op(self) -> OperationRef {
        use crate::EntityWithId;
        match self {
            Self::Op(x) => x,
            Self::Block(x) => panic!("expected operation, but got {}", x.borrow().id()),
        }
    }

    /// Get the closest operation associated with this program point
    ///
    /// If this program point refers to a block, this will return the last operation in the block,
    /// or `None` if the block is empty.
    pub fn op(&self) -> Option<OperationRef> {
        match self {
            Self::Op(op) => Some(op.clone()),
            Self::Block(block) => block.borrow().back(),
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
        use crate::EntityWithId;
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
