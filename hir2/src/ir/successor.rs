use core::fmt;

use super::OpOperandStorage;
use crate::{AttributeValue, BlockOperandRef, BlockRef, OpOperandRange, OpOperandRangeMut};

pub type OpSuccessorStorage = crate::EntityStorage<SuccessorInfo, 0>;
pub type OpSuccessorRange<'a> = crate::EntityRange<'a, SuccessorInfo>;
pub type OpSuccessorRangeMut<'a> = crate::EntityRangeMut<'a, SuccessorInfo, 0>;

/// This trait represents successor-like values for operations, with support for control-flow
/// predicated on a "key", a sentinel value that must match in order for the successor block to be
/// taken.
///
/// The ability to associate a successor with a user-defined key, is intended for modeling things
/// such as [crate::dialects::hir::Switch], which has one or more successors which are guarded by
/// an integer value that is matched against the input, or selector, value. Most importantly, doing
/// so in a way that keeps everything in sync as the IR is modified.
///
/// When used as a successor argument to an operation, each successor gets its own operand group,
/// and if it has an associated key, keyed successors are stored in a special attribute which tracks
/// each key and its associated successor index. This allows requesting the successor details and
/// getting back the correct key, destination, and operands.
pub trait KeyedSuccessor {
    /// The type of key this successor
    type Key: AttributeValue + Clone + Eq;
    /// The type of value which will represent a reference to this successor.
    ///
    /// You should use [OpSuccessor] if this successor is not keyed.
    type Repr<'a>: 'a;
    /// The type of value which will represent a mutable reference to this successor.
    ///
    /// You should use [OpSuccessorMut] if this successor is not keyed.
    type ReprMut<'a>: 'a;

    /// The (optional) value of the key for this successor.
    ///
    /// Keys must be valid attribute values, as they will be encoded in the operation attributes.
    ///
    /// If `None` is returned, this successor is to be treated like a regular successor argument,
    /// i.e. a destination block and associated operands. If a key is returned, the key must be
    /// unique across the set of keyed successors.
    fn key(&self) -> &Self::Key;
    /// Convert this value into the raw parts comprising the successor information:
    ///
    /// * The (optional) key under which this successor is selected
    /// * The destination block
    /// * The destination operands
    fn into_parts(self) -> (Self::Key, BlockRef, Vec<super::ValueRef>);
    fn into_repr(
        key: Self::Key,
        block: BlockOperandRef,
        operands: OpOperandRange<'_>,
    ) -> Self::Repr<'_>;
    fn into_repr_mut(
        key: Self::Key,
        block: BlockOperandRef,
        operands: OpOperandRangeMut<'_>,
    ) -> Self::ReprMut<'_>;
}

/// This struct tracks successor metadata needed by [crate::Operation]
#[derive(Debug, Clone)]
pub struct SuccessorInfo {
    pub block: BlockOperandRef,
    pub(crate) key: Option<core::ptr::NonNull<()>>,
    pub(crate) operand_group: u8,
}
impl crate::StorableEntity for SuccessorInfo {
    #[inline(always)]
    fn index(&self) -> usize {
        self.block.index()
    }

    #[inline(always)]
    unsafe fn set_index(&mut self, index: usize) {
        self.block.set_index(index);
    }

    #[inline(always)]
    fn unlink(&mut self) {
        self.block.unlink();
    }
}

/// An [OpSuccessor] is a BlockOperand + OpOperandRange for that block
pub struct OpSuccessor<'a> {
    pub dest: BlockOperandRef,
    pub arguments: OpOperandRange<'a>,
}
impl fmt::Debug for OpSuccessor<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OpSuccessor")
            .field("block", &self.dest.borrow().block_id())
            .field_with("arguments", |f| f.debug_list().entries(self.arguments.iter()).finish())
            .finish()
    }
}

/// An [OpSuccessorMut] is a BlockOperand + OpOperandRangeMut for that block
pub struct OpSuccessorMut<'a> {
    pub dest: BlockOperandRef,
    pub arguments: OpOperandRangeMut<'a>,
}
impl fmt::Debug for OpSuccessorMut<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OpSuccessorMut")
            .field("block", &self.dest.borrow().block_id())
            .field_with("arguments", |f| f.debug_list().entries(self.arguments.iter()).finish())
            .finish()
    }
}

pub struct KeyedSuccessorRange<'a, T> {
    range: OpSuccessorRange<'a>,
    operands: &'a OpOperandStorage,
    _marker: core::marker::PhantomData<T>,
}
impl<'a, T> KeyedSuccessorRange<'a, T> {
    pub fn new(range: OpSuccessorRange<'a>, operands: &'a OpOperandStorage) -> Self {
        Self {
            range,
            operands,
            _marker: core::marker::PhantomData,
        }
    }

    pub fn get(&self, index: usize) -> Option<SuccessorWithKey<'_, T>> {
        self.range.get(index).map(|info| {
            let operands = self.operands.group(info.operand_group as usize);
            SuccessorWithKey {
                info,
                operands,
                _marker: core::marker::PhantomData,
            }
        })
    }
}

pub struct KeyedSuccessorRangeMut<'a, T> {
    range: OpSuccessorRangeMut<'a>,
    operands: &'a mut OpOperandStorage,
    _marker: core::marker::PhantomData<T>,
}
impl<'a, T> KeyedSuccessorRangeMut<'a, T> {
    pub fn new(range: OpSuccessorRangeMut<'a>, operands: &'a mut OpOperandStorage) -> Self {
        Self {
            range,
            operands,
            _marker: core::marker::PhantomData,
        }
    }

    pub fn get(&self, index: usize) -> Option<SuccessorWithKey<'_, T>> {
        self.range.get(index).map(|info| {
            let operands = self.operands.group(info.operand_group as usize);
            SuccessorWithKey {
                info,
                operands,
                _marker: core::marker::PhantomData,
            }
        })
    }

    pub fn get_mut(&mut self, index: usize) -> Option<SuccessorWithKeyMut<'_, T>> {
        self.range.get_mut(index).map(|info| {
            let operands = self.operands.group_mut(info.operand_group as usize);
            SuccessorWithKeyMut {
                info,
                operands,
                _marker: core::marker::PhantomData,
            }
        })
    }
}

pub struct SuccessorWithKey<'a, T> {
    info: &'a SuccessorInfo,
    operands: OpOperandRange<'a>,
    _marker: core::marker::PhantomData<T>,
}
impl<'a, T> SuccessorWithKey<'a, T> {
    pub fn key(&self) -> Option<&T> {
        self.info.key.map(|ptr| unsafe { &*(ptr.as_ptr() as *mut T) })
    }

    pub fn block(&self) -> BlockRef {
        self.info.block.borrow().block.clone()
    }

    #[inline(always)]
    pub fn arguments(&self) -> &OpOperandRange<'a> {
        &self.operands
    }
}

pub struct SuccessorWithKeyMut<'a, T> {
    info: &'a SuccessorInfo,
    operands: OpOperandRangeMut<'a>,
    _marker: core::marker::PhantomData<T>,
}
impl<'a, T> SuccessorWithKeyMut<'a, T> {
    pub fn key(&self) -> Option<&T> {
        self.info.key.map(|ptr| unsafe { &*(ptr.as_ptr() as *mut T) })
    }

    pub fn block(&self) -> BlockRef {
        self.info.block.borrow().block.clone()
    }

    #[inline(always)]
    pub fn arguments(&self) -> &OpOperandRangeMut<'a> {
        &self.operands
    }

    #[inline(always)]
    pub fn arguments_mut(&mut self) -> &mut OpOperandRangeMut<'a> {
        &mut self.operands
    }
}
