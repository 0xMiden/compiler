use core::fmt;

use super::Context;
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
    pub value: Option<ValueRef>,
    /// The owner of this operand, i.e. the operation it is an operand of
    pub owner: OperationRef,
    /// The index of this operand in the operand list of an operation
    pub index: u8,
}
impl OpOperandImpl {
    #[inline]
    pub fn new(value: ValueRef, owner: OperationRef, index: u8) -> Self {
        Self {
            value: Some(value),
            owner,
            index,
        }
    }

    #[track_caller]
    pub fn value(&self) -> EntityRef<'_, dyn Value> {
        self.value.as_ref().expect("operand is unlinked").borrow()
    }

    #[inline]
    pub const fn as_value_ref(&self) -> ValueRef {
        self.value.unwrap()
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
    pub fn set(&mut self, mut value: ValueRef) {
        let this = self.as_operand_ref();
        if let Some(mut prev) = self.value.take() {
            unsafe {
                let mut prev = prev.borrow_mut();
                prev.uses_mut().cursor_mut_from_ptr(this).remove();
            }
        }
        self.value = Some(value);
        value.borrow_mut().insert_use(this);
    }
}
impl fmt::Debug for OpOperand {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        fmt::Debug::fmt(&*self.borrow(), f)
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

        let value = self.value.map(|value| value.borrow());
        let value = value.as_ref().map(|value| ValueInfo {
            id: value.id(),
            ty: value.ty(),
        });
        f.debug_struct("OpOperand")
            .field("index", &self.index)
            .field("value", &value)
            .finish_non_exhaustive()
    }
}
impl crate::Spanned for OpOperandImpl {
    fn span(&self) -> crate::SourceSpan {
        self.value().span()
    }
}
impl crate::Entity for OpOperandImpl {}
impl crate::EntityListItem for OpOperandImpl {}
impl crate::StorableEntity for OpOperandImpl {
    #[inline(always)]
    fn index(&self) -> usize {
        self.index as usize
    }

    unsafe fn set_index(&mut self, index: usize) {
        self.index = index.try_into().expect("too many operands");
    }

    fn unlink(&mut self) {
        if !self.as_operand_ref().is_linked() {
            return;
        }
        if let Some(mut value) = self.value.take() {
            let ptr = self.as_operand_ref();
            let mut value = value.borrow_mut();
            let uses = value.uses_mut();
            unsafe {
                let mut cursor = uses.cursor_mut_from_ptr(ptr);
                cursor.remove();
            }
        }
    }
}

pub type OpOperandStorage = crate::EntityStorage<OpOperand, 1>;
pub type OpOperandRange<'a> = crate::EntityRange<'a, OpOperand>;
pub type OpOperandRangeMut<'a> = crate::EntityRangeMut<'a, OpOperand, 1>;

impl OpOperandRangeMut<'_> {
    pub fn set_operands<I>(&mut self, operands: I, owner: OperationRef, context: &Context)
    where
        I: IntoIterator<Item = ValueRef>,
    {
        let mut operands = operands.into_iter().enumerate();
        let mut num_operands = 0;
        while let Some((index, value)) = operands.next() {
            if let Some(operand_ref) = self.get_mut(index) {
                num_operands += 1;
                let mut operand = operand_ref.borrow_mut();
                // If the new operand value and the existing one are the same, no change is required
                if operand.value.is_some_and(|v| v == value) {
                    continue;
                }
                // Otherwise, set the operand value to the new value
                operand.set(value);
            } else {
                // The operand group is being extended
                self.extend(core::iter::once((index, value)).chain(operands).map(|(_, value)| {
                    num_operands += 1;
                    context.make_operand(value, owner, 0)
                }));
                break;
            }
        }

        // Remove excess operands
        if num_operands < self.len() {
            for _ in 0..(self.len() - num_operands) {
                let _ = self.pop();
            }
        }
    }
}
