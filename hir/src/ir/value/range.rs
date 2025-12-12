use core::borrow::Borrow;

use smallvec::{smallvec, SmallVec};

use super::*;
use crate::adt::SmallSet;

/// An abstraction over the different type of [Value]-derived entity ranges, e.g. operands, results,
/// block arguments, vs type-erased collections.
///
/// This range types supports fewer conveniences than [EntityRange] or raw slices provide, as we
/// are not able to handle all ranges exactly the same (for example, borrowing an element from the
/// range works for all but op operands, as those have an extra layer of indirection).
///
/// In general, this should be used in only narrow circumstances where a more specific range type
/// cannot be used.
#[derive(Default)]
pub enum ValueRange<'a, const N: usize = 2> {
    /// A default-initialized empty range
    #[default]
    Empty,
    /// The values in the range are type-erased, but owned
    Owned(SmallVec<[ValueRef; N]>),
    /// The values in the range are type-erased, but borrowed
    Borrowed(&'a [ValueRef]),
    /// The value range contains block arguments
    BlockArguments(&'a [BlockArgumentRef]),
    /// The value range contains operands
    Operands(&'a [OpOperand]),
    /// The value range contains results
    Results(&'a [OpResultRef]),
}

impl<'values, const N: usize> ValueRange<'values, N> {
    /// Returns true if this range is empty
    pub fn is_empty(&self) -> bool {
        match self {
            Self::Empty => true,
            Self::Owned(values) => values.is_empty(),
            Self::Borrowed(values) => values.is_empty(),
            Self::BlockArguments(range) => range.is_empty(),
            Self::Operands(range) => range.is_empty(),
            Self::Results(range) => range.is_empty(),
        }
    }

    /// Returns the number of values in the range
    pub fn len(&self) -> usize {
        match self {
            Self::Empty => 0,
            Self::Owned(values) => values.len(),
            Self::Borrowed(values) => values.len(),
            Self::BlockArguments(range) => range.len(),
            Self::Operands(range) => range.len(),
            Self::Results(range) => range.len(),
        }
    }

    pub fn slice<'a, 'b: 'a + 'values, R>(&'b self, range: R) -> ValueRange<'a, N>
    where
        R: core::slice::SliceIndex<[ValueRef], Output = [ValueRef]>,
        R: core::slice::SliceIndex<[BlockArgumentRef], Output = [BlockArgumentRef]>,
        R: core::slice::SliceIndex<[OpOperand], Output = [OpOperand]>,
        R: core::slice::SliceIndex<[OpResultRef], Output = [OpResultRef]>,
    {
        match self {
            Self::Empty => ValueRange::Empty,
            Self::Owned(values) => Self::Borrowed(&values[range]),
            Self::Borrowed(values) => Self::Borrowed(&values[range]),
            Self::BlockArguments(values) => Self::BlockArguments(&values[range]),
            Self::Operands(values) => Self::Operands(&values[range]),
            Self::Results(values) => Self::Results(&values[range]),
        }
    }

    /// Returns the value at `index` in the range
    pub fn get(&self, index: usize) -> Option<ValueRef> {
        match self {
            Self::Empty => None,
            Self::Owned(values) => values.get(index).cloned(),
            Self::Borrowed(values) => values.get(index).cloned(),
            Self::BlockArguments(range) => {
                range.get(index).map(|operand| operand.borrow().as_value_ref())
            }
            Self::Operands(range) => {
                range.get(index).map(|operand| operand.borrow().as_value_ref())
            }
            Self::Results(range) => range.get(index).map(|result| result.borrow().as_value_ref()),
        }
    }

    /// Returns true if `value` is present in this range
    pub fn contains<V>(&self, value: V) -> bool
    where
        V: Borrow<ValueRef>,
    {
        let value = value.borrow();
        match self {
            Self::Empty => false,
            Self::Owned(values) => values.contains(value),
            Self::Borrowed(values) => values.contains(value),
            Self::BlockArguments(args) => args.iter().copied().any(|arg| arg as ValueRef == *value),
            Self::Operands(operands) => {
                operands.iter().any(|operand| operand.borrow().as_value_ref() == *value)
            }
            Self::Results(results) => {
                results.iter().copied().any(|result| result as ValueRef == *value)
            }
        }
    }

    /// Iterate over the values in this range as [ValueRef]s
    pub fn iter(&self) -> ValueRangeIter<'_, N> {
        match self {
            Self::Empty => ValueRangeIter::new(ValueRange::Borrowed(&[])),
            Self::Owned(values) => ValueRangeIter::new(ValueRange::Borrowed(values.as_slice())),
            Self::Borrowed(values) => ValueRangeIter::new(ValueRange::Borrowed(values)),
            Self::BlockArguments(values) => ValueRangeIter::new(ValueRange::BlockArguments(values)),
            Self::Operands(values) => ValueRangeIter::new(ValueRange::Operands(values)),
            Self::Results(values) => ValueRangeIter::new(ValueRange::Results(values)),
        }
    }

    /// Convert this into an owned [ValueRange] with static lifetime
    pub fn into_owned(self) -> ValueRange<'static, N> {
        ValueRange::Owned(self.into_smallvec())
    }

    /// Convert this into an owned [SmallVec] of the underlying values.
    pub fn into_smallvec(self) -> SmallVec<[ValueRef; N]> {
        match self {
            Self::Empty => smallvec![],
            Self::Owned(values) => values,
            Self::Borrowed(values) => SmallVec::from_slice(values),
            Self::BlockArguments(args) => args.iter().copied().map(|arg| arg as ValueRef).collect(),
            Self::Operands(operands) => {
                operands.iter().map(|operand| operand.borrow().as_value_ref()).collect()
            }
            Self::Results(results) => {
                results.iter().copied().map(|result| result as ValueRef).collect()
            }
        }
    }

    /// Obtain a [alloc::vec::Vec] from this range.
    pub fn to_vec(&self) -> alloc::vec::Vec<ValueRef> {
        match self {
            Self::Empty => Default::default(),
            Self::Owned(values) => values.to_vec(),
            Self::Borrowed(values) => values.to_vec(),
            Self::BlockArguments(args) => args.iter().copied().map(|arg| arg as ValueRef).collect(),
            Self::Operands(operands) => {
                operands.iter().map(|operand| operand.borrow().as_value_ref()).collect()
            }
            Self::Results(results) => {
                results.iter().copied().map(|result| result as ValueRef).collect()
            }
        }
    }
}

impl<const N: usize> core::fmt::Debug for ValueRange<'_, N> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

impl<const N: usize> core::fmt::Display for ValueRange<'_, N> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let mut builder = f.debug_list();
        for value in self.iter() {
            builder.entry_with(|f| write!(f, "{value}"));
        }

        builder.finish()
    }
}

impl<const N: usize> FromIterator<ValueRef> for ValueRange<'static, N> {
    fn from_iter<I>(iter: I) -> Self
    where
        I: IntoIterator<Item = ValueRef>,
    {
        Self::Owned(SmallVec::from_iter(iter))
    }
}

impl<const N: usize> FromIterator<BlockArgumentRef> for ValueRange<'static, N> {
    fn from_iter<I>(iter: I) -> Self
    where
        I: IntoIterator<Item = BlockArgumentRef>,
    {
        Self::from_iter(iter.into_iter().map(|arg| arg as ValueRef))
    }
}

impl<const N: usize> FromIterator<OpResultRef> for ValueRange<'static, N> {
    fn from_iter<I>(iter: I) -> Self
    where
        I: IntoIterator<Item = OpResultRef>,
    {
        Self::from_iter(iter.into_iter().map(|result| result as ValueRef))
    }
}

impl<const N: usize> FromIterator<OpOperand> for ValueRange<'static, N> {
    fn from_iter<I>(iter: I) -> Self
    where
        I: IntoIterator<Item = OpOperand>,
    {
        Self::from_iter(iter.into_iter().map(|operand| operand.borrow().as_value_ref()))
    }
}

impl<'a, const N: usize> IntoIterator for ValueRange<'a, N> {
    type IntoIter = ValueRangeIter<'a, N>;
    type Item = ValueRef;

    fn into_iter(self) -> Self::IntoIter {
        ValueRangeIter::new(self)
    }
}

impl<'a, 'b: 'a, const N: usize> IntoIterator for &'a ValueRange<'b, N> {
    type IntoIter = ValueRangeIter<'a, N>;
    type Item = ValueRef;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl<const N: usize> From<SmallVec<[ValueRef; N]>> for ValueRange<'static, N> {
    fn from(values: SmallVec<[ValueRef; N]>) -> Self {
        Self::Owned(SmallVec::from_slice(&values))
    }
}

impl<'a, const M: usize, const N: usize> From<&'a SmallVec<[ValueRef; M]>> for ValueRange<'a, N> {
    fn from(values: &'a SmallVec<[ValueRef; M]>) -> Self {
        Self::Borrowed(values.as_slice())
    }
}

impl<const N: usize> From<SmallSet<ValueRef, N>> for ValueRange<'static, N> {
    fn from(values: SmallSet<ValueRef, N>) -> Self {
        Self::Owned(values.into_vec())
    }
}

impl<'a, const M: usize, const N: usize> From<&'a SmallSet<ValueRef, M>> for ValueRange<'a, N> {
    fn from(values: &'a SmallSet<ValueRef, M>) -> Self {
        Self::Borrowed(values.as_slice())
    }
}

impl<'a, const N: usize> From<&'a [ValueRef]> for ValueRange<'a, N> {
    fn from(values: &'a [ValueRef]) -> Self {
        Self::Borrowed(values)
    }
}

impl<'a, const N: usize> From<BlockArgumentRange<'a>> for ValueRange<'a, N> {
    fn from(range: BlockArgumentRange<'a>) -> Self {
        Self::BlockArguments(range.as_slice())
    }
}

impl<'a, const N: usize> From<&'a [BlockArgumentRef]> for ValueRange<'a, N> {
    fn from(range: &'a [BlockArgumentRef]) -> Self {
        Self::BlockArguments(range)
    }
}

impl<'a, const N: usize> From<OpOperandRange<'a>> for ValueRange<'a, N> {
    fn from(range: OpOperandRange<'a>) -> Self {
        Self::Operands(range.as_slice())
    }
}

impl<'a, const N: usize> From<&'a [OpOperand]> for ValueRange<'a, N> {
    fn from(range: &'a [OpOperand]) -> Self {
        Self::Operands(range)
    }
}

impl<'a, const N: usize> From<OpResultRange<'a>> for ValueRange<'a, N> {
    fn from(range: OpResultRange<'a>) -> Self {
        Self::Results(range.as_slice())
    }
}

impl<'a, const N: usize> From<&'a [OpResultRef]> for ValueRange<'a, N> {
    fn from(range: &'a [OpResultRef]) -> Self {
        Self::Results(range)
    }
}

impl<'a, const N: usize> From<SuccessorOperandRange<'a>> for ValueRange<'a, N> {
    fn from(range: SuccessorOperandRange<'a>) -> Self {
        Self::from(range.into_forwarded())
    }
}

pub trait AsValueRange {
    fn as_value_range(&self) -> ValueRange<'_, 2>;
}

impl<T: AsValueRange> AsValueRange for Option<T> {
    fn as_value_range(&self) -> ValueRange<'_, 2> {
        match self.as_ref() {
            Some(values) => values.as_value_range(),
            None => ValueRange::Empty,
        }
    }
}

impl AsValueRange for [ValueRef] {
    fn as_value_range(&self) -> ValueRange<'_, 2> {
        ValueRange::Borrowed(self)
    }
}

impl AsValueRange for [BlockArgumentRef] {
    fn as_value_range(&self) -> ValueRange<'_, 2> {
        ValueRange::BlockArguments(self)
    }
}

impl AsValueRange for [OpOperand] {
    fn as_value_range(&self) -> ValueRange<'_, 2> {
        ValueRange::Operands(self)
    }
}

impl AsValueRange for [OpResultRef] {
    fn as_value_range(&self) -> ValueRange<'_, 2> {
        ValueRange::Results(self)
    }
}

impl AsValueRange for alloc::vec::Vec<ValueRef> {
    fn as_value_range(&self) -> ValueRange<'_, 2> {
        ValueRange::Borrowed(self.as_slice())
    }
}

impl<const N: usize> AsValueRange for SmallVec<[ValueRef; N]> {
    fn as_value_range(&self) -> ValueRange<'_, 2> {
        ValueRange::Borrowed(self.as_slice())
    }
}

impl AsValueRange for OpOperandStorage {
    fn as_value_range(&self) -> ValueRange<'_, 2> {
        ValueRange::Operands(self.all().as_slice())
    }
}

impl AsValueRange for OpResultStorage {
    fn as_value_range(&self) -> ValueRange<'_, 2> {
        ValueRange::Results(self.all().as_slice())
    }
}

impl AsValueRange for OpOperandRange<'_> {
    fn as_value_range(&self) -> ValueRange<'_, 2> {
        ValueRange::Operands(self.as_slice())
    }
}

impl AsValueRange for OpOperandRangeMut<'_> {
    fn as_value_range(&self) -> ValueRange<'_, 2> {
        ValueRange::Operands(self.as_slice())
    }
}

impl AsValueRange for OpResultRange<'_> {
    fn as_value_range(&self) -> ValueRange<'_, 2> {
        ValueRange::Results(self.as_slice())
    }
}

impl AsValueRange for OpResultRangeMut<'_> {
    fn as_value_range(&self) -> ValueRange<'_, 2> {
        ValueRange::Results(self.as_slice())
    }
}

impl AsValueRange for BlockArgumentRange<'_> {
    fn as_value_range(&self) -> ValueRange<'_, 2> {
        ValueRange::BlockArguments(self.as_slice())
    }
}

impl AsValueRange for BlockArgumentRangeMut<'_> {
    fn as_value_range(&self) -> ValueRange<'_, 2> {
        ValueRange::BlockArguments(self.as_slice())
    }
}

/// An iterator consuming the contents of a [ValueRange]
pub struct ValueRangeIter<'a, const N: usize> {
    range: ValueRange<'a, N>,
    index: usize,
}

impl<'a, const N: usize> ValueRangeIter<'a, N> {
    pub fn new(range: ValueRange<'a, N>) -> Self {
        Self { range, index: 0 }
    }
}

impl<const N: usize> core::iter::FusedIterator for ValueRangeIter<'_, N> {}
impl<const N: usize> ExactSizeIterator for ValueRangeIter<'_, N> {
    fn len(&self) -> usize {
        let len = self.range.len();
        len.saturating_sub(self.index)
    }
}
impl<const N: usize> Iterator for ValueRangeIter<'_, N> {
    type Item = ValueRef;

    fn next(&mut self) -> Option<Self::Item> {
        let len = self.range.len();
        if self.index >= len {
            return None;
        }

        let index = self.index;
        self.index += 1;
        match &self.range {
            ValueRange::Empty => None,
            ValueRange::Owned(values) => values.get(index).cloned(),
            ValueRange::Borrowed(values) => values.get(index).cloned(),
            ValueRange::BlockArguments(range) => {
                range.get(index).map(|o| o.borrow().as_value_ref())
            }
            ValueRange::Operands(range) => range.get(index).map(|o| o.borrow().as_value_ref()),
            ValueRange::Results(range) => range.get(index).map(|o| o.borrow().as_value_ref()),
        }
    }
}

impl<const N: usize> DoubleEndedIterator for ValueRangeIter<'_, N> {
    fn next_back(&mut self) -> Option<Self::Item> {
        let len = self.range.len();
        let index = len.checked_sub(self.index + 1)?;

        self.index += 1;
        match &self.range {
            ValueRange::Empty => None,
            ValueRange::Owned(values) => values.get(index).cloned(),
            ValueRange::Borrowed(values) => values.get(index).cloned(),
            ValueRange::BlockArguments(range) => {
                range.get(index).map(|o| o.borrow().as_value_ref())
            }
            ValueRange::Operands(range) => range.get(index).map(|o| o.borrow().as_value_ref()),
            ValueRange::Results(range) => range.get(index).map(|o| o.borrow().as_value_ref()),
        }
    }
}
