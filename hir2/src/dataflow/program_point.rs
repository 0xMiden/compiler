use core::fmt;

use crate::{BlockRef, Insert, InsertionPoint, OperationRef, Spanned};

/// [ProgramPoint] represents a specific location in the execution of a program.
///
/// A sequence of program points can be combined into a control flow graph.
#[derive(Default, Clone)]
pub enum ProgramPoint {
    #[default]
    Invalid,
    Block {
        block: BlockRef,
        point: Insert,
    },
    Op {
        block: Option<BlockRef>,
        op: OperationRef,
        point: Insert,
    },
}

impl ProgramPoint {
    pub fn new(ip: impl Into<InsertionPoint>) -> Self {
        let ip = ip.into();
        let point = ip.placement;
        match ip.at {
            crate::ProgramPoint::Block(block) => Self::Block { block, point },
            crate::ProgramPoint::Op(op) => {
                let block = op.borrow().parent();
                Self::Op { block, op, point }
            }
        }
    }

    pub fn before(op: OperationRef) -> Self {
        let block = op.borrow().parent();
        Self::Op {
            block,
            op,
            point: Insert::Before,
        }
    }

    pub fn after(op: OperationRef) -> Self {
        let block = op.borrow().parent();
        Self::Op {
            block,
            op,
            point: Insert::After,
        }
    }

    pub fn at_start_of(block: BlockRef) -> Self {
        Self::Block {
            block,
            point: Insert::Before,
        }
    }

    pub fn at_end_of(block: BlockRef) -> Self {
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
            Self::Block { block, .. } => Some(block.clone()),
            Self::Op { block, .. } => block.clone(),
        }
    }

    /// Returns the operation from which this program point relates
    ///
    /// Returns `None` if the program point is either invalid, or not pointing to a specific op
    pub fn operation(&self) -> Option<OperationRef> {
        match self {
            Self::Op { op, .. } => Some(op.clone()),
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
            } => Some(op.clone()),
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
            } => Some(op.clone()),
            Self::Block { .. } | Self::Invalid => None,
        }
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
    /// the correct position relative to `self`. The specific rules are as follows:
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
    pub fn insertion_point(&self) -> Option<InsertionPoint> {
        match self {
            Self::Invalid => None,
            Self::Block { block, point } => Some(InsertionPoint::new(block.clone().into(), *point)),
            Self::Op { block: None, .. } => None,
            Self::Op {
                block: Some(ref block),
                op,
                point,
            } => match point {
                // Place cursor on the op just prior to `op`, so that inserting at the cursor, inserts before `op`
                //
                // If there are no ops before `op`, this returns a cursor to the front of the block, which has the same effect
                Insert::Before => Some(
                    op.prev()
                        .map(|op| InsertionPoint::new(op.into(), *point))
                        .unwrap_or_else(|| InsertionPoint::new(block.clone().into(), *point)),
                ),
                // Place cursor on `op`, so that inserting at the cursor, inserts immediately after `op`
                Insert::After => Some(InsertionPoint::new(op.clone().into(), *point)),
            },
        }
    }
}

impl Eq for ProgramPoint {}

impl PartialEq for ProgramPoint {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Invalid, Self::Invalid) => true,
            (Self::Invalid, _) | (_, Self::Invalid) => false,
            (a, b) => a.insertion_point() == b.insertion_point(),
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
