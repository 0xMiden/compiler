use core::fmt;

use crate::{
    entity::{EntityProjection, EntityProjectionMut},
    Block, BlockRef, EntityCursor, EntityCursorMut, EntityMut, EntityRef, Operation, OperationRef,
    Spanned,
};

/// [ProgramPoint] represents a specific location in the execution of a program.
///
/// A program point consists of two parts:
///
/// * An anchor, either a block or operation
/// * A position, i.e. the direction relative to the anchor to which the program point refers
///
/// A program point can be reified as a cursor within a block, such that an operation inserted at
/// the cursor will be placed at the specified position relative to the anchor.
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
        position: Position,
    },
    /// A program point referring to the entry or exit of an operation
    Op {
        /// The operation this program point refers to
        op: OperationRef,
        /// The placement of the cursor relative to `op`
        position: Position,
    },
}

/// Represents the placement of inserted items relative to a [ProgramPoint]
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Position {
    /// New items will be inserted before the current program point
    Before,
    /// New items will be inserted after the current program point
    After,
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
        Self::from(op.as_operation_ref())
    }
}

/// Construct a ProgramPoint referring to the point at entry to `op`
impl From<OperationRef> for ProgramPoint {
    #[inline]
    fn from(op: OperationRef) -> Self {
        Self::Op {
            op,
            position: Position::Before,
        }
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
            position: Position::Before,
        }
    }
}

#[doc(hidden)]
#[derive(Copy, Clone)]
pub struct BlockPoint {
    block: BlockRef,
    point: Position,
}
impl From<BlockPoint> for ProgramPoint {
    fn from(point: BlockPoint) -> Self {
        ProgramPoint::Block {
            block: point.block,
            position: point.point,
        }
    }
}
impl From<BlockRef> for BlockPoint {
    fn from(block: BlockRef) -> Self {
        Self {
            block,
            point: Position::Before,
        }
    }
}
impl From<&Block> for BlockPoint {
    fn from(block: &Block) -> Self {
        Self {
            block: block.as_block_ref(),
            point: Position::Before,
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
            Self::Op {
                position: ref mut point,
                ..
            }
            | Self::Block {
                position: ref mut point,
                ..
            } => {
                *point = Position::After;
            }
        }
        pp
    }

    /// Create a [ProgramPoint] at entry to `block`, i.e. "before"
    pub fn at_start_of(block: impl Into<BlockPoint>) -> Self {
        let BlockPoint { block, .. } = block.into();
        Self::Block {
            block,
            position: Position::Before,
        }
    }

    /// Create a [ProgramPoint] at exit from `block`, i.e. "after"
    pub fn at_end_of(block: impl Into<BlockPoint>) -> Self {
        let BlockPoint { block, .. } = block.into();
        Self::Block {
            block,
            position: Position::After,
        }
    }

    /// Returns true if this program point is at the start of the containing block
    pub fn is_at_block_start(&self) -> bool {
        self.operation().is_some_and(|op| {
            op.parent().is_some() && op.prev().is_none() && self.placement() == Position::Before
        }) || matches!(self, Self::Block { position: Position::Before, block, .. } if block.borrow().body().is_empty())
    }

    /// Returns true if this program point is at the end of the containing block
    pub fn is_at_block_end(&self) -> bool {
        self.operation().is_some_and(|op| {
            op.parent().is_some() && op.next().is_none() && self.placement() == Position::After
        }) || matches!(self, Self::Block { position: Position::After, block, .. } if block.borrow().body().is_empty())
    }

    /// Returns the block of the program point anchor.
    ///
    /// Returns `None`, if the program point is either invalid, or pointing to an orphaned operation
    pub fn block(&self) -> Option<BlockRef> {
        match self {
            Self::Invalid => None,
            Self::Block { block, .. } => Some(*block),
            Self::Op { op, .. } => op.parent(),
        }
    }

    /// Returns the program point anchor as an operation.
    ///
    /// Returns `None` if the program point is either invalid, or not pointing to a specific op
    pub fn operation(&self) -> Option<OperationRef> {
        match self {
            Self::Invalid => None,
            Self::Block {
                position: Position::Before,
                block,
                ..
            } => block.borrow().body().front().as_pointer(),
            Self::Block {
                position: Position::After,
                block,
                ..
            } => block.borrow().body().back().as_pointer(),
            Self::Op { op, .. } => Some(*op),
        }
    }

    /// Returns the operation after [Self::operation], relative to this program point.
    ///
    /// If the current program point is in an orphaned operation, this will return the current op.
    ///
    /// Returns `None` if the program point is either invalid, or not pointing to a specific op
    #[track_caller]
    pub fn next_operation(&self) -> Option<OperationRef> {
        assert!(!self.is_at_block_end());
        match self {
            Self::Op {
                position: Position::After,
                op,
                ..
            } if op.parent().is_some() => op.next(),
            Self::Op { op, .. } => Some(*op),
            Self::Block {
                position: Position::Before,
                block,
            } => block.borrow().front(),
            Self::Block { .. } | Self::Invalid => None,
        }
    }

    /// Returns the operation preceding [Self::operation], relative to this program point.
    ///
    /// If the current program point is in an orphaned operation, this will return the current op.
    ///
    /// Returns `None` if the program point is either invalid, or not pointing to a specific op
    #[track_caller]
    pub fn prev_operation(&self) -> Option<OperationRef> {
        assert!(!self.is_at_block_start());
        match self {
            Self::Op {
                position: Position::Before,
                op,
                ..
            } if op.parent().is_some() => op.prev(),
            Self::Op { op, .. } => Some(*op),
            Self::Block {
                position: Position::After,
                block,
            } => block.borrow().back(),
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

    /// The positioning relative to the program point anchor
    pub fn placement(&self) -> Position {
        match self {
            Self::Invalid => Position::After,
            Self::Block {
                position: point, ..
            }
            | Self::Op {
                position: point, ..
            } => *point,
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
            Self::Block {
                block,
                position: point,
            } => Some(EntityRef::project(block.borrow(), |block| match point {
                Position::Before => block.body().front(),
                Position::After => block.body().back(),
            })),
            Self::Op {
                op,
                position: point,
            } => {
                let block = op.parent()?;
                Some(EntityRef::project(block.borrow(), |block| match point {
                    Position::Before => {
                        if let Some(placement) = op.prev() {
                            unsafe { block.body().cursor_from_ptr(placement) }
                        } else {
                            block.body().cursor()
                        }
                    }
                    Position::After => unsafe { block.body().cursor_from_ptr(*op) },
                }))
            }
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
            Self::Block {
                block,
                position: point,
            } => Some(EntityMut::project(block.borrow_mut(), |block| match point {
                Position::Before => block.body_mut().cursor_mut(),
                Position::After => block.body_mut().back_mut(),
            })),
            Self::Op {
                op,
                position: point,
            } => {
                let mut block = op.parent()?;
                Some(EntityMut::project(block.borrow_mut(), |block| match point {
                    Position::Before => {
                        if let Some(placement) = op.prev() {
                            unsafe { block.body_mut().cursor_mut_from_ptr(placement) }
                        } else {
                            block.body_mut().cursor_mut()
                        }
                    }
                    Position::After => unsafe { block.body_mut().cursor_mut_from_ptr(*op) },
                }))
            }
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
                    position: xp,
                },
                Self::Block {
                    block: y,
                    position: yp,
                },
            ) => x == y && xp == yp,
            (
                Self::Op {
                    op: x,
                    position: xp,
                    ..
                },
                Self::Op {
                    op: y,
                    position: yp,
                    ..
                },
            ) => x == y && xp == yp,
            (..) => false,
        }
    }
}

impl core::hash::Hash for ProgramPoint {
    fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
        core::mem::discriminant(self).hash(state);
        match self {
            Self::Invalid => (),
            Self::Block {
                block,
                position: point,
            } => {
                core::ptr::hash(BlockRef::as_ptr(block), state);
                point.hash(state);
            }
            Self::Op {
                op,
                position: point,
                ..
            } => {
                core::ptr::hash(OperationRef::as_ptr(op), state);
                point.hash(state);
            }
        }
    }
}

impl Spanned for ProgramPoint {
    fn span(&self) -> crate::SourceSpan {
        use crate::SourceSpan;

        match self {
            Self::Invalid => SourceSpan::UNKNOWN,
            Self::Block {
                block,
                position: point,
            } => match point {
                Position::Before => {
                    block.borrow().body().front().get().map(|op| op.span()).unwrap_or_default()
                }
                Position::After => {
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
            Self::Block {
                block,
                position: point,
            } => match point {
                Position::Before => write!(f, "start({})", &block.borrow().id()),
                Position::After => write!(f, "end({})", &block.borrow().id()),
            },
            Self::Op {
                op,
                position: point,
            } => {
                use crate::formatter::{const_text, display};
                let block = op
                    .parent()
                    .map(|blk| display(blk.borrow().id()))
                    .unwrap_or_else(|| const_text("null"));
                match point {
                    Position::Before => {
                        write!(f, "before({} in {block})", &op.borrow().name())
                    }
                    Position::After => {
                        write!(f, "after({} in {block})", &op.borrow().name())
                    }
                }
            }
        }
    }
}

impl fmt::Debug for ProgramPoint {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        use crate::EntityWithId;
        match self {
            Self::Invalid => f.write_str("Invalid"),
            Self::Block {
                block,
                position: point,
            } => f
                .debug_struct("Block")
                .field("block", &block.borrow().id())
                .field("point", point)
                .finish(),
            Self::Op {
                op,
                position: point,
            } => f
                .debug_struct("Op")
                .field("block", &op.parent().map(|blk| blk.borrow().id()))
                .field("point", point)
                .field("op", &op.borrow())
                .finish(),
        }
    }
}
