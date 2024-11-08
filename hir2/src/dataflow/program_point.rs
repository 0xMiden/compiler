use core::fmt;

use crate::{
    entity::{EntityProjection, EntityProjectionMut},
    Block, BlockRef, EntityCursor, EntityCursorMut, EntityMut, EntityRef, Insert, Operation,
    OperationRef, Spanned,
};

/// [ProgramPoint] represents a specific location in the execution of a program.
///
/// A sequence of program points can be combined into a control flow graph.
#[derive(Default, Copy, Clone)]
pub enum ProgramPoint {
    /// A program point which refers to nothing, and is always invalid if used
    #[default]
    Invalid,
    /// A program point referring to the entry or exit of a block
    Block {
        /// The block this program point refers to
        block: BlockRef,
        /// The placement of the cursor relative to `block`
        point: Insert,
    },
    /// A program point referring to the entry or exit of an operation
    Op {
        /// The block to which this operation belongs
        block: Option<BlockRef>,
        /// The operation this program point refers to
        op: OperationRef,
        /// The placement of the cursor relative to `op`
        point: Insert,
    },
}

impl<T> From<EntityRef<'_, T>> for ProgramPoint
where
    for<'a> ProgramPoint: From<&'a T>,
{
    #[inline]
    fn from(entity: EntityRef<'_, T>) -> Self {
        Self::from(&*entity)
    }
}

impl<T> From<EntityMut<'_, T>> for ProgramPoint
where
    for<'a> ProgramPoint: From<&'a T>,
{
    #[inline]
    fn from(entity: EntityMut<'_, T>) -> Self {
        Self::from(&*entity)
    }
}

/// Construct a ProgramPoint referring to the point at entry to `op`
impl From<&Operation> for ProgramPoint {
    #[inline]
    fn from(op: &Operation) -> Self {
        let block = op.parent();
        Self::Op {
            block,
            op: op.as_operation_ref(),
            point: Insert::Before,
        }
    }
}

/// Construct a ProgramPoint referring to the point at entry to `op`
impl From<OperationRef> for ProgramPoint {
    #[inline]
    fn from(op: OperationRef) -> Self {
        Self::from(op.borrow())
    }
}

/// Construct a ProgramPoint referring to the point at entry to `block`
impl From<&Block> for ProgramPoint {
    #[inline]
    fn from(block: &Block) -> Self {
        Self::at_start_of(block.as_block_ref())
    }
}

/// Construct a ProgramPoint referring to the point at entry to `block`
impl From<BlockRef> for ProgramPoint {
    #[inline]
    fn from(block: BlockRef) -> Self {
        Self::Block {
            block,
            point: Insert::Before,
        }
    }
}

#[derive(Copy, Clone)]
pub struct BlockPoint {
    block: BlockRef,
    point: Insert,
}
impl From<BlockPoint> for ProgramPoint {
    fn from(point: BlockPoint) -> Self {
        ProgramPoint::Block {
            block: point.block,
            point: point.point,
        }
    }
}
impl From<BlockRef> for BlockPoint {
    fn from(block: BlockRef) -> Self {
        Self {
            block,
            point: Insert::Before,
        }
    }
}
impl From<&Block> for BlockPoint {
    fn from(block: &Block) -> Self {
        Self {
            block: block.as_block_ref(),
            point: Insert::Before,
        }
    }
}

impl ProgramPoint {
    /// Create a [ProgramPoint] at entry to `entity`, i.e. "before"
    #[inline]
    pub fn before(entity: impl Into<ProgramPoint>) -> Self {
        entity.into()
    }

    /// Create a [ProgramPoint] at exit from `entity`, i.e. "after"
    pub fn after(entity: impl Into<ProgramPoint>) -> Self {
        let mut pp = entity.into();
        match &mut pp {
            Self::Invalid => (),
            Self::Op { ref mut point, .. } | Self::Block { ref mut point, .. } => {
                *point = Insert::After;
            }
        }
        pp
    }

    /// Create a [ProgramPoint] at entry to `block`, i.e. "before"
    pub fn at_start_of(block: impl Into<BlockPoint>) -> Self {
        let BlockPoint { block, .. } = block.into();
        Self::Block {
            block,
            point: Insert::Before,
        }
    }

    /// Create a [ProgramPoint] at exit from `block`, i.e. "after"
    pub fn at_end_of(block: impl Into<BlockPoint>) -> Self {
        let BlockPoint { block, .. } = block.into();
        Self::Block {
            block,
            point: Insert::After,
        }
    }

    /// Returns true if this program point is at the start of the containing block
    pub fn is_at_block_start(&self) -> bool {
        match self {
            Self::Invalid => false,
            Self::Block {
                point: Insert::Before,
                ..
            } => true,
            Self::Block { block, .. } => block.borrow().body().is_empty(),
            Self::Op { block: None, .. } => false,
            Self::Op { op, .. } => op.prev().is_none(),
        }
    }

    /// Returns true if this program point is at the end of the containing block
    pub fn is_at_block_end(&self) -> bool {
        match self {
            Self::Invalid => false,
            Self::Block {
                point: Insert::After,
                ..
            } => true,
            Self::Block { block, .. } => block.borrow().body().is_empty(),
            Self::Op { block: None, .. } => false,
            Self::Op { op, .. } => op.next().is_none(),
        }
    }

    /// Returns the block containing this program point
    ///
    /// Returns `None`, if the program point is either invalid, or pointing to an orphaned operation
    pub fn block(&self) -> Option<BlockRef> {
        match self {
            Self::Invalid => None,
            Self::Block { block, .. } => Some(*block),
            Self::Op { block, .. } => *block,
        }
    }

    /// Returns the operation from which this program point relates
    ///
    /// Returns `None` if the program point is either invalid, or not pointing to a specific op
    pub fn operation(&self) -> Option<OperationRef> {
        match self {
            Self::Op { op, .. } => Some(*op),
            Self::Block { .. } | Self::Invalid => None,
        }
    }

    /// Returns the operation after [Self::operation], relative to this program point.
    ///
    /// If the current program point is in an orphaned operation, this will return the current op.
    ///
    /// Returns `None` if the program point is either invalid, or not pointing to a specific op
    pub fn next_operation(&self) -> Option<OperationRef> {
        match self {
            Self::Op {
                block: Some(_), op, ..
            } => op.next(),
            Self::Op {
                block: None, op, ..
            } => Some(*op),
            Self::Block { .. } | Self::Invalid => None,
        }
    }

    /// Returns the operation preceding [Self::operation], relative to this program point.
    ///
    /// If the current program point is in an orphaned operation, this will return the current op.
    ///
    /// Returns `None` if the program point is either invalid, or not pointing to a specific op
    pub fn prev_operation(&self) -> Option<OperationRef> {
        match self {
            Self::Op {
                block: Some(_), op, ..
            } => op.prev(),
            Self::Op {
                block: None, op, ..
            } => Some(*op),
            Self::Block { .. } | Self::Invalid => None,
        }
    }

    /// Returns true if this program point refers to a valid program point
    #[inline]
    pub fn is_valid(&self) -> bool {
        !self.is_unset()
    }

    /// Returns true if this program point is invalid/unset
    #[inline]
    pub fn is_unset(&self) -> bool {
        matches!(self, Self::Invalid)
    }

    pub fn point(&self) -> Option<Insert> {
        match self {
            Self::Invalid => None,
            Self::Block { point, .. } | Self::Op { point, .. } => Some(*point),
        }
    }

    /// Obtain an immutable cursor in the block corresponding to this program point.
    ///
    /// The resulting cursor can have `as_pointer` or `get` called on it to get the operation to
    /// which this point is relative. The intuition around where the cursor is placed for a given
    /// program point can be understood as answering the question of "where does the cursor need
    /// to be, such that if I inserted an op at that cursor, that the insertion would be placed at
    /// the referenced program point (semantically before or after an operation or block). The
    /// specific rules are as follows:
    ///
    /// * If "before" a block, the resulting cursor is the null cursor for the containing block,
    ///   since an insertion at the null cursor will be placed at the start of the block.
    /// * If "after" a block, the cursor is placed on the last operation in the block, as insertion
    ///   will place the inserted op at the end of the block
    /// * If "before" an operation, the cursor is placed on the operation immediately preceding
    ///   `self`, or a null cursor is returned. In both cases, an insertion at the returned cursor
    ///   would be placed immediately before `self`
    /// * If "after" an operation, the cursor is placed on the operation in `self`, so that
    ///   insertion will place the inserted op immediately after `self`.
    ///
    /// NOTE: The block to which this program point refers will be borrowed for the lifetime of the
    /// returned [EntityProjection].
    pub fn cursor<'a, 'b: 'a, 'c: 'b>(
        &'c self,
    ) -> Option<EntityProjection<'b, EntityCursor<'a, Operation>>> {
        match self {
            Self::Invalid => None,
            Self::Block { block, point } => {
                Some(EntityRef::project(block.borrow(), |block| match point {
                    Insert::Before => block.body().front(),
                    Insert::After => block.body().back(),
                }))
            }
            Self::Op { block: None, .. } => None,
            Self::Op {
                block: Some(block),
                op,
                point,
            } => Some(EntityRef::project(block.borrow(), |block| match point {
                Insert::Before => {
                    let mut cursor = unsafe { block.body().cursor_from_ptr(*op) };
                    cursor.move_prev();
                    cursor
                }
                Insert::After => unsafe { block.body().cursor_from_ptr(*op) },
            })),
        }
    }

    /// Same as [Self::cursor], but obtains a mutable cursor instead.
    ///
    /// NOTE: The block to which this program point refers will be borrowed mutably for the lifetime
    /// of the returned [EntityProjectionMut].
    pub fn cursor_mut<'a, 'b: 'a, 'c: 'b>(
        &'c mut self,
    ) -> Option<EntityProjectionMut<'b, EntityCursorMut<'a, Operation>>> {
        match self {
            Self::Invalid => None,
            Self::Block { block, point } => {
                Some(EntityMut::project(block.borrow_mut(), |block| match point {
                    Insert::Before => block.body_mut().cursor_mut(),
                    Insert::After => block.body_mut().back_mut(),
                }))
            }
            Self::Op { block: None, .. } => None,
            Self::Op {
                block: Some(block),
                op,
                point,
            } => Some(EntityMut::project(block.borrow_mut(), |block| match point {
                Insert::Before => {
                    let mut cursor = unsafe { block.body_mut().cursor_mut_from_ptr(*op) };
                    cursor.move_prev();
                    cursor
                }
                Insert::After => unsafe { block.body_mut().cursor_mut_from_ptr(*op) },
            })),
        }
    }
}

impl Eq for ProgramPoint {}

impl PartialEq for ProgramPoint {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Invalid, Self::Invalid) => true,
            (Self::Invalid, _) | (_, Self::Invalid) => false,
            (
                Self::Block {
                    block: x,
                    point: xp,
                },
                Self::Block {
                    block: y,
                    point: yp,
                },
            ) => x == y && xp == yp,
            (
                Self::Op {
                    op: x, point: xp, ..
                },
                Self::Op {
                    op: y, point: yp, ..
                },
            ) => x == y && xp == yp,
            (..) => false,
        }
    }
}

impl core::hash::Hash for ProgramPoint {
    fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
        use crate::EntityWithId;

        core::mem::discriminant(self).hash(state);
        self.block().map(|b| b.borrow().id()).hash(state);
        match self.operation() {
            None => core::ptr::hash::<crate::Operation, _>(core::ptr::null(), state),
            Some(op) => core::ptr::hash::<crate::Operation, _>(&*op.borrow(), state),
        }
    }
}

impl Spanned for ProgramPoint {
    fn span(&self) -> crate::SourceSpan {
        use crate::SourceSpan;

        match self {
            Self::Invalid => SourceSpan::UNKNOWN,
            Self::Block { block, point } => match point {
                Insert::Before => {
                    block.borrow().body().front().get().map(|op| op.span()).unwrap_or_default()
                }
                Insert::After => {
                    block.borrow().body().back().get().map(|op| op.span()).unwrap_or_default()
                }
            },
            Self::Op { op, .. } => op.borrow().span(),
        }
    }
}

impl fmt::Display for ProgramPoint {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        use crate::EntityWithId;
        match self {
            Self::Invalid => f.write_str("<invalid>"),
            Self::Block { block, point } => match point {
                Insert::Before => write!(f, "start({})", &block.borrow().id()),
                Insert::After => write!(f, "end({})", &block.borrow().id()),
            },
            Self::Op {
                block: None,
                op,
                point,
            } => match point {
                Insert::Before => write!(f, "before({} in null)", &op.borrow().name()),
                Insert::After => write!(f, "after({} in null)", &op.borrow().name()),
            },
            Self::Op {
                block: Some(block),
                op,
                point,
            } => match point {
                Insert::Before => {
                    write!(f, "before({} in {})", &op.borrow().name(), &block.borrow().id())
                }
                Insert::After => {
                    write!(f, "after({} in {})", &op.borrow().name(), &block.borrow().id())
                }
            },
        }
    }
}

impl fmt::Debug for ProgramPoint {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        use crate::EntityWithId;
        match self {
            Self::Invalid => f.write_str("Invalid"),
            Self::Block { block, point } => f
                .debug_struct("Block")
                .field("block", &block.borrow().id())
                .field("point", point)
                .finish(),
            Self::Op {
                block: None,
                op,
                point,
            } => f
                .debug_struct("Orphaned")
                .field("point", point)
                .field("op", &op.borrow())
                .finish_non_exhaustive(),
            Self::Op {
                block: Some(block),
                op,
                point,
            } => f
                .debug_struct("Op")
                .field("block", &block.borrow().id())
                .field("point", point)
                .field("op", &op.borrow())
                .finish(),
        }
    }
}
