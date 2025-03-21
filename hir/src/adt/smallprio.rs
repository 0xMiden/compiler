use core::{cmp::Ordering, fmt};

use smallvec::SmallVec;

use super::SmallDeque;

/// [SmallPriorityQueue] is a priority queue structure that can store a specified number
/// of elements inline (i.e. on the stack) without allocating memory from the heap.
///
/// Elements in the queue are stored "largest priority first", as determined by the [Ord]
/// implementation of the element type. If you instead wish to have a "lowest priority first"
/// queue, you can use [core::cmp::Reverse] to invert the natural order of the type.
///
/// It is an exercise for the reader to figure out how to wrap a type with a custom comparator
/// function. Since that isn't particularly needed yet, no built-in support for that is provided.
pub struct SmallPriorityQueue<T, const N: usize = 8> {
    pq: SmallDeque<T, N>,
}
impl<T: fmt::Debug, const N: usize> fmt::Debug for SmallPriorityQueue<T, N> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(self.iter()).finish()
    }
}
impl<T, const N: usize> Default for SmallPriorityQueue<T, N> {
    fn default() -> Self {
        Self {
            pq: Default::default(),
        }
    }
}
impl<T: Clone, const N: usize> Clone for SmallPriorityQueue<T, N> {
    fn clone(&self) -> Self {
        Self {
            pq: self.pq.clone(),
        }
    }

    fn clone_from(&mut self, source: &Self) {
        self.pq.clone_from(&source.pq);
    }
}

impl<T, const N: usize> SmallPriorityQueue<T, N> {
    /// Returns true if this map is empty
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.pq.is_empty()
    }

    /// Returns the number of key/value pairs in this map
    #[inline]
    pub fn len(&self) -> usize {
        self.pq.len()
    }

    /// Pop the highest priority item from the queue
    #[inline]
    pub fn pop(&mut self) -> Option<T> {
        self.pq.pop_front()
    }

    /// Pop the lowest priority item from the queue
    #[inline]
    pub fn pop_last(&mut self) -> Option<T> {
        self.pq.pop_back()
    }

    /// Get a reference to the next highest priority item in the queue
    #[inline]
    pub fn front(&self) -> Option<&T> {
        self.pq.front()
    }

    /// Get a reference to the lowest highest priority item in the queue
    #[inline]
    pub fn back(&self) -> Option<&T> {
        self.pq.back()
    }

    /// Get a front-to-back iterator over the items in the queue
    #[inline]
    pub fn iter(&self) -> super::smalldeque::Iter<'_, T> {
        self.pq.iter()
    }
}

impl<T, const N: usize> SmallPriorityQueue<T, N>
where
    T: Ord,
{
    /// Returns a new, empty [SmallPriorityQueue]
    pub const fn new() -> Self {
        Self {
            pq: SmallDeque::new(),
        }
    }

    /// Push an item on the queue.
    ///
    /// If the item's priority is equal to, or greater than any other item in the queue, the newly
    /// pushed item will be placed at the front of the queue. Otherwise, the item is placed in the
    /// queue at the next slot where it's priority is at least the same as the next value in the
    /// queue at that slot.
    pub fn push(&mut self, item: T) {
        if let Some(head) = self.pq.front() {
            match head.cmp(&item) {
                Ordering::Greater => self.push_slow(item),
                Ordering::Equal | Ordering::Less => {
                    // Push to the front for efficiency
                    self.pq.push_front(item);
                }
            }
        } else {
            self.pq.push_back(item);
        }
    }

    /// Push an item on the queue, by conducting a search for the most appropriate index at which
    /// to insert the new item, based upon a comparator function that compares the priorities of
    /// the items.
    fn push_slow(&mut self, item: T) {
        match self.pq.binary_search_by(|probe| probe.cmp(&item)) {
            Ok(index) => {
                self.pq.insert(index, item);
            }
            Err(index) => {
                self.pq.insert(index, item);
            }
        }
    }
}

impl<T, const N: usize> IntoIterator for SmallPriorityQueue<T, N> {
    type IntoIter = super::smalldeque::IntoIter<T, N>;
    type Item = T;

    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.pq.into_iter()
    }
}

impl<T, const N: usize> FromIterator<T> for SmallPriorityQueue<T, N>
where
    T: Ord,
{
    #[inline]
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self {
        let mut pq = SmallDeque::from_iter(iter);

        let items = pq.make_contiguous();

        items.sort();

        Self { pq }
    }
}

impl<T: Ord, const N: usize> From<SmallVec<[T; N]>> for SmallPriorityQueue<T, N> {
    fn from(mut value: SmallVec<[T; N]>) -> Self {
        value.sort();

        Self {
            pq: SmallDeque::from(value),
        }
    }
}
