use core::fmt;

use crate::{EntityRef, OperationRef, Type, UnsafeIntrusiveEntityRef, Value, ValueId, ValueRef};

pub type OpOperand = UnsafeIntrusiveEntityRef<OpOperandImpl>;
pub type OpOperandList = crate::EntityList<OpOperandImpl>;
#[allow(unused)]
pub type OpOperandIter<'a> = crate::EntityIter<'a, OpOperandImpl>;
#[allow(unused)]
pub type OpOperandCursor<'a> = crate::EntityCursor<'a, OpOperandImpl>;
#[allow(unused)]
pub type OpOperandCursorMut<'a> = crate::EntityCursorMut<'a, OpOperandImpl>;

/// An [OpOperand] represents a use of a [Value] by an [Operation]
pub struct OpOperandImpl {
    /// The operand value
    pub value: ValueRef,
    /// The owner of this operand, i.e. the operation it is an operand of
    pub owner: OperationRef,
    /// The index of this operand in the operand list of an operation
    pub index: u8,
}
impl OpOperandImpl {
    #[inline]
    pub fn new(value: ValueRef, owner: OperationRef, index: u8) -> Self {
        Self {
            value,
            owner,
            index,
        }
    }

    pub fn value(&self) -> EntityRef<'_, dyn Value> {
        self.value.borrow()
    }

    #[inline]
    pub const fn as_value_ref(&self) -> ValueRef {
        self.value
    }

    #[inline]
    pub fn as_operand_ref(&self) -> OpOperand {
        unsafe { OpOperand::from_raw(self) }
    }

    pub fn owner(&self) -> EntityRef<'_, crate::Operation> {
        self.owner.borrow()
    }

    pub fn ty(&self) -> crate::Type {
        self.value().ty().clone()
    }

    pub fn operand_group(&self) -> u8 {
        let owner = self.owner.borrow();
        let operands = owner.operands();
        let operand_index = self.index as usize;
        let group_index = operands
            .groups()
            .position(|group| group.range().contains(&operand_index))
            .expect("broken operand reference!");
        group_index as u8
    }

    /// Set the operand value to `value`, removing the operand from the use list of the previous
    /// value, and adding it to the use list of `value`.
    pub fn set(&mut self, value: ValueRef) {
        let this = self.as_operand_ref();
        let mut prev = self.value;
        unsafe {
            let mut prev = prev.borrow_mut();
            prev.uses_mut().cursor_mut_from_ptr(this).remove();
        }
        self.value = value;
        self.value.borrow_mut().insert_use(this);
    }
}
impl fmt::Debug for OpOperandImpl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        #[derive(Debug)]
        #[allow(unused)]
        struct ValueInfo<'a> {
            id: ValueId,
            ty: &'a Type,
        }

        let value = self.value.borrow();
        let id = value.id();
        let ty = value.ty();
        f.debug_struct("OpOperand")
            .field("index", &self.index)
            .field("value", &ValueInfo { id, ty })
            .finish_non_exhaustive()
    }
}
impl crate::Spanned for OpOperandImpl {
    fn span(&self) -> crate::SourceSpan {
        self.value.borrow().span()
    }
}
impl crate::Entity for OpOperandImpl {}
impl crate::StorableEntity for OpOperandImpl {
    #[inline(always)]
    fn index(&self) -> usize {
        self.index as usize
    }

    unsafe fn set_index(&mut self, index: usize) {
        self.index = index.try_into().expect("too many operands");
    }

    fn unlink(&mut self) {
        let ptr = self.as_operand_ref();
        let mut value = self.value.borrow_mut();
        let uses = value.uses_mut();
        unsafe {
            let mut cursor = uses.cursor_mut_from_ptr(ptr);
            cursor.remove();
        }
    }
}

pub type OpOperandStorage = crate::EntityStorage<OpOperand, 1>;
pub type OpOperandRange<'a> = crate::EntityRange<'a, OpOperand>;
pub type OpOperandRangeMut<'a> = crate::EntityRangeMut<'a, OpOperand, 1>;
