use core::fmt;

use super::OpOperandStorage;
use crate::{AttributeValue, BlockOperandRef, BlockRef, OpOperandRange, OpOperandRangeMut};

pub type OpSuccessorStorage = crate::EntityStorage<SuccessorInfo, 0>;
pub type OpSuccessorRange<'a> = crate::EntityRange<'a, SuccessorInfo>;
pub type OpSuccessorRangeMut<'a> = crate::EntityRangeMut<'a, SuccessorInfo, 0>;

/// This trait represents common behavior shared by any range of successor operands.
pub trait SuccessorOperands {
    /// Returns true if there are no operands in this set
    fn is_empty(&self) -> bool {
        self.num_produced() == 0 && self.len() == 0
    }
    /// Returns the total number of operands in this set
    fn len(&self) -> usize;
    /// Returns the number of internally produced operands in this set
    fn num_produced(&self) -> usize;
    /// Returns true if the operand at `index` is internally produced
    #[inline]
    fn is_operand_produced(&self, index: usize) -> bool {
        index < self.num_produced()
    }
    /// Get the range of forwarded operands
    fn forwarded(&self) -> OpOperandRange<'_>;
    /// Get a [SuccessorOperand] representing the operand at `index`
    ///
    /// Returns `None` if the index is out of range.
    fn get(&self, index: usize) -> Option<SuccessorOperand> {
        if self.is_operand_produced(index) {
            Some(SuccessorOperand::Produced)
        } else {
            self.forwarded()
                .get(index)
                .map(|op_operand| SuccessorOperand::Forwarded(op_operand.borrow().as_value_ref()))
        }
    }

    /// Get a [SuccessorOperand] representing the operand at `index`.
    ///
    /// Panics if the index is out of range.
    fn get_unchecked(&self, index: usize) -> SuccessorOperand {
        if self.is_operand_produced(index) {
            SuccessorOperand::Produced
        } else {
            SuccessorOperand::Forwarded(self.forwarded()[index].borrow().as_value_ref())
        }
    }

    /// Gets the index of the forwarded operand which maps to the given successor block argument
    /// index.
    ///
    /// Panics if the given block argument index does not correspond to a forwarded operand.
    fn get_operand_index(&self, block_argument_index: usize) -> usize {
        assert!(
            self.is_operand_produced(block_argument_index),
            "cannot map operands produced by the operation"
        );
        let base_index = self.forwarded()[0].borrow().index as usize;
        base_index + (block_argument_index - self.num_produced())
    }
}

/// This type models how operands are forwarded to block arguments in control flow. It consists of a
/// number, denoting how many of the successor block arguments are produced by the operation,
/// followed by a range of operands that are forwarded. The produced operands are passed to the
/// first few block arguments of the successor, followed by the forwarded operands. It is
/// unsupported to pass them in a different order.
///
/// An example operation with both of these concepts would be a branch-on-error operation, that
/// internally produces an error object on the error path:
///
/// ```hir,ignore
/// invoke %function(%0)
///    label ^success ^error(%1 : i32)
///
/// ^error(%e: !error, %arg0 : i32):
///     ...
/// ```
///
/// This operation would return an instance of [SuccessorOperands] with a produced operand count of
/// 1 (mapped to `%e` in the successor) and forwarded operands consisting of `%1` in the example
/// above (mapped to `%arg0` in the successor).
pub struct SuccessorOperandRange<'a> {
    /// The explicit op operands which are to be passed along to the successor
    forwarded: OpOperandRange<'a>,
    /// The number of operands that are produced internally within the operation and which are to
    /// be passed to the successor before any forwarded operands.
    num_produced: usize,
}
impl<'a> SuccessorOperandRange<'a> {
    /// Create an empty successor operand set
    pub fn empty() -> Self {
        Self {
            forwarded: OpOperandRange::empty(),
            num_produced: 0,
        }
    }

    /// Create a successor operand set consisting solely of forwarded op operands
    #[inline]
    pub const fn forward(forwarded: OpOperandRange<'a>) -> Self {
        Self {
            forwarded,
            num_produced: 0,
        }
    }

    /// Create a successor operand set consisting solely of `num_produced` internally produced
    /// results
    pub fn produced(num_produced: usize) -> Self {
        Self {
            forwarded: OpOperandRange::empty(),
            num_produced,
        }
    }

    /// Create a new successor operand set with the given number of internally produced results,
    /// and forwarded op operands.
    #[inline]
    pub const fn new(num_produced: usize, forwarded: OpOperandRange<'a>) -> Self {
        Self {
            forwarded,
            num_produced,
        }
    }
}
impl<'a> SuccessorOperands for SuccessorOperandRange<'a> {
    #[inline]
    fn len(&self) -> usize {
        self.num_produced + self.forwarded.len()
    }

    #[inline(always)]
    fn num_produced(&self) -> usize {
        self.num_produced
    }

    fn forwarded(&self) -> OpOperandRange<'_> {
        self.forwarded.clone()
    }
}

/// The mutable variant of [SuccessorOperandsRange].
pub struct SuccessorOperandRangeMut<'a> {
    /// The explicit op operands which are to be passed along to the successor
    forwarded: OpOperandRangeMut<'a>,
    /// The number of operands that are produced internally within the operation and which are to
    /// be passed to the successor before any forwarded operands.
    num_produced: usize,
}
impl<'a> SuccessorOperandRangeMut<'a> {
    /// Create a successor operand set consisting solely of forwarded op operands
    #[inline]
    pub const fn forward(forwarded: OpOperandRangeMut<'a>) -> Self {
        Self {
            forwarded,
            num_produced: 0,
        }
    }

    /// Create a new successor operand set with the given number of internally produced results,
    /// and forwarded op operands.
    #[inline]
    pub const fn new(num_produced: usize, forwarded: OpOperandRangeMut<'a>) -> Self {
        Self {
            forwarded,
            num_produced,
        }
    }

    #[inline(always)]
    pub fn forwarded_mut(&mut self) -> &mut OpOperandRangeMut<'a> {
        &mut self.forwarded
    }
}
impl<'a> SuccessorOperands for SuccessorOperandRangeMut<'a> {
    #[inline]
    fn len(&self) -> usize {
        self.num_produced + self.forwarded.len()
    }

    #[inline(always)]
    fn num_produced(&self) -> usize {
        self.num_produced
    }

    #[inline(always)]
    fn forwarded(&self) -> OpOperandRange<'_> {
        self.forwarded.as_immutable()
    }
}

/// Represents an operand in a [SuccessorOperands] set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SuccessorOperand {
    /// This operand is internally produced by the operation, and passed to the successor before
    /// any forwarded op operands.
    Produced,
    /// This operand is a forwarded operand of the operation.
    Forwarded(crate::ValueRef),
}

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

    pub fn iter(&self) -> KeyedSuccessorRangeIter<'a, '_, T> {
        KeyedSuccessorRangeIter {
            range: self,
            index: 0,
        }
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

pub struct KeyedSuccessorRangeIter<'a, 'b: 'a, T> {
    range: &'b KeyedSuccessorRange<'a, T>,
    index: usize,
}
impl<'a, 'b: 'a, T> Iterator for KeyedSuccessorRangeIter<'a, 'b, T> {
    type Item = SuccessorWithKey<'b, T>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.index >= self.range.range.len() {
            return None;
        }

        let idx = self.index;
        self.index += 1;
        self.range.get(idx)
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
