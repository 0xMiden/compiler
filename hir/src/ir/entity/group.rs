use core::fmt;

/// Represents size and range information for a contiguous grouping of entities in a vector.
///
/// This is used so that individual groups can be grown or shrunk, while maintaining stability
/// of references to items in other groups.
#[derive(Default, Copy, Clone)]
pub struct EntityGroup(u32);
impl fmt::Debug for EntityGroup {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EntityGroup")
            .field("range", &self.as_range())
            .field("len", &self.len())
            .finish()
    }
}
impl EntityGroup {
    /// The start index occupies the low 16 bits, the length the high 16 bits.
    const START_MASK: u32 = u16::MAX as u32;
    const LEN_SHIFT: u32 = 16;

    /// Create a new group of size `len`, starting at index `start`
    ///
    /// Panics if `start` or `len` overflows `u16::MAX`
    pub fn new(start: usize, len: usize) -> Self {
        let start = u16::try_from(start).expect("too many items") as u32;
        let len = u16::try_from(len).expect("group too large") as u32;
        let group = start | (len << Self::LEN_SHIFT);

        Self(group)
    }

    /// Get the start index in the containing vector
    #[inline]
    pub fn start(&self) -> usize {
        (self.0 & Self::START_MASK) as usize
    }

    /// Get the end index (exclusive) in the containing vector
    #[inline]
    pub fn end(&self) -> usize {
        self.start() + self.len()
    }

    /// Returns true if this group is empty
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Get the number of items in this group
    #[inline]
    pub fn len(&self) -> usize {
        (self.0 >> Self::LEN_SHIFT) as usize
    }

    /// Get the [core::ops::Range] equivalent of this group
    pub fn as_range(&self) -> core::ops::Range<usize> {
        let start = self.start();
        let len = self.len();
        start..(start + len)
    }

    /// Increase the size of this group by `n` items
    ///
    /// Panics if the resulting size overflows `u16::MAX`
    pub fn grow(&mut self, n: usize) {
        let len = u16::try_from(self.len() + n).expect("group is too large") as u32;
        self.0 = (self.0 & Self::START_MASK) | (len << Self::LEN_SHIFT);
    }

    /// Decrease the size of this group by `n` items, to no less than zero
    pub fn shrink(&mut self, n: usize) {
        let len = self.len().saturating_sub(n) as u32;
        self.0 = (self.0 & Self::START_MASK) | (len << Self::LEN_SHIFT);
    }

    /// Shift the position of this group by `offset`
    ///
    /// Panics if the resulting start index is negative or overflows `u16::MAX`
    pub fn shift_start(&mut self, offset: isize) {
        let start = self.start().checked_add_signed(offset).expect("group offset is negative");
        let start = u16::try_from(start).expect("group offset is too large") as u32;
        self.0 = (self.0 & !Self::START_MASK) | start;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entity_group_empty() {
        let group = EntityGroup::new(0, 0);
        assert_eq!(group.start(), 0);
        assert_eq!(group.end(), 0);
        assert_eq!(group.len(), 0);
        assert!(group.is_empty());
        assert_eq!(group.as_range(), 0..0);

        let group = EntityGroup::new(101, 0);
        assert_eq!(group.start(), 101);
        assert_eq!(group.end(), 101);
        assert_eq!(group.len(), 0);
        assert!(group.is_empty());
        assert_eq!(group.as_range(), 101..101);
    }

    #[test]
    fn entity_group_non_empty() {
        let group = EntityGroup::new(0, 1);
        assert_eq!(group.start(), 0);
        assert_eq!(group.end(), 1);
        assert_eq!(group.len(), 1);
        assert!(!group.is_empty());
        assert_eq!(group.as_range(), 0..1);

        let group = EntityGroup::new(255, 255);
        assert_eq!(group.start(), 255);
        assert_eq!(group.end(), 510);
        assert_eq!(group.len(), 255);
        assert!(!group.is_empty());
        assert_eq!(group.as_range(), 255..510);
    }

    #[test]
    fn entity_group_grow() {
        let mut group = EntityGroup::new(10, 0);
        assert_eq!(group.start(), 10);
        assert_eq!(group.end(), 10);
        assert_eq!(group.len(), 0);
        assert!(group.is_empty());
        assert_eq!(group.as_range(), 10..10);

        group.grow(1);

        assert_eq!(group.start(), 10);
        assert_eq!(group.end(), 11);
        assert_eq!(group.len(), 1);
        assert!(!group.is_empty());
        assert_eq!(group.as_range(), 10..11);

        group.grow(3);

        assert_eq!(group.start(), 10);
        assert_eq!(group.end(), 14);
        assert_eq!(group.len(), 4);
        assert!(!group.is_empty());
        assert_eq!(group.as_range(), 10..14);
    }

    #[test]
    fn entity_group_shrink() {
        let mut group = EntityGroup::new(10, 4);
        assert_eq!(group.start(), 10);
        assert_eq!(group.end(), 14);
        assert_eq!(group.len(), 4);
        assert!(!group.is_empty());
        assert_eq!(group.as_range(), 10..14);

        group.shrink(3);

        assert_eq!(group.start(), 10);
        assert_eq!(group.end(), 11);
        assert_eq!(group.len(), 1);
        assert!(!group.is_empty());
        assert_eq!(group.as_range(), 10..11);

        group.shrink(1);

        assert_eq!(group.start(), 10);
        assert_eq!(group.end(), 10);
        assert_eq!(group.len(), 0);
        assert!(group.is_empty());
        assert_eq!(group.as_range(), 10..10);

        group.shrink(1);

        assert_eq!(group.start(), 10);
        assert_eq!(group.end(), 10);
        assert_eq!(group.len(), 0);
        assert!(group.is_empty());
        assert_eq!(group.as_range(), 10..10);
    }

    #[test]
    fn entity_group_shift_start() {
        let mut group = EntityGroup::new(10, 0);
        assert_eq!(group.start(), 10);
        assert_eq!(group.end(), 10);
        assert_eq!(group.len(), 0);
        assert!(group.is_empty());
        assert_eq!(group.as_range(), 10..10);

        group.shift_start(10);
        assert_eq!(group.start(), 20);
        assert_eq!(group.end(), 20);
        assert_eq!(group.len(), 0);
        assert!(group.is_empty());
        assert_eq!(group.as_range(), 20..20);

        group.shift_start(-5);
        assert_eq!(group.start(), 15);
        assert_eq!(group.end(), 15);
        assert_eq!(group.len(), 0);
        assert!(group.is_empty());
        assert_eq!(group.as_range(), 15..15);
    }
}
