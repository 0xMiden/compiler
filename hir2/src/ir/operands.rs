use core::{fmt, num::NonZeroU16};

use smallvec::{smallvec, SmallVec};

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

    pub fn unlink(&mut self) {
        let ptr = unsafe { OpOperand::from_raw(self as *mut Self) };
        let mut value = self.value.borrow_mut();
        let uses = value.uses_mut();
        unsafe {
            let mut cursor = uses.cursor_mut_from_ptr(ptr);
            cursor.remove();
        }
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

#[derive(Default, Copy, Clone)]
struct OpOperandGroup(Option<NonZeroU16>);
impl OpOperandGroup {
    const START_MASK: u16 = u8::MAX as u16;

    fn new(start: usize, len: usize) -> Self {
        if len == 0 {
            return Self::default();
        }

        let start = u16::try_from(start).expect("too many operands");
        let len = u16::try_from(len).expect("operand group too large");
        let group = start | (len << 8);

        Self(Some(unsafe { NonZeroU16::new_unchecked(group) }))
    }

    #[allow(unused)]
    #[inline]
    pub fn start(&self) -> Option<usize> {
        Some((self.0?.get() & Self::START_MASK) as usize)
    }

    #[inline]
    pub fn end(&self) -> Option<usize> {
        self.as_range().map(|range| range.end)
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.0.is_none()
    }

    #[allow(unused)]
    #[inline]
    pub fn len(&self) -> usize {
        self.0.as_ref().map(|group| (group.get() >> 8) as usize).unwrap_or(0)
    }

    pub fn as_range(&self) -> Option<core::ops::Range<usize>> {
        let raw = self.0?.get();
        let start = (raw & Self::START_MASK) as usize;
        let len = (raw >> 8) as usize;
        Some(start..(start + len))
    }

    pub fn increase_size(&mut self, size: usize) {
        let group = self.0.as_mut().expect("expected non-empty group");
        let raw = group.get();
        let size = u16::try_from(size).expect("too many operands");
        let start = raw & Self::START_MASK;
        let len = (raw >> 8) + size;
        assert!(len <= u8::MAX as u16, "operand group is too large");
        *group = unsafe { NonZeroU16::new_unchecked(start | (len << 8)) };
    }

    pub fn decrease_size(&mut self, size: usize) {
        let group = self.0.as_mut().expect("expected non-empty group");
        let raw = group.get();
        let size = u16::try_from(size).expect("too many operands");
        let len = (raw >> 8) - size;
        if len > 0 {
            let start = raw & Self::START_MASK;
            *group = unsafe { NonZeroU16::new_unchecked(start | (len << 8)) };
        } else {
            self.0 = None;
        }
    }

    pub fn shift_start(&mut self, offset: isize) {
        let offset = i16::try_from(offset).expect("offset too large");
        if let Some(group) = self.0.as_mut() {
            let raw = group.get();
            let mut start = raw & Self::START_MASK;
            if offset >= 0 {
                start += offset as u16;
            } else {
                start -= offset.unsigned_abs();
            }
            assert!(start <= Self::START_MASK, "too many operands");
            // Clear previous start value
            let raw = raw & !Self::START_MASK;
            *group = unsafe { NonZeroU16::new_unchecked(raw | start) };
        }
    }
}

pub struct OpOperandStorage {
    operands: SmallVec<[OpOperand; 1]>,
    groups: SmallVec<[OpOperandGroup; 2]>,
}
impl fmt::Debug for OpOperandStorage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OpOperandStorage")
            .field_with("groups", |f| {
                let mut builder = f.debug_list();
                for group in self.groups.iter() {
                    match group.as_range() {
                        Some(range) => {
                            let operands = &self.operands[range.clone()];
                            builder.entry_with(|f| {
                                f.debug_map()
                                    .entry(&"range", &range)
                                    .entry(&"operands", &operands)
                                    .finish()
                            });
                        }
                        None => {
                            builder.entry(&"<empty>");
                        }
                    }
                }
                builder.finish()
            })
            .finish()
    }
}
impl Default for OpOperandStorage {
    fn default() -> Self {
        Self {
            operands: Default::default(),
            groups: smallvec![OpOperandGroup::default()],
        }
    }
}
impl OpOperandStorage {
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.operands.is_empty()
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.operands.len()
    }

    #[inline]
    pub fn iter(&self) -> core::slice::Iter<'_, OpOperand> {
        self.operands.iter()
    }

    #[inline]
    pub fn iter_mut(&mut self) -> core::slice::IterMut<'_, OpOperand> {
        self.operands.iter_mut()
    }

    /// Push operand to the last operand group
    pub fn push_operand(&mut self, mut operand: OpOperand) {
        let index = self.operands.len() as u8;
        operand.borrow_mut().index = index;
        self.operands.push(operand);
        let group = self.groups.last_mut().unwrap();
        if group.is_empty() {
            *group = OpOperandGroup::new(self.operands.len(), 1);
            return;
        }
        group.increase_size(1);
    }

    /// Push operand to the specified group
    pub fn push_operand_to_group(&mut self, group: usize, operand: OpOperand) {
        if self.groups.len() <= group {
            self.groups.resize(group + 1, OpOperandGroup::default());
        }
        let mut group = self.group_mut(group);
        group.push(operand);
    }

    /// Create operand group with index `group`, allocating any intervening groups if missing
    pub fn push_operands_to_group<I>(&mut self, group: usize, operands: I)
    where
        I: IntoIterator<Item = OpOperand>,
    {
        if self.groups.len() <= group {
            self.groups.resize(group + 1, OpOperandGroup::default());
        }
        let mut group = self.group_mut(group);
        group.extend(operands);
    }

    /// Push multiple operands to the last operand group
    pub fn extend<I>(&mut self, operands: I)
    where
        I: IntoIterator<Item = OpOperand>,
    {
        let mut group = self.group_mut(self.groups.len() - 1);
        group.extend(operands);
    }

    pub fn clear(&mut self) {
        for mut operand in self.operands.drain(..) {
            let mut operand = operand.borrow_mut();
            operand.unlink();
        }
        self.groups.clear();
        self.groups.push(OpOperandGroup::default());
    }

    /// Get all the operands
    pub fn all(&self) -> OpOperandRange<'_> {
        OpOperandRange {
            range: 0..self.operands.len(),
            operands: self.operands.as_slice(),
        }
    }

    /// Get operands for the specified group
    pub fn group(&self, group: usize) -> OpOperandRange<'_> {
        OpOperandRange {
            range: self.groups[group].as_range().unwrap_or(0..0),
            operands: self.operands.as_slice(),
        }
    }

    /// Get operands for the specified group
    pub fn group_mut(&mut self, group: usize) -> OpOperandRangeMut<'_> {
        let range = self.groups[group].as_range();
        OpOperandRangeMut {
            group,
            range,
            groups: &mut self.groups,
            operands: &mut self.operands,
        }
    }
}
impl core::ops::Index<usize> for OpOperandStorage {
    type Output = OpOperand;

    #[inline]
    fn index(&self, index: usize) -> &Self::Output {
        &self.operands[index]
    }
}
impl core::ops::IndexMut<usize> for OpOperandStorage {
    #[inline]
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        &mut self.operands[index]
    }
}

/// A reference to a range of operands in [OpOperandStorage]
pub struct OpOperandRange<'a> {
    range: core::ops::Range<usize>,
    operands: &'a [OpOperand],
}
impl<'a> OpOperandRange<'a> {
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.as_slice().is_empty()
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.as_slice().len()
    }

    #[inline]
    pub fn as_slice(&self) -> &[OpOperand] {
        &self.operands[self.range.start..self.range.end]
    }

    #[inline]
    pub fn iter(&self) -> core::slice::Iter<'_, OpOperand> {
        self.as_slice().iter()
    }

    #[inline]
    pub fn get(&self, index: usize) -> Option<&OpOperand> {
        self.as_slice().get(index)
    }
}
impl<'a> core::ops::Index<usize> for OpOperandRange<'a> {
    type Output = OpOperand;

    #[inline]
    fn index(&self, index: usize) -> &Self::Output {
        &self.as_slice()[index]
    }
}

/// A mutable range of operands in [OpOperandStorage]
///
/// Operands outside the range are not modified, however the range itself can have its size change,
/// which as a result will shift other operands around. Any other groups in [OpOperandStorage] will
/// be updated to reflect such changes, so in general this should be transparent.
pub struct OpOperandRangeMut<'a> {
    group: usize,
    range: Option<core::ops::Range<usize>>,
    groups: &'a mut [OpOperandGroup],
    operands: &'a mut SmallVec<[OpOperand; 1]>,
}
impl<'a> OpOperandRangeMut<'a> {
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.as_slice().is_empty()
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.as_slice().len()
    }

    #[inline]
    pub fn push(&mut self, operand: OpOperand) {
        self.extend([operand]);
    }

    pub fn extend<I>(&mut self, operands: I)
    where
        I: IntoIterator<Item = OpOperand>,
    {
        // Handle edge case where group is the last group
        let is_last = self.groups.len() == self.group + 1;
        let is_empty = self.range.is_none();

        if is_last && is_empty {
            let prev_len = self.operands.len();
            self.operands.extend(operands.into_iter().enumerate().map(|(i, mut operand)| {
                let mut operand_mut = operand.borrow_mut();
                operand_mut.index = (prev_len + i) as u8;
                drop(operand_mut);
                operand
            }));
            let num_inserted = self.operands.len().abs_diff(prev_len);
            if num_inserted == 0 {
                return;
            }
            self.groups[self.group] = OpOperandGroup::new(self.operands.len(), num_inserted);
            self.range = self.groups[self.group].as_range();
        } else if is_last {
            self.extend_last(operands);
        } else {
            self.extend_within(operands);
        }
    }

    fn extend_last<I>(&mut self, operands: I)
    where
        I: IntoIterator<Item = OpOperand>,
    {
        let prev_len = self.operands.len();
        self.operands.extend(operands.into_iter().enumerate().map(|(i, mut operand)| {
            let mut operand_mut = operand.borrow_mut();
            operand_mut.index = (prev_len + i) as u8;
            drop(operand_mut);
            operand
        }));
        let num_inserted = self.operands.len().abs_diff(prev_len);
        if num_inserted == 0 {
            return;
        }
        self.groups[self.group].increase_size(num_inserted);
        self.range = self.groups[self.group].as_range();
    }

    fn extend_within<I>(&mut self, operands: I)
    where
        I: IntoIterator<Item = OpOperand>,
    {
        let prev_len = self.operands.len();
        let num_inserted;

        match self.range.as_mut() {
            Some(range) => {
                let start = range.end;
                self.operands.insert_many(
                    range.end,
                    operands.into_iter().enumerate().map(|(i, mut operand)| {
                        let mut operand_mut = operand.borrow_mut();
                        operand_mut.index = (start + i) as u8;
                        drop(operand_mut);
                        operand
                    }),
                );
                num_inserted = self.operands.len().abs_diff(prev_len);
                if num_inserted == 0 {
                    return;
                }
                self.groups[self.group].increase_size(num_inserted);
                range.end += num_inserted;
            }
            None => {
                let start = self.groups[..self.group]
                    .iter()
                    .rev()
                    .filter_map(OpOperandGroup::end)
                    .next()
                    .unwrap_or(0);
                self.operands.insert_many(
                    start,
                    operands.into_iter().enumerate().map(|(i, mut operand)| {
                        let mut operand_mut = operand.borrow_mut();
                        operand_mut.index = (start + i) as u8;
                        drop(operand_mut);
                        operand
                    }),
                );
                num_inserted = self.operands.len().abs_diff(prev_len);
                if num_inserted == 0 {
                    return;
                }
                self.groups[self.group] = OpOperandGroup::new(start, num_inserted);
                self.range = self.groups[self.group].as_range();
            }
        }

        // Shift groups
        for group in self.groups[(self.group + 1)..].iter_mut() {
            if group.is_empty() {
                continue;
            }
            group.shift_start(num_inserted as isize);
        }

        // Shift operand indices
        let shifted = self.range.as_ref().unwrap().end;
        for operand in self.operands[shifted..].iter_mut() {
            let mut operand_mut = operand.borrow_mut();
            operand_mut.index += 1;
        }
    }

    pub fn pop(&mut self) -> Option<OpOperand> {
        let range = self.range.as_mut()?;
        let index = range.end;
        range.end -= 1;
        if (*range).is_empty() {
            self.range = None;
        }
        self.groups[self.group].decrease_size(1);
        let mut removed = self.operands.remove(index);
        {
            let mut operand_mut = removed.borrow_mut();
            operand_mut.unlink();
        }

        // Shift groups
        for group in self.groups[(self.group + 1)..].iter_mut() {
            if group.is_empty() {
                continue;
            }
            group.shift_start(-1);
        }

        // Shift operand indices
        for operand in self.operands[index..].iter_mut() {
            let mut operand_mut = operand.borrow_mut();
            operand_mut.index -= 1;
        }

        Some(removed)
    }

    #[inline]
    pub fn as_slice(&self) -> &[OpOperand] {
        &self.operands[self.range.clone().unwrap_or(0..0)]
    }

    #[inline]
    pub fn as_slice_mut(&mut self) -> &mut [OpOperand] {
        &mut self.operands[self.range.clone().unwrap_or(0..0)]
    }

    #[inline]
    pub fn iter(&self) -> core::slice::Iter<'_, OpOperand> {
        self.as_slice().iter()
    }

    #[inline]
    pub fn iter_mut(&mut self) -> core::slice::IterMut<'_, OpOperand> {
        self.as_slice_mut().iter_mut()
    }

    #[inline]
    pub fn get(&self, index: usize) -> Option<&OpOperand> {
        self.as_slice().get(index)
    }

    #[inline]
    pub fn get_mut(&mut self, index: usize) -> Option<&mut OpOperand> {
        self.as_slice_mut().get_mut(index)
    }
}
impl<'a> core::ops::Index<usize> for OpOperandRangeMut<'a> {
    type Output = OpOperand;

    #[inline]
    fn index(&self, index: usize) -> &Self::Output {
        &self.as_slice()[index]
    }
}
impl<'a> core::ops::IndexMut<usize> for OpOperandRangeMut<'a> {
    #[inline]
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        &mut self.as_slice_mut()[index]
    }
}
