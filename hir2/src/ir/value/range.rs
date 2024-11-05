use smallvec::SmallVec;

use super::*;

/// An abstraction over the different type of [Value]-derived entity ranges, e.g. operands, results,
/// block arguments, vs type-erased collections.
///
/// This range types supports fewer conveniences than [EntityRange] or raw slices provide, as we
/// are not able to handle all ranges exactly the same (for example, borrowing an element from the
/// range works for all but op operands, as those have an extra layer of indirection).
///
/// In general, this should be used in only narrow circumstances where a more specific range type
/// cannot be used.
pub enum ValueRange<'a> {
    /// The values in the range are type-erased, but owned
    Owned(SmallVec<[ValueRef; 2]>),
    /// The values in the range are type-erased, but borrowed
    Borrowed(&'a [ValueRef]),
    /// The value range contains block arguments
    BlockArguments(BlockArgumentRange<'a>),
    /// The value range contains operands
    Operands(OpOperandRange<'a>),
    /// The value range contains results
    Results(OpResultRange<'a>),
}

impl<'a> ValueRange<'a> {
    /// Returns true if this range is empty
    pub fn is_empty(&self) -> bool {
        match self {
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
            Self::Owned(values) => values.len(),
            Self::Borrowed(values) => values.len(),
            Self::BlockArguments(range) => range.len(),
            Self::Operands(range) => range.len(),
            Self::Results(range) => range.len(),
        }
    }

    /// Returns the value at `index` in the range
    pub fn get(&self, index: usize) -> Option<ValueRef> {
        match self {
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

    /// Iterate over the values in this range as [ValueRef]s
    pub fn iter(&self) -> ValueRangeIter<'_, 'a> {
        ValueRangeIter::new(self)
    }
}

impl<'a> From<SmallVec<[ValueRef; 2]>> for ValueRange<'a> {
    fn from(values: SmallVec<[ValueRef; 2]>) -> Self {
        Self::Owned(values)
    }
}

impl<'a> From<&'a [ValueRef]> for ValueRange<'a> {
    fn from(values: &'a [ValueRef]) -> Self {
        Self::Borrowed(values)
    }
}

impl<'a> From<BlockArgumentRange<'a>> for ValueRange<'a> {
    fn from(range: BlockArgumentRange<'a>) -> Self {
        Self::BlockArguments(range)
    }
}

impl<'a> From<OpOperandRange<'a>> for ValueRange<'a> {
    fn from(range: OpOperandRange<'a>) -> Self {
        Self::Operands(range)
    }
}

impl<'a> From<OpResultRange<'a>> for ValueRange<'a> {
    fn from(range: OpResultRange<'a>) -> Self {
        Self::Results(range)
    }
}

/// An iterator over the contents of a [ValueRange]
pub struct ValueRangeIter<'a, 'b: 'a> {
    range: &'a ValueRange<'b>,
    index: usize,
}

impl<'a, 'b: 'a> ValueRangeIter<'a, 'b> {
    pub fn new(range: &'a ValueRange<'b>) -> Self {
        Self { range, index: 0 }
    }
}

impl<'a, 'b: 'a> core::iter::FusedIterator for ValueRangeIter<'a, 'b> {}
impl<'a, 'b: 'a> ExactSizeIterator for ValueRangeIter<'a, 'b> {
    fn len(&self) -> usize {
        let len = self.range.len();
        len.saturating_sub(self.index)
    }
}
impl<'a, 'b: 'a> Iterator for ValueRangeIter<'a, 'b> {
    type Item = ValueRef;

    fn next(&mut self) -> Option<Self::Item> {
        let len = self.range.len();
        if self.index >= len {
            return None;
        }

        let index = self.index;
        self.index += 1;
        match &self.range {
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

impl<'a, 'b: 'a> DoubleEndedIterator for ValueRangeIter<'a, 'b> {
    fn next_back(&mut self) -> Option<Self::Item> {
        let len = self.range.len();
        let index = len.checked_sub(self.index + 1)?;

        self.index += 1;
        match &self.range {
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
