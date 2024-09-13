use core::{fmt, ptr};

use super::{Block, EntityCursor, EntityCursorMut, EntityIter, EntityList, Type, Usable};
use crate::{SourceSpan, Spanned};

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum ValueKind {
    Result,
    BlockArgument,
}

#[derive(Spanned)]
pub struct ValueImpl {
    kind: ValueKind,
    ty: Type,
    #[span]
    span: SourceSpan,
    uses: OpOperandList,
}
impl ValueImpl {
    #[inline(always)]
    pub const fn kind(&self) -> ValueKind {
        self.kind
    }

    pub fn is_result(&self) -> bool {
        matches!(self, ValueKind::Result)
    }

    pub fn is_block_argument(&self) -> bool {
        matches!(self, ValueKind::BlockArgument)
    }

    #[inline(always)]
    pub fn ty(&self) -> &Type {
        &self.ty
    }

    #[inline(always)]
    pub fn set_type(&mut self, ty: Type) {
        self.ty = ty;
    }
}
impl Usable for ValueImpl {
    type Use = OpOperand;

    #[inline]
    fn is_used(&self) -> bool {
        !self.uses.is_empty()
    }

    #[inline]
    fn uses(&self) -> OpOperandIter<'_> {
        self.uses.iter()
    }

    #[inline]
    fn first_use(&self) -> OpOperandCursor<'_> {
        self.uses.front()
    }

    #[inline]
    fn first_use_mut(&mut self) -> OpOperandCursorMut<'_> {
        self.uses.front_mut()
    }
}
impl fmt::Debug for ValueImpl {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("ValueImpl")
            .field("kind", &self.kind)
            .field("ty", &self.ty)
            .field("uses", &self.uses)
            .finish()
    }
}

pub type Value = EntityHandle<ValueImpl>;

#[derive(Spanned)]
pub struct BlockArgument {
    #[span]
    value: ValueImpl,
    owner: EntityHandle<Block>,
    index: u8,
}
impl Usable for BlockArgument {
    type Use = OpOperand;

    #[inline]
    fn is_used(&self) -> bool {
        self.value.is_used()
    }

    #[inline]
    fn uses(&self) -> OpOperandIter<'_> {
        self.value.uses()
    }

    #[inline]
    fn first_use(&self) -> OpOperandCursor<'_> {
        self.value.first_use()
    }

    #[inline]
    fn first_use_mut(&mut self) -> OpOperandCursorMut<'_> {
        self.value.first_use_mut()
    }
}

/// An [OpResult] represents the definition of a [Value] by the result of an [Operation]
#[derive(Spanned)]
pub struct OpResult {
    #[span]
    value: ValueImpl,
    owner: EntityHandle<Operation>,
    index: u8,
}
impl Usable for OpResult {
    type Use = OpOperand;

    #[inline]
    fn is_used(&self) -> bool {
        self.value.is_used()
    }

    #[inline]
    fn uses(&self) -> OpOperandIter<'_> {
        self.value.uses()
    }

    #[inline]
    fn first_use(&self) -> OpOperandCursor<'_> {
        self.value.first_use()
    }

    #[inline]
    fn first_use_mut(&mut self) -> OpOperandCursorMut<'_> {
        self.value.first_use_mut()
    }
}

pub type OpOperandList = EntityList<OpOperand>;
pub type OpOperandIter<'a> = EntityIter<'a, OpOperand>;
pub type OpOperandCursor<'a> = EntityCursor<'a, OpOperand>;
pub type OpOperandCursorMut<'a> = EntityCursorMut<'a, OpOperand>;

/// An [OpOperand] represents a use of a [Value] by an [Operation]
pub struct OpOperand {
    /// The operand value
    pub value: Value,
    /// The owner of this operand, i.e. the operation it is an operand of
    pub owner: EntityHandle<Operation>,
    /// The index of this operand in the operand list of an operation
    pub index: u8,
}
impl OpOperand {
    #[inline]
    pub fn new(value: Value, owner: EntityHandle<Operation>, index: u8) -> Self {
        Self {
            value,
            owner,
            index,
        }
    }
}
