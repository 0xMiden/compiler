use core::fmt;

use smallvec::{smallvec, SmallVec};

use super::{EntityGroup, StorableEntity};

/// [EntityStorage] provides an abstraction over storing IR entities in an [crate::Operation].
///
/// Specifically, it provides an abstraction for storing IR entities in a flat vector, while
/// retaining the ability to semantically group the entities, access them by group or individually,
/// and grow or shrink the group or overall set.
///
/// The implementation expects the types stored in it to implement the [StorableEntity] trait, which
/// provides it the ability to ensure the entity is kept up to date with its position in the
/// set. Additionally, it ensures that removing an entity will unlink that entity from any
/// dependents or dependencies that it needs to maintain links for.
///
/// Users can control the number of entities stored inline via the `INLINE` const parameter. By
/// default, only a single entity is stored inline, but sometimes more may be desired if you know
/// that a particular entity always has a particular cardinality.
pub struct EntityStorage<T, const INLINE: usize = 1> {
    /// The items being stored
    items: SmallVec<[T; INLINE]>,
    /// The semantic grouping information for this instance.
    ///
    /// There is always at least one group, and more can be explicitly added/removed.
    groups: SmallVec<[EntityGroup; 2]>,
}

impl<T: fmt::Debug, const INLINE: usize> fmt::Debug for EntityStorage<T, INLINE> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct(core::any::type_name::<Self>())
            .field_with("groups", |f| {
                let mut builder = f.debug_list();
                for group in self.groups.iter() {
                    let range = group.as_range();
                    let items = &self.items[range.clone()];
                    builder.entry_with(|f| {
                        f.debug_map().entry(&"range", &range).entry(&"items", &items).finish()
                    });
                }
                builder.finish()
            })
            .finish()
    }
}

impl<T, const INLINE: usize> Default for EntityStorage<T, INLINE> {
    fn default() -> Self {
        Self {
            items: Default::default(),
            groups: smallvec![EntityGroup::default()],
        }
    }
}

impl<T, const INLINE: usize> EntityStorage<T, INLINE> {
    /// Returns true if there are no items in storage.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Returns the total number of items in storage.
    #[inline]
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Returns the number of groups with allocated storage.
    #[inline]
    pub fn num_groups(&self) -> usize {
        self.groups.len()
    }

    /// Get an iterator over all of the items in storage
    #[inline]
    pub fn iter(&self) -> core::slice::Iter<'_, T> {
        self.items.iter()
    }

    /// Get a mutable iterator over all of the items in storage
    #[inline]
    pub fn iter_mut(&mut self) -> core::slice::IterMut<'_, T> {
        self.items.iter_mut()
    }
}

impl<T: StorableEntity, const INLINE: usize> EntityStorage<T, INLINE> {
    /// Push an item to the last group
    pub fn push(&mut self, mut item: T) {
        let index = self.items.len();
        unsafe {
            item.set_index(index);
        }
        self.items.push(item);
        let group = self.groups.last_mut().unwrap();
        group.grow(1);
    }

    /// Extend the last group with `items`
    pub fn extend<I>(&mut self, items: I)
    where
        I: IntoIterator<Item = T>,
    {
        let mut group = self.group_mut(self.groups.len() - 1);
        group.extend(items);
    }

    /// Push `items` as a new group, and return the group index
    #[inline]
    pub fn push_group(&mut self, items: impl IntoIterator<Item = T>) -> usize {
        let group = self.groups.len();
        self.extend_group(group, items);
        group
    }

    /// Push `item` to the specified group
    pub fn push_to_group(&mut self, group: usize, item: T) {
        if self.groups.len() <= group {
            let next_offset = self.groups.last().map(|group| group.as_range().end).unwrap_or(0);
            self.groups.resize(group + 1, EntityGroup::new(next_offset, 0));
        }
        let mut group = self.group_mut(group);
        group.push(item);
    }

    /// Pushes `items` to the given group, creating it if necessary, and allocating any intervening
    /// implied groups if they have not been created it.
    pub fn extend_group<I>(&mut self, group: usize, items: I)
    where
        I: IntoIterator<Item = T>,
    {
        if self.groups.len() <= group {
            let next_offset = self.groups.last().map(|group| group.as_range().end).unwrap_or(0);
            self.groups.resize(group + 1, EntityGroup::new(next_offset, 0));
        }
        let mut group = self.group_mut(group);
        group.extend(items);
    }

    /// Clear all items in storage
    pub fn clear(&mut self) {
        for mut item in self.items.drain(..) {
            item.unlink();
        }
        self.groups.clear();
        self.groups.push(EntityGroup::default());
    }

    /// Get all the items in storage
    pub fn all(&self) -> EntityRange<'_, T> {
        EntityRange {
            range: 0..self.items.len(),
            items: self.items.as_slice(),
        }
    }

    /// Get an [EntityRange] covering items in the specified group
    pub fn group(&self, group: usize) -> EntityRange<'_, T> {
        EntityRange {
            range: self.groups[group].as_range(),
            items: self.items.as_slice(),
        }
    }

    /// Get an [EntityRangeMut] covering items in the specified group
    pub fn group_mut(&mut self, group: usize) -> EntityRangeMut<'_, T, INLINE> {
        let range = self.groups[group].as_range();
        EntityRangeMut {
            group,
            range,
            groups: &mut self.groups,
            items: &mut self.items,
        }
    }
}
impl<T, const INLINE: usize> core::ops::Index<usize> for EntityStorage<T, INLINE> {
    type Output = T;

    #[inline]
    fn index(&self, index: usize) -> &Self::Output {
        &self.items[index]
    }
}
impl<T, const INLINE: usize> core::ops::IndexMut<usize> for EntityStorage<T, INLINE> {
    #[inline]
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        &mut self.items[index]
    }
}

/// A reference to a range of items in [EntityStorage]
pub struct EntityRange<'a, T> {
    range: core::ops::Range<usize>,
    items: &'a [T],
}
impl<'a, T> EntityRange<'a, T> {
    /// Returns true if this range is empty
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.as_slice().is_empty()
    }

    /// Returns the size of this range
    #[inline]
    pub fn len(&self) -> usize {
        self.as_slice().len()
    }

    /// Get this range as a slice
    #[inline]
    pub fn as_slice(&self) -> &[T] {
        if self.range.is_empty() {
            &[]
        } else {
            &self.items[self.range.start..self.range.end]
        }
    }

    /// Get an iterator over the items in this range
    #[inline]
    pub fn iter(&self) -> core::slice::Iter<'_, T> {
        self.as_slice().iter()
    }

    /// Get an item at the specified index relative to this range, or `None` if the index is out of bounds.
    #[inline]
    pub fn get(&self, index: usize) -> Option<&T> {
        self.as_slice().get(index)
    }
}
impl<'a, T> core::ops::Index<usize> for EntityRange<'a, T> {
    type Output = T;

    #[inline]
    fn index(&self, index: usize) -> &Self::Output {
        &self.as_slice()[index]
    }
}

/// A mutable range of items in [EntityStorage]
///
/// Items outside the range are not modified, however the range itself can have its size change,
/// which as a result will shift other items around. Any other groups in [EntityStorage] will
/// be updated to reflect such changes, so in general this should be transparent.
pub struct EntityRangeMut<'a, T, const INLINE: usize = 1> {
    group: usize,
    range: core::ops::Range<usize>,
    groups: &'a mut [EntityGroup],
    items: &'a mut SmallVec<[T; INLINE]>,
}
impl<'a, T, const INLINE: usize> EntityRangeMut<'a, T, INLINE> {
    /// Returns true if this range is empty
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.as_slice().is_empty()
    }

    /// Get the number of items covered by this range
    #[inline]
    pub fn len(&self) -> usize {
        self.as_slice().len()
    }

    /// Get this range as a slice
    #[inline]
    pub fn as_slice(&self) -> &[T] {
        if self.range.is_empty() {
            &[]
        } else {
            &self.items[self.range.start..self.range.end]
        }
    }

    /// Get this range as a mutable slice
    #[inline]
    pub fn as_slice_mut(&mut self) -> &mut [T] {
        if self.range.is_empty() {
            &mut []
        } else {
            &mut self.items[self.range.start..self.range.end]
        }
    }

    /// Get an iterator over the items covered by this range
    #[inline]
    pub fn iter(&self) -> core::slice::Iter<'_, T> {
        self.as_slice().iter()
    }

    /// Get a mutable iterator over the items covered by this range
    #[inline]
    pub fn iter_mut(&mut self) -> core::slice::IterMut<'_, T> {
        self.as_slice_mut().iter_mut()
    }

    /// Get a reference to the item at `index`, relative to the start of this range.
    #[inline]
    pub fn get(&self, index: usize) -> Option<&T> {
        self.as_slice().get(index)
    }

    /// Get a mutable reference to the item at `index`, relative to the start of this range.
    #[inline]
    pub fn get_mut(&mut self, index: usize) -> Option<&mut T> {
        self.as_slice_mut().get_mut(index)
    }
}

impl<'a, T: StorableEntity, const INLINE: usize> EntityRangeMut<'a, T, INLINE> {
    /// Append `item` to storage at the end of this range
    #[inline]
    pub fn push(&mut self, item: T) {
        self.extend([item]);
    }

    /// Append `items` to storage at the end of this range
    pub fn extend<I>(&mut self, operands: I)
    where
        I: IntoIterator<Item = T>,
    {
        // Handle edge case where group is the last group
        let is_last = self.groups.len() == self.group + 1;
        if is_last {
            self.extend_last(operands);
        } else {
            self.extend_within(operands);
        }
    }

    fn extend_last<I>(&mut self, items: I)
    where
        I: IntoIterator<Item = T>,
    {
        let prev_len = self.items.len();
        self.items.extend(items.into_iter().enumerate().map(|(i, mut item)| {
            unsafe {
                item.set_index(prev_len + i);
            }
            item
        }));
        let num_inserted = self.items.len().abs_diff(prev_len);
        if num_inserted == 0 {
            return;
        }
        self.groups[self.group].grow(num_inserted);
        self.range = self.groups[self.group].as_range();
    }

    fn extend_within<I>(&mut self, items: I)
    where
        I: IntoIterator<Item = T>,
    {
        let prev_len = self.items.len();
        let start = self.range.end;
        self.items.insert_many(
            start,
            items.into_iter().enumerate().map(|(i, mut item)| {
                unsafe {
                    item.set_index(start + i);
                }
                item
            }),
        );
        let num_inserted = self.items.len().abs_diff(prev_len);
        if num_inserted == 0 {
            return;
        }
        self.groups[self.group].grow(num_inserted);
        self.range = self.groups[self.group].as_range();

        // Shift groups
        for group in self.groups[(self.group + 1)..].iter_mut() {
            group.shift_start(num_inserted as isize);
        }

        // Shift item indices
        let shifted = self.range.end;
        for (offset, item) in self.items[shifted..].iter_mut().enumerate() {
            unsafe {
                item.set_index(shifted + offset);
            }
        }
    }

    /// Remove the last item from this group, or `None` if empty
    pub fn pop(&mut self) -> Option<T> {
        if self.range.is_empty() {
            return None;
        }
        let index = self.range.end - 1;
        self.range.end = index;
        self.groups[self.group].shrink(1);
        let mut removed = self.items.remove(index);
        {
            removed.unlink();
        }

        // Shift groups
        let next_group = self.group + 1;
        if next_group < self.groups.len() {
            for group in self.groups[next_group..].iter_mut() {
                group.shift_start(-1);
            }
        }

        // Shift item indices
        let next_item = index;
        if next_item < self.items.len() {
            for (offset, item) in self.items[next_item..].iter_mut().enumerate() {
                unsafe {
                    item.set_index(next_item + offset);
                }
            }
        }

        Some(removed)
    }
}
impl<'a, T, const INLINE: usize> core::ops::Index<usize> for EntityRangeMut<'a, T, INLINE> {
    type Output = T;

    #[inline]
    fn index(&self, index: usize) -> &Self::Output {
        &self.as_slice()[index]
    }
}
impl<'a, T, const INLINE: usize> core::ops::IndexMut<usize> for EntityRangeMut<'a, T, INLINE> {
    #[inline]
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        &mut self.as_slice_mut()[index]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Copy, Clone, PartialEq, Eq)]
    struct Item {
        index: usize,
        value: usize,
    }
    impl Item {
        pub fn new(value: usize) -> Self {
            Self { index: 0, value }
        }
    }
    impl StorableEntity for Item {
        fn index(&self) -> usize {
            self.index
        }

        unsafe fn set_index(&mut self, index: usize) {
            self.index = index;
        }

        fn unlink(&mut self) {}
    }

    type ItemStorage = EntityStorage<Item, 1>;
    #[allow(unused)]
    type ItemRange<'a> = EntityRange<'a, Item>;
    #[allow(unused)]
    type ItemRangeMut<'a> = EntityRangeMut<'a, Item, 1>;

    #[test]
    fn entity_storage_empty_operations() {
        let mut storage = ItemStorage::default();

        // No items, but always have at least one group
        assert_eq!(storage.len(), 0);
        assert!(storage.is_empty());
        assert_eq!(storage.num_groups(), 1);

        {
            let range = storage.all();
            assert_eq!(range.len(), 0);
            assert!(range.is_empty());
            assert_eq!(range.as_slice(), &[]);
            assert_eq!(range.iter().next(), None);
        }

        // No items, two groups
        let group = storage.push_group(None);
        assert_eq!(group, 1);
        assert_eq!(storage.num_groups(), 2);
        assert_eq!(storage.len(), 0);
        assert!(storage.is_empty());

        {
            let range = storage.group(0);
            assert_eq!(range.len(), 0);
            assert!(range.is_empty());
            assert_eq!(range.as_slice(), &[]);
            assert_eq!(range.iter().next(), None);
        }
    }

    #[test]
    fn entity_storage_push_to_empty_group_entity_range() {
        let mut storage = ItemStorage::default();

        // Get group as mutable range
        let mut group_range = storage.group_mut(0);

        // Verify handling of empty group in EntityRangeMut
        assert_eq!(group_range.len(), 0);
        assert!(group_range.is_empty());
        assert_eq!(group_range.as_slice(), &[]);
        assert_eq!(group_range.iter().next(), None);

        // Push items to range
        group_range.push(Item::new(0));
        group_range.push(Item::new(1));

        // Verify range reflects changes
        assert_eq!(group_range.len(), 2);
        assert!(!group_range.is_empty());
        assert_eq!(
            group_range.as_slice(),
            &[Item { index: 0, value: 0 }, Item { index: 1, value: 1 }]
        );
        assert_eq!(group_range.iter().next(), Some(&Item { index: 0, value: 0 }));
    }

    #[test]
    fn entity_storage_pop_from_non_empty_group_entity_range() {
        let mut storage = ItemStorage::default();

        assert_eq!(storage.num_groups(), 1);
        storage.push_to_group(0, Item::new(0));
        assert_eq!(storage.len(), 1);
        assert!(!storage.is_empty());

        // Get group as mutable range
        let mut group_range = storage.group_mut(0);
        assert_eq!(group_range.len(), 1);
        assert!(!group_range.is_empty());
        assert_eq!(group_range.as_slice(), &[Item { index: 0, value: 0 }]);
        assert_eq!(group_range.iter().next(), Some(&Item { index: 0, value: 0 }));

        // Pop item from range
        let item = group_range.pop();
        assert_eq!(item, Some(Item { index: 0, value: 0 }));
        assert_eq!(group_range.len(), 0);
        assert!(group_range.is_empty());
        assert_eq!(group_range.as_slice(), &[]);
        assert_eq!(group_range.iter().next(), None);
        assert_eq!(group_range.range.clone(), 0..0);

        // Pop from empty range should have no effect
        let item = group_range.pop();
        assert_eq!(item, None);
        assert_eq!(group_range.len(), 0);
        assert!(group_range.is_empty());
        assert_eq!(group_range.as_slice(), &[]);
        assert_eq!(group_range.iter().next(), None);
        assert_eq!(group_range.range.clone(), 0..0);
    }

    #[test]
    fn entity_storage_push_to_empty_group_entity_range_before_other_groups() {
        let mut storage = ItemStorage::default();

        storage.extend_group(0, [Item::new(0), Item::new(1)]);
        let group1 = storage.push_group(None);
        let group2 = storage.push_group(None);
        let group3 = storage.push_group([Item::new(4), Item::new(5)]);

        assert!(!storage.is_empty());
        assert_eq!(storage.len(), 4);
        assert_eq!(storage.num_groups(), 4);

        assert_eq!(storage.group(0).range.clone(), 0..2);
        assert_eq!(storage.group(1).range.clone(), 2..2);
        assert_eq!(storage.group(2).range.clone(), 2..2);
        assert_eq!(storage.group(3).range.clone(), 2..4);

        // Insert items into first non-empty group
        {
            let mut group_range = storage.group_mut(group1);

            // Verify handling of empty group in EntityRangeMut
            assert_eq!(group_range.len(), 0);
            assert!(group_range.is_empty());
            assert_eq!(group_range.as_slice(), &[]);
            assert_eq!(group_range.iter().next(), None);

            // Push items to range
            group_range.push(Item::new(2));
            group_range.push(Item::new(3));

            // Verify range reflects changes
            assert_eq!(group_range.len(), 2);
            assert!(!group_range.is_empty());
            assert_eq!(
                group_range.as_slice(),
                &[Item { index: 2, value: 2 }, Item { index: 3, value: 3 }]
            );
            assert_eq!(group_range.iter().next(), Some(&Item { index: 2, value: 2 }));
        }

        // The subsequent empty group should still be empty, but at a new offset
        let group_range = storage.group(group2);
        assert_eq!(group_range.range.clone(), 4..4);
        assert_eq!(group_range.len(), 0);
        assert!(group_range.is_empty());
        assert_eq!(group_range.as_slice(), &[]);
        assert_eq!(group_range.iter().next(), None);

        // The trailing non-empty group should have updated offsets
        let group_range = storage.group(group3);
        assert_eq!(group_range.range.clone(), 4..6);
        assert_eq!(group_range.len(), 2);
        assert!(!group_range.is_empty());
        assert_eq!(
            group_range.as_slice(),
            &[Item { index: 4, value: 4 }, Item { index: 5, value: 5 }]
        );
        assert_eq!(group_range.iter().next(), Some(&Item { index: 4, value: 4 }));
    }

    #[test]
    fn entity_storage_pop_from_non_empty_group_entity_range_before_other_groups() {
        let mut storage = ItemStorage::default();

        storage.extend_group(0, [Item::new(0), Item::new(1)]);
        let group1 = storage.push_group(None);
        let group2 = storage.push_group(None);
        let group3 = storage.push_group([Item::new(4), Item::new(5)]);

        assert!(!storage.is_empty());
        assert_eq!(storage.len(), 4);
        assert_eq!(storage.num_groups(), 4);

        assert_eq!(storage.group(0).range.clone(), 0..2);
        assert_eq!(storage.group(1).range.clone(), 2..2);
        assert_eq!(storage.group(2).range.clone(), 2..2);
        assert_eq!(storage.group(3).range.clone(), 2..4);

        // Pop from group0
        {
            let mut group_range = storage.group_mut(0);
            let item = group_range.pop();
            assert_eq!(item, Some(Item { index: 1, value: 1 }));
            assert_eq!(group_range.len(), 1);
            assert!(!group_range.is_empty());
            assert_eq!(group_range.as_slice(), &[Item { index: 0, value: 0 }]);
        }

        // The subsequent empty group(s) should still be empty, but at a new offset
        for group_index in [group1, group2] {
            let group_range = storage.group(group_index);
            assert_eq!(group_range.range.clone(), 1..1);
            assert_eq!(group_range.len(), 0);
            assert!(group_range.is_empty());
            assert_eq!(group_range.as_slice(), &[]);
            assert_eq!(group_range.iter().next(), None);
        }

        // The trailing non-empty group should have updated offsets
        let group_range = storage.group(group3);
        assert_eq!(group_range.range.clone(), 1..3);
        assert_eq!(group_range.len(), 2);
        assert!(!group_range.is_empty());
        assert_eq!(
            group_range.as_slice(),
            &[Item { index: 1, value: 4 }, Item { index: 2, value: 5 }]
        );
        assert_eq!(group_range.iter().next(), Some(&Item { index: 1, value: 4 }));
    }
}
