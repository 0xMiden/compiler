use core::{
    cmp::Ordering,
    fmt,
    iter::{repeat_n, repeat_with, ByRefSized},
    ops::{Index, IndexMut, Range, RangeBounds},
    ptr::{self, NonNull},
};

use smallvec::SmallVec;

use super::SizedTypeProperties;

/// [SmallDeque] is a [alloc::collections::VecDeque]-like structure that can store a specified
/// number of elements inline (i.e. on the stack) without allocating memory from the heap.
///
/// This data structure is designed to basically provide the functionality of `VecDeque` without
/// needing to allocate on the heap for small numbers of nodes.
///
/// Internally, [SmallDeque] is implemented on top of [SmallVec].
///
/// Most of the implementation is ripped from the standard library `VecDeque` impl, but adapted
/// for `SmallVec`
pub struct SmallDeque<T, const N: usize = 8> {
    /// `self[0]`, if it exists, is `buf[head]`.
    /// `head < buf.capacity()`, unless `buf.capacity() == 0` when `head == 0`.
    head: usize,
    /// The number of initialized elements, starting from the one at `head` and potentially
    /// wrapping around.
    ///
    /// If `len == 0`, the exact value of `head` is unimportant.
    ///
    /// If `T` is zero-sized, then `self.len <= usize::MAX`, otherwise
    /// `self.len <= isize::MAX as usize`
    len: usize,
    buf: SmallVec<[T; N]>,
}
impl<T: Clone, const N: usize> Clone for SmallDeque<T, N> {
    fn clone(&self) -> Self {
        let mut deq = Self::with_capacity(self.len());
        deq.extend(self.iter().cloned());
        deq
    }

    fn clone_from(&mut self, source: &Self) {
        self.clear();
        self.extend(source.iter().cloned());
    }
}
impl<T, const N: usize> Default for SmallDeque<T, N> {
    fn default() -> Self {
        Self {
            head: 0,
            len: 0,
            buf: Default::default(),
        }
    }
}
impl<T, const N: usize> SmallDeque<T, N> {
    /// Returns a new, empty [SmallDeque]
    #[inline]
    #[must_use]
    pub const fn new() -> Self {
        Self {
            head: 0,
            len: 0,
            buf: SmallVec::new_const(),
        }
    }

    /// Create an empty deque with pre-allocated space for `capacity` elements.
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            head: 0,
            len: 0,
            buf: SmallVec::with_capacity(capacity),
        }
    }

    /// Returns true if this map is empty
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Returns the number of key/value pairs in this map
    pub fn len(&self) -> usize {
        self.len
    }

    /// Return a front-to-back iterator.
    pub fn iter(&self) -> Iter<'_, T> {
        let (a, b) = self.as_slices();
        Iter::new(a.iter(), b.iter())
    }

    /// Return a front-to-back iterator that returns mutable references
    pub fn iter_mut(&mut self) -> IterMut<'_, T> {
        let (a, b) = self.as_mut_slices();
        IterMut::new(a.iter_mut(), b.iter_mut())
    }

    /// Returns a pair of slices which contain, in order, the contents of the
    /// deque.
    ///
    /// If [`SmallDeque::make_contiguous`] was previously called, all elements of the
    /// deque will be in the first slice and the second slice will be empty.
    #[inline]
    pub fn as_slices(&self) -> (&[T], &[T]) {
        let (a_range, b_range) = self.slice_ranges(.., self.len);
        // SAFETY: `slice_ranges` always returns valid ranges into the physical buffer.
        unsafe { (&*self.buffer_range(a_range), &*self.buffer_range(b_range)) }
    }

    /// Returns a pair of slices which contain, in order, the contents of the
    /// deque.
    ///
    /// If [`SmallDeque::make_contiguous`] was previously called, all elements of the
    /// deque will be in the first slice and the second slice will be empty.
    #[inline]
    pub fn as_mut_slices(&mut self) -> (&mut [T], &mut [T]) {
        let (a_range, b_range) = self.slice_ranges(.., self.len);
        // SAFETY: `slice_ranges` always returns valid ranges into the physical buffer.
        unsafe { (&mut *self.buffer_range_mut(a_range), &mut *self.buffer_range_mut(b_range)) }
    }

    /// Given a range into the logical buffer of the deque, this function
    /// return two ranges into the physical buffer that correspond to
    /// the given range. The `len` parameter should usually just be `self.len`;
    /// the reason it's passed explicitly is that if the deque is wrapped in
    /// a `Drain`, then `self.len` is not actually the length of the deque.
    ///
    /// # Safety
    ///
    /// This function is always safe to call. For the resulting ranges to be valid
    /// ranges into the physical buffer, the caller must ensure that the result of
    /// calling `slice::range(range, ..len)` represents a valid range into the
    /// logical buffer, and that all elements in that range are initialized.
    fn slice_ranges<R>(&self, range: R, len: usize) -> (Range<usize>, Range<usize>)
    where
        R: RangeBounds<usize>,
    {
        let Range { start, end } = core::slice::range(range, ..len);
        let len = end - start;

        if len == 0 {
            (0..0, 0..0)
        } else {
            // `slice::range` guarantees that `start <= end <= len`.
            // because `len != 0`, we know that `start < end`, so `start < len`
            // and the indexing is valid.
            let wrapped_start = self.to_physical_idx(start);

            // this subtraction can never overflow because `wrapped_start` is
            // at most `self.capacity()` (and if `self.capacity != 0`, then `wrapped_start` is strictly less
            // than `self.capacity`).
            let head_len = self.capacity() - wrapped_start;

            if head_len >= len {
                // we know that `len + wrapped_start <= self.capacity <= usize::MAX`, so this addition can't overflow
                (wrapped_start..wrapped_start + len, 0..0)
            } else {
                // can't overflow because of the if condition
                let tail_len = len - head_len;
                (wrapped_start..self.capacity(), 0..tail_len)
            }
        }
    }

    /// Creates an iterator that covers the specified range in the deque.
    ///
    /// # Panics
    ///
    /// Panics if the starting point is greater than the end point or if
    /// the end point is greater than the length of the deque.
    #[inline]
    pub fn range<R>(&self, range: R) -> Iter<'_, T>
    where
        R: RangeBounds<usize>,
    {
        let (a_range, b_range) = self.slice_ranges(range, self.len);
        // SAFETY: The ranges returned by `slice_ranges`
        // are valid ranges into the physical buffer, so
        // it's ok to pass them to `buffer_range` and
        // dereference the result.
        let a = unsafe { &*self.buffer_range(a_range) };
        let b = unsafe { &*self.buffer_range(b_range) };
        Iter::new(a.iter(), b.iter())
    }

    /// Creates an iterator that covers the specified mutable range in the deque.
    ///
    /// # Panics
    ///
    /// Panics if the starting point is greater than the end point or if
    /// the end point is greater than the length of the deque.
    #[inline]
    pub fn range_mut<R>(&mut self, range: R) -> IterMut<'_, T>
    where
        R: RangeBounds<usize>,
    {
        let (a_range, b_range) = self.slice_ranges(range, self.len);
        // SAFETY: The ranges returned by `slice_ranges`
        // are valid ranges into the physical buffer, so
        // it's ok to pass them to `buffer_range` and
        // dereference the result.
        let a = unsafe { &mut *self.buffer_range_mut(a_range) };
        let b = unsafe { &mut *self.buffer_range_mut(b_range) };
        IterMut::new(a.iter_mut(), b.iter_mut())
    }

    /// Get a reference to the element at the given index.
    ///
    /// Element at index 0 is the front of the queue.
    pub fn get(&self, index: usize) -> Option<&T> {
        if index < self.len {
            let index = self.to_physical_idx(index);
            unsafe { Some(&*self.ptr().add(index)) }
        } else {
            None
        }
    }

    /// Get a mutable reference to the element at the given index.
    ///
    /// Element at index 0 is the front of the queue.
    pub fn get_mut(&mut self, index: usize) -> Option<&mut T> {
        if index < self.len {
            let index = self.to_physical_idx(index);
            unsafe { Some(&mut *self.ptr_mut().add(index)) }
        } else {
            None
        }
    }

    /// Swaps elements at indices `i` and `j`
    ///
    /// `i` and `j` may be equal.
    ///
    /// Element at index 0 is the front of the queue.
    ///
    /// # Panics
    ///
    /// Panics if either index is out of bounds.
    pub fn swap(&mut self, i: usize, j: usize) {
        assert!(i < self.len());
        assert!(j < self.len());
        let ri = self.to_physical_idx(i);
        let rj = self.to_physical_idx(j);
        unsafe { ptr::swap(self.ptr_mut().add(ri), self.ptr_mut().add(rj)) }
    }

    /// Returns the number of elements the deque can hold without reallocating.
    #[inline]
    pub fn capacity(&self) -> usize {
        if T::IS_ZST {
            usize::MAX
        } else {
            self.buf.capacity()
        }
    }

    /// Reserves the minimum capacity for at least `additional` more elements to be inserted in the
    /// given deque. Does nothing if the capacity is already sufficient.
    ///
    /// Note that the allocator may give the collection more space than it requests. Therefore
    /// capacity can not be relied upon to be precisely minimal. Prefer [`reserve`] if future
    /// insertions are expected.
    ///
    /// # Panics
    ///
    /// Panics if the new capacity overflows `usize`.
    pub fn reserve_exact(&mut self, additional: usize) {
        let new_cap = self.len.checked_add(additional).expect("capacity overflow");
        let old_cap = self.capacity();

        if new_cap > old_cap {
            self.buf.try_grow(new_cap).expect("capacity overflow");
            unsafe {
                self.handle_capacity_increase(old_cap);
            }
        }
    }

    /// Reserves capacity for at least `additional` more elements to be inserted in the given
    /// deque. The collection may reserve more space to speculatively avoid frequent reallocations.
    ///
    /// # Panics
    ///
    /// Panics if the new capacity overflows `usize`.
    pub fn reserve(&mut self, additional: usize) {
        let new_cap = self.len.checked_add(additional).expect("capacity overflow");
        let old_cap = self.capacity();

        if new_cap > old_cap {
            // we don't need to reserve_exact(), as the size doesn't have
            // to be a power of 2.
            self.buf.try_grow(new_cap).expect("capacity overflow");
            unsafe {
                self.handle_capacity_increase(old_cap);
            }
        }
    }

    /// Shortens the deque, keeping the first `len` elements and dropping
    /// the rest.
    ///
    /// If `len` is greater or equal to the deque's current length, this has
    /// no effect.
    pub fn truncate(&mut self, len: usize) {
        /// Runs the destructor for all items in the slice when it gets dropped (normally or
        /// during unwinding).
        struct Dropper<'a, T>(&'a mut [T]);

        impl<T> Drop for Dropper<'_, T> {
            fn drop(&mut self) {
                unsafe {
                    ptr::drop_in_place(self.0);
                }
            }
        }

        // Safe because:
        //
        // * Any slice passed to `drop_in_place` is valid; the second case has
        //   `len <= front.len()` and returning on `len > self.len()` ensures
        //   `begin <= back.len()` in the first case
        // * The head of the SmallDeque is moved before calling `drop_in_place`,
        //   so no value is dropped twice if `drop_in_place` panics
        unsafe {
            if len >= self.len {
                return;
            }

            let (front, back) = self.as_mut_slices();
            if len > front.len() {
                let begin = len - front.len();
                let drop_back = back.get_unchecked_mut(begin..) as *mut _;
                self.len = len;
                ptr::drop_in_place(drop_back);
            } else {
                let drop_back = back as *mut _;
                let drop_front = front.get_unchecked_mut(len..) as *mut _;
                self.len = len;

                // Make sure the second half is dropped even when a destructor
                // in the first one panics.
                let _back_dropper = Dropper(&mut *drop_back);
                ptr::drop_in_place(drop_front);
            }
        }
    }

    /// Removes the specified range from the deque in bulk, returning all
    /// removed elements as an iterator. If the iterator is dropped before
    /// being fully consumed, it drops the remaining removed elements.
    ///
    /// The returned iterator keeps a mutable borrow on the queue to optimize
    /// its implementation.
    ///
    ///
    /// # Panics
    ///
    /// Panics if the starting point is greater than the end point or if
    /// the end point is greater than the length of the deque.
    ///
    /// # Leaking
    ///
    /// If the returned iterator goes out of scope without being dropped (due to
    /// [`mem::forget`], for example), the deque may have lost and leaked
    /// elements arbitrarily, including elements outside the range.
    #[inline]
    pub fn drain<R>(&mut self, range: R) -> Drain<'_, T, N>
    where
        R: RangeBounds<usize>,
    {
        // Memory safety
        //
        // When the Drain is first created, the source deque is shortened to
        // make sure no uninitialized or moved-from elements are accessible at
        // all if the Drain's destructor never gets to run.
        //
        // Drain will ptr::read out the values to remove.
        // When finished, the remaining data will be copied back to cover the hole,
        // and the head/tail values will be restored correctly.
        //
        let Range { start, end } = core::slice::range(range, ..self.len);
        let drain_start = start;
        let drain_len = end - start;

        // The deque's elements are parted into three segments:
        // * 0  -> drain_start
        // * drain_start -> drain_start+drain_len
        // * drain_start+drain_len -> self.len
        //
        // H = self.head; T = self.head+self.len; t = drain_start+drain_len; h = drain_head
        //
        // We store drain_start as self.len, and drain_len and self.len as
        // drain_len and orig_len respectively on the Drain. This also
        // truncates the effective array such that if the Drain is leaked, we
        // have forgotten about the potentially moved values after the start of
        // the drain.
        //
        //        H   h   t   T
        // [. . . o o x x o o . . .]
        //
        // "forget" about the values after the start of the drain until after
        // the drain is complete and the Drain destructor is run.

        unsafe { Drain::new(self, drain_start, drain_len) }
    }

    /// Clears the deque, removing all values.
    #[inline]
    pub fn clear(&mut self) {
        self.truncate(0);
        // Not strictly necessary, but leaves things in a more consistent/predictable state.
        self.head = 0;
    }

    /// Returns `true` if the deque contains an element equal to the
    /// given value.
    ///
    /// This operation is *O*(*n*).
    ///
    /// Note that if you have a sorted `SmallDeque`, [`binary_search`] may be faster.
    ///
    /// [`binary_search`]: SmallDeque::binary_search
    pub fn contains(&self, x: &T) -> bool
    where
        T: PartialEq<T>,
    {
        let (a, b) = self.as_slices();
        a.contains(x) || b.contains(x)
    }

    /// Provides a reference to the front element, or `None` if the deque is
    /// empty.
    pub fn front(&self) -> Option<&T> {
        self.get(0)
    }

    /// Provides a mutable reference to the front element, or `None` if the
    /// deque is empty.
    pub fn front_mut(&mut self) -> Option<&mut T> {
        self.get_mut(0)
    }

    /// Provides a reference to the back element, or `None` if the deque is
    /// empty.
    pub fn back(&self) -> Option<&T> {
        self.get(self.len.wrapping_sub(1))
    }

    /// Provides a mutable reference to the back element, or `None` if the
    /// deque is empty.
    pub fn back_mut(&mut self) -> Option<&mut T> {
        self.get_mut(self.len.wrapping_sub(1))
    }

    /// Removes the first element and returns it, or `None` if the deque is
    /// empty.
    pub fn pop_front(&mut self) -> Option<T> {
        if self.is_empty() {
            None
        } else {
            let old_head = self.head;
            self.head = self.to_physical_idx(1);
            self.len -= 1;
            unsafe {
                core::hint::assert_unchecked(self.len < self.capacity());
                Some(self.buffer_read(old_head))
            }
        }
    }

    /// Removes the last element from the deque and returns it, or `None` if
    /// it is empty.
    pub fn pop_back(&mut self) -> Option<T> {
        if self.is_empty() {
            None
        } else {
            self.len -= 1;
            unsafe {
                core::hint::assert_unchecked(self.len < self.capacity());
                Some(self.buffer_read(self.to_physical_idx(self.len)))
            }
        }
    }

    /// Prepends an element to the deque.
    pub fn push_front(&mut self, value: T) {
        if self.is_full() {
            self.grow();
        }

        self.head = self.wrap_sub(self.head, 1);
        self.len += 1;

        unsafe {
            self.buffer_write(self.head, value);
        }
    }

    /// Appends an element to the back of the deque.
    pub fn push_back(&mut self, value: T) {
        if self.is_full() {
            self.grow();
        }

        unsafe { self.buffer_write(self.to_physical_idx(self.len), value) }
        self.len += 1;
    }

    #[inline]
    fn is_contiguous(&self) -> bool {
        // Do the calculation like this to avoid overflowing if len + head > usize::MAX
        self.head <= self.capacity() - self.len
    }

    /// Removes an element from anywhere in the deque and returns it,
    /// replacing it with the first element.
    ///
    /// This does not preserve ordering, but is *O*(1).
    ///
    /// Returns `None` if `index` is out of bounds.
    ///
    /// Element at index 0 is the front of the queue.
    pub fn swap_remove_front(&mut self, index: usize) -> Option<T> {
        let length = self.len;
        if index < length && index != 0 {
            self.swap(index, 0);
        } else if index >= length {
            return None;
        }
        self.pop_front()
    }

    /// Removes an element from anywhere in the deque and returns it,
    /// replacing it with the last element.
    ///
    /// This does not preserve ordering, but is *O*(1).
    ///
    /// Returns `None` if `index` is out of bounds.
    ///
    /// Element at index 0 is the front of the queue.
    pub fn swap_remove_back(&mut self, index: usize) -> Option<T> {
        let length = self.len;
        if length > 0 && index < length - 1 {
            self.swap(index, length - 1);
        } else if index >= length {
            return None;
        }
        self.pop_back()
    }

    /// Inserts an element at `index` within the deque, shifting all elements
    /// with indices greater than or equal to `index` towards the back.
    ///
    /// Element at index 0 is the front of the queue.
    ///
    /// # Panics
    ///
    /// Panics if `index` is greater than deque's length
    pub fn insert(&mut self, index: usize, value: T) {
        assert!(index <= self.len(), "index out of bounds");
        if self.is_full() {
            self.grow();
        }

        let k = self.len - index;
        if k < index {
            // `index + 1` can't overflow, because if index was usize::MAX, then either the
            // assert would've failed, or the deque would've tried to grow past usize::MAX
            // and panicked.
            unsafe {
                // see `remove()` for explanation why this wrap_copy() call is safe.
                self.wrap_copy(self.to_physical_idx(index), self.to_physical_idx(index + 1), k);
                self.buffer_write(self.to_physical_idx(index), value);
                self.len += 1;
            }
        } else {
            let old_head = self.head;
            self.head = self.wrap_sub(self.head, 1);
            unsafe {
                self.wrap_copy(old_head, self.head, index);
                self.buffer_write(self.to_physical_idx(index), value);
                self.len += 1;
            }
        }
    }

    /// Removes and returns the element at `index` from the deque.
    /// Whichever end is closer to the removal point will be moved to make
    /// room, and all the affected elements will be moved to new positions.
    /// Returns `None` if `index` is out of bounds.
    ///
    /// Element at index 0 is the front of the queue.
    pub fn remove(&mut self, index: usize) -> Option<T> {
        if self.len <= index {
            return None;
        }

        let wrapped_idx = self.to_physical_idx(index);

        let elem = unsafe { Some(self.buffer_read(wrapped_idx)) };

        let k = self.len - index - 1;
        // safety: due to the nature of the if-condition, whichever wrap_copy gets called,
        // its length argument will be at most `self.len / 2`, so there can't be more than
        // one overlapping area.
        if k < index {
            unsafe { self.wrap_copy(self.wrap_add(wrapped_idx, 1), wrapped_idx, k) };
            self.len -= 1;
        } else {
            let old_head = self.head;
            self.head = self.to_physical_idx(1);
            unsafe { self.wrap_copy(old_head, self.head, index) };
            self.len -= 1;
        }

        elem
    }

    /// Splits the deque into two at the given index.
    ///
    /// Returns a newly allocated `SmallDeque`. `self` contains elements `[0, at)`,
    /// and the returned deque contains elements `[at, len)`.
    ///
    /// Note that the capacity of `self` does not change.
    ///
    /// Element at index 0 is the front of the queue.
    ///
    /// # Panics
    ///
    /// Panics if `at > len`.
    #[inline]
    #[must_use = "use `.truncate()` if you don't need the other half"]
    pub fn split_off(&mut self, at: usize) -> Self {
        let len = self.len;
        assert!(at <= len, "`at` out of bounds");

        let other_len = len - at;
        let mut other = Self::with_capacity(other_len);

        unsafe {
            let (first_half, second_half) = self.as_slices();

            let first_len = first_half.len();
            let second_len = second_half.len();
            if at < first_len {
                // `at` lies in the first half.
                let amount_in_first = first_len - at;

                ptr::copy_nonoverlapping(
                    first_half.as_ptr().add(at),
                    other.ptr_mut(),
                    amount_in_first,
                );

                // just take all of the second half.
                ptr::copy_nonoverlapping(
                    second_half.as_ptr(),
                    other.ptr_mut().add(amount_in_first),
                    second_len,
                );
            } else {
                // `at` lies in the second half, need to factor in the elements we skipped
                // in the first half.
                let offset = at - first_len;
                let amount_in_second = second_len - offset;
                ptr::copy_nonoverlapping(
                    second_half.as_ptr().add(offset),
                    other.ptr_mut(),
                    amount_in_second,
                );
            }
        }

        // Cleanup where the ends of the buffers are
        self.len = at;
        other.len = other_len;

        other
    }

    /// Moves all the elements of `other` into `self`, leaving `other` empty.
    ///
    /// # Panics
    ///
    /// Panics if the new number of elements in self overflows a `usize`.
    #[inline]
    pub fn append(&mut self, other: &mut Self) {
        if T::IS_ZST {
            self.len = self.len.checked_add(other.len).expect("capacity overflow");
            other.len = 0;
            other.head = 0;
            return;
        }

        self.reserve(other.len);
        unsafe {
            let (left, right) = other.as_slices();
            self.copy_slice(self.to_physical_idx(self.len), left);
            // no overflow, because self.capacity() >= old_cap + left.len() >= self.len + left.len()
            self.copy_slice(self.to_physical_idx(self.len + left.len()), right);
        }
        // SAFETY: Update pointers after copying to avoid leaving doppelganger
        // in case of panics.
        self.len += other.len;
        // Now that we own its values, forget everything in `other`.
        other.len = 0;
        other.head = 0;
    }

    /// Retains only the elements specified by the predicate.
    ///
    /// In other words, remove all elements `e` for which `f(&e)` returns false.
    /// This method operates in place, visiting each element exactly once in the
    /// original order, and preserves the order of the retained elements.
    pub fn retain<F>(&mut self, mut f: F)
    where
        F: FnMut(&T) -> bool,
    {
        self.retain_mut(|elem| f(elem));
    }

    /// Retains only the elements specified by the predicate.
    ///
    /// In other words, remove all elements `e` for which `f(&e)` returns false.
    /// This method operates in place, visiting each element exactly once in the
    /// original order, and preserves the order of the retained elements.
    pub fn retain_mut<F>(&mut self, mut f: F)
    where
        F: FnMut(&mut T) -> bool,
    {
        let len = self.len;
        let mut idx = 0;
        let mut cur = 0;

        // Stage 1: All values are retained.
        while cur < len {
            if !f(&mut self[cur]) {
                cur += 1;
                break;
            }
            cur += 1;
            idx += 1;
        }
        // Stage 2: Swap retained value into current idx.
        while cur < len {
            if !f(&mut self[cur]) {
                cur += 1;
                continue;
            }

            self.swap(idx, cur);
            cur += 1;
            idx += 1;
        }
        // Stage 3: Truncate all values after idx.
        if cur != idx {
            self.truncate(idx);
        }
    }

    // Double the buffer size. This method is inline(never), so we expect it to only
    // be called in cold paths.
    // This may panic or abort
    #[inline(never)]
    fn grow(&mut self) {
        // Extend or possibly remove this assertion when valid use-cases for growing the
        // buffer without it being full emerge
        debug_assert!(self.is_full());
        let old_cap = self.capacity();
        self.buf.grow(old_cap + 1);
        unsafe {
            self.handle_capacity_increase(old_cap);
        }
        debug_assert!(!self.is_full());
    }

    /// Modifies the deque in-place so that `len()` is equal to `new_len`,
    /// either by removing excess elements from the back or by appending
    /// elements generated by calling `generator` to the back.
    pub fn resize_with(&mut self, new_len: usize, generator: impl FnMut() -> T) {
        let len = self.len;

        if new_len > len {
            self.extend(repeat_with(generator).take(new_len - len))
        } else {
            self.truncate(new_len);
        }
    }

    /// Rearranges the internal storage of this deque so it is one contiguous
    /// slice, which is then returned.
    ///
    /// This method does not allocate and does not change the order of the
    /// inserted elements. As it returns a mutable slice, this can be used to
    /// sort a deque.
    ///
    /// Once the internal storage is contiguous, the [`as_slices`] and
    /// [`as_mut_slices`] methods will return the entire contents of the
    /// deque in a single slice.
    ///
    /// [`as_slices`]: SmallDeque::as_slices
    /// [`as_mut_slices`]: SmallDeque::as_mut_slices
    pub fn make_contiguous(&mut self) -> &mut [T] {
        if T::IS_ZST {
            self.head = 0;
        }

        if self.is_contiguous() {
            unsafe {
                return core::slice::from_raw_parts_mut(self.ptr_mut().add(self.head), self.len);
            }
        }

        let &mut Self { head, len, .. } = self;
        let ptr = self.ptr_mut();
        let cap = self.capacity();

        let free = cap - len;
        let head_len = cap - head;
        let tail = len - head_len;
        let tail_len = tail;

        if free >= head_len {
            // there is enough free space to copy the head in one go,
            // this means that we first shift the tail backwards, and then
            // copy the head to the correct position.
            //
            // from: DEFGH....ABC
            // to:   ABCDEFGH....
            unsafe {
                self.copy(0, head_len, tail_len);
                // ...DEFGH.ABC
                self.copy_nonoverlapping(head, 0, head_len);
                // ABCDEFGH....
            }

            self.head = 0;
        } else if free >= tail_len {
            // there is enough free space to copy the tail in one go,
            // this means that we first shift the head forwards, and then
            // copy the tail to the correct position.
            //
            // from: FGH....ABCDE
            // to:   ...ABCDEFGH.
            unsafe {
                self.copy(head, tail, head_len);
                // FGHABCDE....
                self.copy_nonoverlapping(0, tail + head_len, tail_len);
                // ...ABCDEFGH.
            }

            self.head = tail;
        } else {
            // `free` is smaller than both `head_len` and `tail_len`.
            // the general algorithm for this first moves the slices
            // right next to each other and then uses `slice::rotate`
            // to rotate them into place:
            //
            // initially:   HIJK..ABCDEFG
            // step 1:      ..HIJKABCDEFG
            // step 2:      ..ABCDEFGHIJK
            //
            // or:
            //
            // initially:   FGHIJK..ABCDE
            // step 1:      FGHIJKABCDE..
            // step 2:      ABCDEFGHIJK..

            // pick the shorter of the 2 slices to reduce the amount
            // of memory that needs to be moved around.
            if head_len > tail_len {
                // tail is shorter, so:
                //  1. copy tail forwards
                //  2. rotate used part of the buffer
                //  3. update head to point to the new beginning (which is just `free`)

                unsafe {
                    // if there is no free space in the buffer, then the slices are already
                    // right next to each other and we don't need to move any memory.
                    if free != 0 {
                        // because we only move the tail forward as much as there's free space
                        // behind it, we don't overwrite any elements of the head slice, and
                        // the slices end up right next to each other.
                        self.copy(0, free, tail_len);
                    }

                    // We just copied the tail right next to the head slice,
                    // so all of the elements in the range are initialized
                    let slice = &mut *self.buffer_range_mut(free..self.capacity());

                    // because the deque wasn't contiguous, we know that `tail_len < self.len == slice.len()`,
                    // so this will never panic.
                    slice.rotate_left(tail_len);

                    // the used part of the buffer now is `free..self.capacity()`, so set
                    // `head` to the beginning of that range.
                    self.head = free;
                }
            } else {
                // head is shorter so:
                //  1. copy head backwards
                //  2. rotate used part of the buffer
                //  3. update head to point to the new beginning (which is the beginning of the buffer)

                unsafe {
                    // if there is no free space in the buffer, then the slices are already
                    // right next to each other and we don't need to move any memory.
                    if free != 0 {
                        // copy the head slice to lie right behind the tail slice.
                        self.copy(self.head, tail_len, head_len);
                    }

                    // because we copied the head slice so that both slices lie right
                    // next to each other, all the elements in the range are initialized.
                    let slice = &mut *self.buffer_range_mut(0..self.len);

                    // because the deque wasn't contiguous, we know that `head_len < self.len == slice.len()`
                    // so this will never panic.
                    slice.rotate_right(head_len);

                    // the used part of the buffer now is `0..self.len`, so set
                    // `head` to the beginning of that range.
                    self.head = 0;
                }
            }
        }

        unsafe { core::slice::from_raw_parts_mut(ptr.add(self.head), self.len) }
    }

    /// Rotates the double-ended queue `n` places to the left.
    ///
    /// Equivalently,
    /// - Rotates item `n` into the first position.
    /// - Pops the first `n` items and pushes them to the end.
    /// - Rotates `len() - n` places to the right.
    ///
    /// # Panics
    ///
    /// If `n` is greater than `len()`. Note that `n == len()`
    /// does _not_ panic and is a no-op rotation.
    ///
    /// # Complexity
    ///
    /// Takes `*O*(min(n, len() - n))` time and no extra space.
    pub fn rotate_left(&mut self, n: usize) {
        assert!(n <= self.len());
        let k = self.len - n;
        if n <= k {
            unsafe { self.rotate_left_inner(n) }
        } else {
            unsafe { self.rotate_right_inner(k) }
        }
    }

    /// Rotates the double-ended queue `n` places to the right.
    ///
    /// Equivalently,
    /// - Rotates the first item into position `n`.
    /// - Pops the last `n` items and pushes them to the front.
    /// - Rotates `len() - n` places to the left.
    ///
    /// # Panics
    ///
    /// If `n` is greater than `len()`. Note that `n == len()`
    /// does _not_ panic and is a no-op rotation.
    ///
    /// # Complexity
    ///
    /// Takes `*O*(min(n, len() - n))` time and no extra space.
    pub fn rotate_right(&mut self, n: usize) {
        assert!(n <= self.len());
        let k = self.len - n;
        if n <= k {
            unsafe { self.rotate_right_inner(n) }
        } else {
            unsafe { self.rotate_left_inner(k) }
        }
    }

    // SAFETY: the following two methods require that the rotation amount
    // be less than half the length of the deque.
    //
    // `wrap_copy` requires that `min(x, capacity() - x) + copy_len <= capacity()`,
    // but then `min` is never more than half the capacity, regardless of x,
    // so it's sound to call here because we're calling with something
    // less than half the length, which is never above half the capacity.
    unsafe fn rotate_left_inner(&mut self, mid: usize) {
        debug_assert!(mid * 2 <= self.len());
        unsafe {
            self.wrap_copy(self.head, self.to_physical_idx(self.len), mid);
        }
        self.head = self.to_physical_idx(mid);
    }

    unsafe fn rotate_right_inner(&mut self, k: usize) {
        debug_assert!(k * 2 <= self.len());
        self.head = self.wrap_sub(self.head, k);
        unsafe {
            self.wrap_copy(self.to_physical_idx(self.len), self.head, k);
        }
    }

    /// Binary searches this `SmallDeque` for a given element.
    /// If the `SmallDeque` is not sorted, the returned result is unspecified and
    /// meaningless.
    ///
    /// If the value is found then [`Result::Ok`] is returned, containing the
    /// index of the matching element. If there are multiple matches, then any
    /// one of the matches could be returned. If the value is not found then
    /// [`Result::Err`] is returned, containing the index where a matching
    /// element could be inserted while maintaining sorted order.
    ///
    /// See also [`binary_search_by`], [`binary_search_by_key`], and [`partition_point`].
    ///
    /// [`binary_search_by`]: SmallDeque::binary_search_by
    /// [`binary_search_by_key`]: SmallDeque::binary_search_by_key
    /// [`partition_point`]: SmallDeque::partition_point
    ///
    #[inline]
    pub fn binary_search(&self, x: &T) -> Result<usize, usize>
    where
        T: Ord,
    {
        self.binary_search_by(|e| e.cmp(x))
    }

    /// Binary searches this `SmallDeque` with a comparator function.
    ///
    /// The comparator function should return an order code that indicates
    /// whether its argument is `Less`, `Equal` or `Greater` the desired
    /// target.
    /// If the `SmallDeque` is not sorted or if the comparator function does not
    /// implement an order consistent with the sort order of the underlying
    /// `SmallDeque`, the returned result is unspecified and meaningless.
    ///
    /// If the value is found then [`Result::Ok`] is returned, containing the
    /// index of the matching element. If there are multiple matches, then any
    /// one of the matches could be returned. If the value is not found then
    /// [`Result::Err`] is returned, containing the index where a matching
    /// element could be inserted while maintaining sorted order.
    ///
    /// See also [`binary_search`], [`binary_search_by_key`], and [`partition_point`].
    ///
    /// [`binary_search`]: SmallDeque::binary_search
    /// [`binary_search_by_key`]: SmallDeque::binary_search_by_key
    /// [`partition_point`]: SmallDeque::partition_point
    pub fn binary_search_by<'a, F>(&'a self, mut f: F) -> Result<usize, usize>
    where
        F: FnMut(&'a T) -> Ordering,
    {
        let (front, back) = self.as_slices();
        // clippy doesn't recognize that `f` would be moved if we followed it's recommendation
        #[allow(clippy::redundant_closure)]
        let cmp_back = back.first().map(|e| f(e));

        if let Some(Ordering::Equal) = cmp_back {
            Ok(front.len())
        } else if let Some(Ordering::Less) = cmp_back {
            back.binary_search_by(f)
                .map(|idx| idx + front.len())
                .map_err(|idx| idx + front.len())
        } else {
            front.binary_search_by(f)
        }
    }

    /// Binary searches this `SmallDeque` with a key extraction function.
    ///
    /// Assumes that the deque is sorted by the key, for instance with
    /// [`make_contiguous().sort_by_key()`] using the same key extraction function.
    /// If the deque is not sorted by the key, the returned result is
    /// unspecified and meaningless.
    ///
    /// If the value is found then [`Result::Ok`] is returned, containing the
    /// index of the matching element. If there are multiple matches, then any
    /// one of the matches could be returned. If the value is not found then
    /// [`Result::Err`] is returned, containing the index where a matching
    /// element could be inserted while maintaining sorted order.
    ///
    /// See also [`binary_search`], [`binary_search_by`], and [`partition_point`].
    ///
    /// [`make_contiguous().sort_by_key()`]: SmallDeque::make_contiguous
    /// [`binary_search`]: SmallDeque::binary_search
    /// [`binary_search_by`]: SmallDeque::binary_search_by
    /// [`partition_point`]: SmallDeque::partition_point
    #[inline]
    pub fn binary_search_by_key<'a, B, F>(&'a self, b: &B, mut f: F) -> Result<usize, usize>
    where
        F: FnMut(&'a T) -> B,
        B: Ord,
    {
        self.binary_search_by(|k| f(k).cmp(b))
    }

    /// Returns the index of the partition point according to the given predicate
    /// (the index of the first element of the second partition).
    ///
    /// The deque is assumed to be partitioned according to the given predicate.
    /// This means that all elements for which the predicate returns true are at the start of the deque
    /// and all elements for which the predicate returns false are at the end.
    /// For example, `[7, 15, 3, 5, 4, 12, 6]` is partitioned under the predicate `x % 2 != 0`
    /// (all odd numbers are at the start, all even at the end).
    ///
    /// If the deque is not partitioned, the returned result is unspecified and meaningless,
    /// as this method performs a kind of binary search.
    ///
    /// See also [`binary_search`], [`binary_search_by`], and [`binary_search_by_key`].
    ///
    /// [`binary_search`]: SmallDeque::binary_search
    /// [`binary_search_by`]: SmallDeque::binary_search_by
    /// [`binary_search_by_key`]: SmallDeque::binary_search_by_key
    pub fn partition_point<P>(&self, mut pred: P) -> usize
    where
        P: FnMut(&T) -> bool,
    {
        let (front, back) = self.as_slices();

        #[allow(clippy::redundant_closure)]
        if let Some(true) = back.first().map(|v| pred(v)) {
            back.partition_point(pred) + front.len()
        } else {
            front.partition_point(pred)
        }
    }
}

impl<T: Clone, const N: usize> SmallDeque<T, N> {
    /// Modifies the deque in-place so that `len()` is equal to new_len,
    /// either by removing excess elements from the back or by appending clones of `value`
    /// to the back.
    pub fn resize(&mut self, new_len: usize, value: T) {
        if new_len > self.len() {
            let extra = new_len - self.len();
            self.extend(repeat_n(value, extra))
        } else {
            self.truncate(new_len);
        }
    }
}

impl<T, const N: usize> SmallDeque<T, N> {
    #[inline]
    fn ptr(&self) -> *const T {
        self.buf.as_ptr()
    }

    #[inline]
    fn ptr_mut(&mut self) -> *mut T {
        self.buf.as_mut_ptr()
    }

    /// Appends an element to the buffer.
    ///
    /// # Safety
    ///
    /// May only be called if `deque.len() < deque.capacity()`
    #[inline]
    unsafe fn push_unchecked(&mut self, element: T) {
        // SAFETY: Because of the precondition, it's guaranteed that there is space in the logical
        // array after the last element.
        unsafe { self.buffer_write(self.to_physical_idx(self.len), element) };
        // This can't overflow because `deque.len() < deque.capacity() <= usize::MAX`
        self.len += 1;
    }

    /// Moves an element out of the buffer
    #[inline]
    unsafe fn buffer_read(&mut self, offset: usize) -> T {
        unsafe { ptr::read(self.ptr().add(offset)) }
    }

    /// Writes an element into the buffer, moving it.
    #[inline]
    unsafe fn buffer_write(&mut self, offset: usize, value: T) {
        unsafe {
            ptr::write(self.ptr_mut().add(offset), value);
        }
    }

    /// Returns a slice pointer into the buffer.
    /// `range` must lie inside `0..self.capacity()`.
    #[inline]
    unsafe fn buffer_range(&self, range: core::ops::Range<usize>) -> *const [T] {
        unsafe { ptr::slice_from_raw_parts(self.ptr().add(range.start), range.end - range.start) }
    }

    /// Returns a slice pointer into the buffer.
    /// `range` must lie inside `0..self.capacity()`.
    #[inline]
    unsafe fn buffer_range_mut(&mut self, range: core::ops::Range<usize>) -> *mut [T] {
        unsafe {
            ptr::slice_from_raw_parts_mut(self.ptr_mut().add(range.start), range.end - range.start)
        }
    }

    /// Returns `true` if the buffer is at full capacity.
    #[inline]
    fn is_full(&self) -> bool {
        self.len == self.capacity()
    }

    /// Returns the index in the underlying buffer for a given logical element index + addend.
    #[inline]
    fn wrap_add(&self, idx: usize, addend: usize) -> usize {
        wrap_index(idx.wrapping_add(addend), self.capacity())
    }

    #[inline]
    fn to_physical_idx(&self, idx: usize) -> usize {
        self.wrap_add(self.head, idx)
    }

    /// Returns the index in the underlying buffer for a given logical element index - subtrahend.
    #[inline]
    fn wrap_sub(&self, idx: usize, subtrahend: usize) -> usize {
        wrap_index(idx.wrapping_sub(subtrahend).wrapping_add(self.capacity()), self.capacity())
    }

    /// Copies a contiguous block of memory len long from src to dst
    #[inline]
    unsafe fn copy(&mut self, src: usize, dst: usize, len: usize) {
        debug_assert!(
            dst + len <= self.capacity(),
            "cpy dst={} src={} len={} cap={}",
            dst,
            src,
            len,
            self.capacity()
        );
        debug_assert!(
            src + len <= self.capacity(),
            "cpy dst={} src={} len={} cap={}",
            dst,
            src,
            len,
            self.capacity()
        );
        unsafe {
            ptr::copy(self.ptr().add(src), self.ptr_mut().add(dst), len);
        }
    }

    /// Copies a contiguous block of memory len long from src to dst
    #[inline]
    unsafe fn copy_nonoverlapping(&mut self, src: usize, dst: usize, len: usize) {
        debug_assert!(
            dst + len <= self.capacity(),
            "cno dst={} src={} len={} cap={}",
            dst,
            src,
            len,
            self.capacity()
        );
        debug_assert!(
            src + len <= self.capacity(),
            "cno dst={} src={} len={} cap={}",
            dst,
            src,
            len,
            self.capacity()
        );
        unsafe {
            ptr::copy_nonoverlapping(self.ptr().add(src), self.ptr_mut().add(dst), len);
        }
    }

    /// Copies a potentially wrapping block of memory len long from src to dest.
    /// (abs(dst - src) + len) must be no larger than capacity() (There must be at
    /// most one continuous overlapping region between src and dest).
    unsafe fn wrap_copy(&mut self, src: usize, dst: usize, len: usize) {
        debug_assert!(
            core::cmp::min(src.abs_diff(dst), self.capacity() - src.abs_diff(dst)) + len
                <= self.capacity(),
            "wrc dst={} src={} len={} cap={}",
            dst,
            src,
            len,
            self.capacity()
        );

        // If T is a ZST, don't do any copying.
        if T::IS_ZST || src == dst || len == 0 {
            return;
        }

        let dst_after_src = self.wrap_sub(dst, src) < len;

        let src_pre_wrap_len = self.capacity() - src;
        let dst_pre_wrap_len = self.capacity() - dst;
        let src_wraps = src_pre_wrap_len < len;
        let dst_wraps = dst_pre_wrap_len < len;

        match (dst_after_src, src_wraps, dst_wraps) {
            (_, false, false) => {
                // src doesn't wrap, dst doesn't wrap
                //
                //        S . . .
                // 1 [_ _ A A B B C C _]
                // 2 [_ _ A A A A B B _]
                //            D . . .
                //
                unsafe {
                    self.copy(src, dst, len);
                }
            }
            (false, false, true) => {
                // dst before src, src doesn't wrap, dst wraps
                //
                //    S . . .
                // 1 [A A B B _ _ _ C C]
                // 2 [A A B B _ _ _ A A]
                // 3 [B B B B _ _ _ A A]
                //    . .           D .
                //
                unsafe {
                    self.copy(src, dst, dst_pre_wrap_len);
                    self.copy(src + dst_pre_wrap_len, 0, len - dst_pre_wrap_len);
                }
            }
            (true, false, true) => {
                // src before dst, src doesn't wrap, dst wraps
                //
                //              S . . .
                // 1 [C C _ _ _ A A B B]
                // 2 [B B _ _ _ A A B B]
                // 3 [B B _ _ _ A A A A]
                //    . .           D .
                //
                unsafe {
                    self.copy(src + dst_pre_wrap_len, 0, len - dst_pre_wrap_len);
                    self.copy(src, dst, dst_pre_wrap_len);
                }
            }
            (false, true, false) => {
                // dst before src, src wraps, dst doesn't wrap
                //
                //    . .           S .
                // 1 [C C _ _ _ A A B B]
                // 2 [C C _ _ _ B B B B]
                // 3 [C C _ _ _ B B C C]
                //              D . . .
                //
                unsafe {
                    self.copy(src, dst, src_pre_wrap_len);
                    self.copy(0, dst + src_pre_wrap_len, len - src_pre_wrap_len);
                }
            }
            (true, true, false) => {
                // src before dst, src wraps, dst doesn't wrap
                //
                //    . .           S .
                // 1 [A A B B _ _ _ C C]
                // 2 [A A A A _ _ _ C C]
                // 3 [C C A A _ _ _ C C]
                //    D . . .
                //
                unsafe {
                    self.copy(0, dst + src_pre_wrap_len, len - src_pre_wrap_len);
                    self.copy(src, dst, src_pre_wrap_len);
                }
            }
            (false, true, true) => {
                // dst before src, src wraps, dst wraps
                //
                //    . . .         S .
                // 1 [A B C D _ E F G H]
                // 2 [A B C D _ E G H H]
                // 3 [A B C D _ E G H A]
                // 4 [B C C D _ E G H A]
                //    . .         D . .
                //
                debug_assert!(dst_pre_wrap_len > src_pre_wrap_len);
                let delta = dst_pre_wrap_len - src_pre_wrap_len;
                unsafe {
                    self.copy(src, dst, src_pre_wrap_len);
                    self.copy(0, dst + src_pre_wrap_len, delta);
                    self.copy(delta, 0, len - dst_pre_wrap_len);
                }
            }
            (true, true, true) => {
                // src before dst, src wraps, dst wraps
                //
                //    . .         S . .
                // 1 [A B C D _ E F G H]
                // 2 [A A B D _ E F G H]
                // 3 [H A B D _ E F G H]
                // 4 [H A B D _ E F F G]
                //    . . .         D .
                //
                debug_assert!(src_pre_wrap_len > dst_pre_wrap_len);
                let delta = src_pre_wrap_len - dst_pre_wrap_len;
                unsafe {
                    self.copy(0, delta, len - src_pre_wrap_len);
                    self.copy(self.capacity() - delta, 0, delta);
                    self.copy(src, dst, dst_pre_wrap_len);
                }
            }
        }
    }

    /// Copies all values from `src` to `dst`, wrapping around if needed.
    /// Assumes capacity is sufficient.
    #[inline]
    unsafe fn copy_slice(&mut self, dst: usize, src: &[T]) {
        debug_assert!(src.len() <= self.capacity());
        let head_room = self.capacity() - dst;
        if src.len() <= head_room {
            unsafe {
                ptr::copy_nonoverlapping(src.as_ptr(), self.ptr_mut().add(dst), src.len());
            }
        } else {
            let (left, right) = src.split_at(head_room);
            unsafe {
                ptr::copy_nonoverlapping(left.as_ptr(), self.ptr_mut().add(dst), left.len());
                ptr::copy_nonoverlapping(right.as_ptr(), self.ptr_mut(), right.len());
            }
        }
    }

    /// Writes all values from `iter` to `dst`.
    ///
    /// # Safety
    ///
    /// Assumes no wrapping around happens.
    /// Assumes capacity is sufficient.
    #[inline]
    unsafe fn write_iter(
        &mut self,
        dst: usize,
        iter: impl Iterator<Item = T>,
        written: &mut usize,
    ) {
        iter.enumerate().for_each(|(i, element)| unsafe {
            self.buffer_write(dst + i, element);
            *written += 1;
        });
    }

    /// Writes all values from `iter` to `dst`, wrapping
    /// at the end of the buffer and returns the number
    /// of written values.
    ///
    /// # Safety
    ///
    /// Assumes that `iter` yields at most `len` items.
    /// Assumes capacity is sufficient.
    unsafe fn write_iter_wrapping(
        &mut self,
        dst: usize,
        mut iter: impl Iterator<Item = T>,
        len: usize,
    ) -> usize {
        struct Guard<'a, T, const N: usize> {
            deque: &'a mut SmallDeque<T, N>,
            written: usize,
        }

        impl<T, const N: usize> Drop for Guard<'_, T, N> {
            fn drop(&mut self) {
                self.deque.len += self.written;
            }
        }

        let head_room = self.capacity() - dst;

        let mut guard = Guard {
            deque: self,
            written: 0,
        };

        if head_room >= len {
            unsafe { guard.deque.write_iter(dst, iter, &mut guard.written) };
        } else {
            unsafe {
                guard.deque.write_iter(
                    dst,
                    ByRefSized(&mut iter).take(head_room),
                    &mut guard.written,
                );
                guard.deque.write_iter(0, iter, &mut guard.written)
            };
        }

        guard.written
    }

    /// Frobs the head and tail sections around to handle the fact that we
    /// just reallocated. Unsafe because it trusts old_capacity.
    #[inline]
    unsafe fn handle_capacity_increase(&mut self, old_capacity: usize) {
        let new_capacity = self.capacity();
        debug_assert!(new_capacity >= old_capacity);

        // Move the shortest contiguous section of the ring buffer
        //
        // H := head
        // L := last element (`self.to_physical_idx(self.len - 1)`)
        //
        //    H             L
        //   [o o o o o o o o ]
        //    H             L
        // A [o o o o o o o o . . . . . . . . ]
        //        L H
        //   [o o o o o o o o ]
        //          H             L
        // B [. . . o o o o o o o o . . . . . ]
        //              L H
        //   [o o o o o o o o ]
        //              L                 H
        // C [o o o o o o . . . . . . . . o o ]

        // can't use is_contiguous() because the capacity is already updated.
        if self.head <= old_capacity - self.len {
            // A
            // Nop
        } else {
            let head_len = old_capacity - self.head;
            let tail_len = self.len - head_len;
            if head_len > tail_len && new_capacity - old_capacity >= tail_len {
                // B
                unsafe {
                    self.copy_nonoverlapping(0, old_capacity, tail_len);
                }
            } else {
                // C
                let new_head = new_capacity - head_len;
                unsafe {
                    // can't use copy_nonoverlapping here, because if e.g. head_len = 2
                    // and new_capacity = old_capacity + 1, then the heads overlap.
                    self.copy(self.head, new_head, head_len);
                }
                self.head = new_head;
            }
        }
        debug_assert!(self.head < self.capacity() || self.capacity() == 0);
    }
}

/// Returns the index in the underlying buffer for a given logical element index.
#[inline]
fn wrap_index(logical_index: usize, capacity: usize) -> usize {
    debug_assert!(
        (logical_index == 0 && capacity == 0)
            || logical_index < capacity
            || (logical_index - capacity) < capacity
    );
    if logical_index >= capacity {
        logical_index - capacity
    } else {
        logical_index
    }
}

impl<T: PartialEq, const N: usize> PartialEq for SmallDeque<T, N> {
    fn eq(&self, other: &Self) -> bool {
        if self.len != other.len() {
            return false;
        }
        let (sa, sb) = self.as_slices();
        let (oa, ob) = other.as_slices();
        match sa.len().cmp(&oa.len()) {
            Ordering::Equal => sa == oa && sb == ob,
            Ordering::Less => {
                // Always divisible in three sections, for example:
                // self:  [a b c|d e f]
                // other: [0 1 2 3|4 5]
                // front = 3, mid = 1,
                // [a b c] == [0 1 2] && [d] == [3] && [e f] == [4 5]
                let front = sa.len();
                let mid = oa.len() - front;

                let (oa_front, oa_mid) = oa.split_at(front);
                let (sb_mid, sb_back) = sb.split_at(mid);
                debug_assert_eq!(sa.len(), oa_front.len());
                debug_assert_eq!(sb_mid.len(), oa_mid.len());
                debug_assert_eq!(sb_back.len(), ob.len());
                sa == oa_front && sb_mid == oa_mid && sb_back == ob
            }
            Ordering::Greater => {
                let front = oa.len();
                let mid = sa.len() - front;

                let (sa_front, sa_mid) = sa.split_at(front);
                let (ob_mid, ob_back) = ob.split_at(mid);
                debug_assert_eq!(sa_front.len(), oa.len());
                debug_assert_eq!(sa_mid.len(), ob_mid.len());
                debug_assert_eq!(sb.len(), ob_back.len());
                sa_front == oa && sa_mid == ob_mid && sb == ob_back
            }
        }
    }
}

impl<T: Eq, const N: usize> Eq for SmallDeque<T, N> {}

macro_rules! __impl_slice_eq1 {
    ([$($vars:tt)*] $lhs:ty, $rhs:ty, $($constraints:tt)*) => {
        impl<T, U, $($vars)*> PartialEq<$rhs> for $lhs
        where
            T: PartialEq<U>,
            $($constraints)*
        {
            fn eq(&self, other: &$rhs) -> bool {
                if self.len() != other.len() {
                    return false;
                }
                let (sa, sb) = self.as_slices();
                let (oa, ob) = other[..].split_at(sa.len());
                sa == oa && sb == ob
            }
        }
    }
}

__impl_slice_eq1! { [A: alloc::alloc::Allocator, const N: usize] SmallDeque<T, N>, Vec<U, A>, }
__impl_slice_eq1! { [const N: usize] SmallDeque<T, N>, SmallVec<[U; N]>, }
__impl_slice_eq1! { [const N: usize] SmallDeque<T, N>, &[U], }
__impl_slice_eq1! { [const N: usize] SmallDeque<T, N>, &mut [U], }
__impl_slice_eq1! { [const N: usize, const M: usize] SmallDeque<T, N>, [U; M], }
__impl_slice_eq1! { [const N: usize, const M: usize] SmallDeque<T, N>, &[U; M], }
__impl_slice_eq1! { [const N: usize, const M: usize] SmallDeque<T, N>, &mut [U; M], }

impl<T: PartialOrd, const N: usize> PartialOrd for SmallDeque<T, N> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        self.iter().partial_cmp(other.iter())
    }
}

impl<T: Ord, const N: usize> Ord for SmallDeque<T, N> {
    #[inline]
    fn cmp(&self, other: &Self) -> Ordering {
        self.iter().cmp(other.iter())
    }
}

impl<T: core::hash::Hash, const N: usize> core::hash::Hash for SmallDeque<T, N> {
    fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
        state.write_length_prefix(self.len);
        // It's not possible to use Hash::hash_slice on slices
        // returned by as_slices method as their length can vary
        // in otherwise identical deques.
        //
        // Hasher only guarantees equivalence for the exact same
        // set of calls to its methods.
        self.iter().for_each(|elem| elem.hash(state));
    }
}

impl<T, const N: usize> Index<usize> for SmallDeque<T, N> {
    type Output = T;

    #[inline]
    fn index(&self, index: usize) -> &T {
        self.get(index).expect("Out of bounds access")
    }
}

impl<T, const N: usize> IndexMut<usize> for SmallDeque<T, N> {
    #[inline]
    fn index_mut(&mut self, index: usize) -> &mut T {
        self.get_mut(index).expect("Out of bounds access")
    }
}

impl<T, const N: usize> FromIterator<T> for SmallDeque<T, N> {
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self {
        SpecFromIter::spec_from_iter(iter.into_iter())
    }
}

impl<T, const N: usize> IntoIterator for SmallDeque<T, N> {
    type IntoIter = IntoIter<T, N>;
    type Item = T;

    /// Consumes the deque into a front-to-back iterator yielding elements by
    /// value.
    fn into_iter(self) -> IntoIter<T, N> {
        IntoIter::new(self)
    }
}

impl<'a, T, const N: usize> IntoIterator for &'a SmallDeque<T, N> {
    type IntoIter = Iter<'a, T>;
    type Item = &'a T;

    fn into_iter(self) -> Iter<'a, T> {
        self.iter()
    }
}

impl<'a, T, const N: usize> IntoIterator for &'a mut SmallDeque<T, N> {
    type IntoIter = IterMut<'a, T>;
    type Item = &'a mut T;

    fn into_iter(self) -> IterMut<'a, T> {
        self.iter_mut()
    }
}

impl<T, const N: usize> Extend<T> for SmallDeque<T, N> {
    fn extend<I: IntoIterator<Item = T>>(&mut self, iter: I) {
        <Self as SpecExtend<T, I::IntoIter>>::spec_extend(self, iter.into_iter());
    }

    #[inline]
    fn extend_one(&mut self, elem: T) {
        self.push_back(elem);
    }

    #[inline]
    fn extend_reserve(&mut self, additional: usize) {
        self.reserve(additional);
    }

    #[inline]
    unsafe fn extend_one_unchecked(&mut self, item: T) {
        // SAFETY: Our preconditions ensure the space has been reserved, and `extend_reserve` is implemented correctly.
        unsafe {
            self.push_unchecked(item);
        }
    }
}

impl<'a, T: 'a + Copy, const N: usize> Extend<&'a T> for SmallDeque<T, N> {
    fn extend<I: IntoIterator<Item = &'a T>>(&mut self, iter: I) {
        self.spec_extend(iter.into_iter());
    }

    #[inline]
    fn extend_one(&mut self, &elem: &'a T) {
        self.push_back(elem);
    }

    #[inline]
    fn extend_reserve(&mut self, additional: usize) {
        self.reserve(additional);
    }

    #[inline]
    unsafe fn extend_one_unchecked(&mut self, &item: &'a T) {
        // SAFETY: Our preconditions ensure the space has been reserved, and `extend_reserve` is implemented correctly.
        unsafe {
            self.push_unchecked(item);
        }
    }
}

impl<T: fmt::Debug, const N: usize> fmt::Debug for SmallDeque<T, N> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(self.iter()).finish()
    }
}

impl<T, const N: usize> From<SmallVec<[T; N]>> for SmallDeque<T, N> {
    /// Turn a [`SmallVec<[T; N]>`] into a [`SmallDeque<T, N>`].
    ///
    /// [`SmallVec<[T; N]>`]: smallvec::SmallVec
    /// [`SmallDeque<T, N>`]: crate::adt::SmallDeque
    ///
    /// This conversion is guaranteed to run in *O*(1) time
    /// and to not re-allocate the `Vec`'s buffer or allocate
    /// any additional memory.
    #[inline]
    fn from(buf: SmallVec<[T; N]>) -> Self {
        let len = buf.len();
        Self { head: 0, len, buf }
    }
}

impl<T, const N: usize> From<SmallDeque<T, N>> for SmallVec<[T; N]> {
    /// Turn a [`SmallDeque<T, N>`] into a [`SmallVec<[T; N]>`].
    ///
    /// [`SmallVec<[T; N]>`]: smallvec::SmallVec
    /// [`SmallDeque<T, N>`]: crate::adt::SmallDeque
    ///
    /// This never needs to re-allocate, but does need to do *O*(*n*) data movement if
    /// the circular buffer doesn't happen to be at the beginning of the allocation.
    fn from(mut other: SmallDeque<T, N>) -> Self {
        use core::mem::ManuallyDrop;

        other.make_contiguous();

        unsafe {
            if other.buf.spilled() {
                let mut other = ManuallyDrop::new(other);
                let buf = other.buf.as_mut_ptr();
                let len = other.len();
                let cap = other.capacity();

                if other.head != 0 {
                    ptr::copy(buf.add(other.head), buf, len);
                }
                SmallVec::from_raw_parts(buf, len, cap)
            } else {
                // `other` is entirely stack-allocated, so we need to produce a new copy that
                // has all of the elements starting at index 0, if not already
                if other.head == 0 {
                    // Steal the underlying vec, and make sure that the length is set
                    let mut buf = other.buf;
                    buf.set_len(other.len);
                    buf
                } else {
                    let mut other = ManuallyDrop::new(other);
                    let ptr = other.buf.as_mut_ptr();
                    let len = other.len();

                    // Construct an uninitialized array on the stack of the same size as the target
                    // SmallVec's inline size, "move" `len` items into it, and the construct the
                    // SmallVec from the raw buffer and len
                    let mut buf = core::mem::MaybeUninit::<T>::uninit_array::<N>();
                    let buf_ptr = core::mem::MaybeUninit::slice_as_mut_ptr(&mut buf);
                    ptr::copy(ptr.add(other.head), buf_ptr, len);
                    // While we are technically potentially letting a subset of elements in the
                    // array that never got uninitialized, be assumed to have been initialized
                    // here - that fact is never material: no references are ever created to those
                    // items, and the array is never dropped, as it is immediately placed in a
                    // ManuallyDrop, and the vector length is set to `len` before any access can
                    // be made to the vector
                    SmallVec::from_buf_and_len(core::mem::MaybeUninit::array_assume_init(buf), len)
                }
            }
        }
    }
}

impl<T, const N: usize> From<[T; N]> for SmallDeque<T, N> {
    /// Converts a `[T; N]` into a `SmallDeque<T, N>`.
    fn from(arr: [T; N]) -> Self {
        use core::mem::ManuallyDrop;

        let mut deq = SmallDeque::<_, N>::with_capacity(N);
        let arr = ManuallyDrop::new(arr);
        if !<T>::IS_ZST {
            // SAFETY: SmallDeque::with_capacity ensures that there is enough capacity.
            unsafe {
                ptr::copy_nonoverlapping(arr.as_ptr(), deq.ptr_mut(), N);
            }
        }
        deq.head = 0;
        deq.len = N;
        deq
    }
}

/// An iterator over the elements of a `SmallDeque`.
///
/// This `struct` is created by the [`iter`] method on [`SmallDeque`]. See its
/// documentation for more.
///
/// [`iter`]: SmallDeque::iter
#[derive(Clone)]
pub struct Iter<'a, T: 'a> {
    i1: core::slice::Iter<'a, T>,
    i2: core::slice::Iter<'a, T>,
}

impl<'a, T> Iter<'a, T> {
    pub(super) fn new(i1: core::slice::Iter<'a, T>, i2: core::slice::Iter<'a, T>) -> Self {
        Self { i1, i2 }
    }
}

impl<T: fmt::Debug> fmt::Debug for Iter<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Iter")
            .field(&self.i1.as_slice())
            .field(&self.i2.as_slice())
            .finish()
    }
}

impl<'a, T> Iterator for Iter<'a, T> {
    type Item = &'a T;

    #[inline]
    fn next(&mut self) -> Option<&'a T> {
        match self.i1.next() {
            Some(val) => Some(val),
            None => {
                // most of the time, the iterator will either always
                // call next(), or always call next_back(). By swapping
                // the iterators once the first one is empty, we ensure
                // that the first branch is taken as often as possible,
                // without sacrificing correctness, as i1 is empty anyways
                core::mem::swap(&mut self.i1, &mut self.i2);
                self.i1.next()
            }
        }
    }

    fn advance_by(&mut self, n: usize) -> Result<(), core::num::NonZero<usize>> {
        let remaining = self.i1.advance_by(n);
        match remaining {
            Ok(()) => Ok(()),
            Err(n) => {
                core::mem::swap(&mut self.i1, &mut self.i2);
                self.i1.advance_by(n.get())
            }
        }
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        let len = self.len();
        (len, Some(len))
    }

    fn fold<Acc, F>(self, accum: Acc, mut f: F) -> Acc
    where
        F: FnMut(Acc, Self::Item) -> Acc,
    {
        let accum = self.i1.fold(accum, &mut f);
        self.i2.fold(accum, &mut f)
    }

    fn try_fold<B, F, R>(&mut self, init: B, mut f: F) -> R
    where
        F: FnMut(B, Self::Item) -> R,
        R: core::ops::Try<Output = B>,
    {
        let acc = self.i1.try_fold(init, &mut f)?;
        self.i2.try_fold(acc, &mut f)
    }

    #[inline]
    fn last(mut self) -> Option<&'a T> {
        self.next_back()
    }
}

impl<'a, T> DoubleEndedIterator for Iter<'a, T> {
    #[inline]
    fn next_back(&mut self) -> Option<&'a T> {
        match self.i2.next_back() {
            Some(val) => Some(val),
            None => {
                // most of the time, the iterator will either always
                // call next(), or always call next_back(). By swapping
                // the iterators once the second one is empty, we ensure
                // that the first branch is taken as often as possible,
                // without sacrificing correctness, as i2 is empty anyways
                core::mem::swap(&mut self.i1, &mut self.i2);
                self.i2.next_back()
            }
        }
    }

    fn advance_back_by(&mut self, n: usize) -> Result<(), core::num::NonZero<usize>> {
        match self.i2.advance_back_by(n) {
            Ok(()) => Ok(()),
            Err(n) => {
                core::mem::swap(&mut self.i1, &mut self.i2);
                self.i2.advance_back_by(n.get())
            }
        }
    }

    fn rfold<Acc, F>(self, accum: Acc, mut f: F) -> Acc
    where
        F: FnMut(Acc, Self::Item) -> Acc,
    {
        let accum = self.i2.rfold(accum, &mut f);
        self.i1.rfold(accum, &mut f)
    }

    fn try_rfold<B, F, R>(&mut self, init: B, mut f: F) -> R
    where
        F: FnMut(B, Self::Item) -> R,
        R: core::ops::Try<Output = B>,
    {
        let acc = self.i2.try_rfold(init, &mut f)?;
        self.i1.try_rfold(acc, &mut f)
    }
}

impl<T> ExactSizeIterator for Iter<'_, T> {
    fn len(&self) -> usize {
        self.i1.len() + self.i2.len()
    }

    fn is_empty(&self) -> bool {
        self.i1.is_empty() && self.i2.is_empty()
    }
}

impl<T> core::iter::FusedIterator for Iter<'_, T> {}

unsafe impl<T> core::iter::TrustedLen for Iter<'_, T> {}

/// A mutable iterator over the elements of a `SmallDeque`.
///
/// This `struct` is created by the [`iter_mut`] method on [`SmallDeque`]. See its
/// documentation for more.
///
/// [`iter_mut`]: SmallDeque::iter_mut
pub struct IterMut<'a, T: 'a> {
    i1: core::slice::IterMut<'a, T>,
    i2: core::slice::IterMut<'a, T>,
}

impl<'a, T> IterMut<'a, T> {
    pub(super) fn new(i1: core::slice::IterMut<'a, T>, i2: core::slice::IterMut<'a, T>) -> Self {
        Self { i1, i2 }
    }
}

impl<T: fmt::Debug> fmt::Debug for IterMut<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("IterMut")
            .field(&self.i1.as_slice())
            .field(&self.i2.as_slice())
            .finish()
    }
}

impl<'a, T> Iterator for IterMut<'a, T> {
    type Item = &'a mut T;

    #[inline]
    fn next(&mut self) -> Option<&'a mut T> {
        match self.i1.next() {
            Some(val) => Some(val),
            None => {
                // most of the time, the iterator will either always
                // call next(), or always call next_back(). By swapping
                // the iterators once the first one is empty, we ensure
                // that the first branch is taken as often as possible,
                // without sacrificing correctness, as i1 is empty anyways
                core::mem::swap(&mut self.i1, &mut self.i2);
                self.i1.next()
            }
        }
    }

    fn advance_by(&mut self, n: usize) -> Result<(), core::num::NonZero<usize>> {
        match self.i1.advance_by(n) {
            Ok(()) => Ok(()),
            Err(remaining) => {
                core::mem::swap(&mut self.i1, &mut self.i2);
                self.i1.advance_by(remaining.get())
            }
        }
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        let len = self.len();
        (len, Some(len))
    }

    fn fold<Acc, F>(self, accum: Acc, mut f: F) -> Acc
    where
        F: FnMut(Acc, Self::Item) -> Acc,
    {
        let accum = self.i1.fold(accum, &mut f);
        self.i2.fold(accum, &mut f)
    }

    fn try_fold<B, F, R>(&mut self, init: B, mut f: F) -> R
    where
        F: FnMut(B, Self::Item) -> R,
        R: core::ops::Try<Output = B>,
    {
        let acc = self.i1.try_fold(init, &mut f)?;
        self.i2.try_fold(acc, &mut f)
    }

    #[inline]
    fn last(mut self) -> Option<&'a mut T> {
        self.next_back()
    }
}

impl<'a, T> DoubleEndedIterator for IterMut<'a, T> {
    #[inline]
    fn next_back(&mut self) -> Option<&'a mut T> {
        match self.i2.next_back() {
            Some(val) => Some(val),
            None => {
                // most of the time, the iterator will either always
                // call next(), or always call next_back(). By swapping
                // the iterators once the first one is empty, we ensure
                // that the first branch is taken as often as possible,
                // without sacrificing correctness, as i2 is empty anyways
                core::mem::swap(&mut self.i1, &mut self.i2);
                self.i2.next_back()
            }
        }
    }

    fn advance_back_by(&mut self, n: usize) -> Result<(), core::num::NonZero<usize>> {
        match self.i2.advance_back_by(n) {
            Ok(()) => Ok(()),
            Err(remaining) => {
                core::mem::swap(&mut self.i1, &mut self.i2);
                self.i2.advance_back_by(remaining.get())
            }
        }
    }

    fn rfold<Acc, F>(self, accum: Acc, mut f: F) -> Acc
    where
        F: FnMut(Acc, Self::Item) -> Acc,
    {
        let accum = self.i2.rfold(accum, &mut f);
        self.i1.rfold(accum, &mut f)
    }

    fn try_rfold<B, F, R>(&mut self, init: B, mut f: F) -> R
    where
        F: FnMut(B, Self::Item) -> R,
        R: core::ops::Try<Output = B>,
    {
        let acc = self.i2.try_rfold(init, &mut f)?;
        self.i1.try_rfold(acc, &mut f)
    }
}

impl<T> ExactSizeIterator for IterMut<'_, T> {
    fn len(&self) -> usize {
        self.i1.len() + self.i2.len()
    }

    fn is_empty(&self) -> bool {
        self.i1.is_empty() && self.i2.is_empty()
    }
}

impl<T> core::iter::FusedIterator for IterMut<'_, T> {}

unsafe impl<T> core::iter::TrustedLen for IterMut<'_, T> {}

/// An owning iterator over the elements of a `SmallDeque`.
///
/// This `struct` is created by the [`into_iter`] method on [`SmallDeque`]
/// (provided by the [`IntoIterator`] trait). See its documentation for more.
///
/// [`into_iter`]: SmallDeque::into_iter
#[derive(Clone)]
pub struct IntoIter<T, const N: usize> {
    inner: SmallDeque<T, N>,
}

impl<T, const N: usize> IntoIter<T, N> {
    pub(super) fn new(inner: SmallDeque<T, N>) -> Self {
        IntoIter { inner }
    }

    pub(super) fn into_smalldeque(self) -> SmallDeque<T, N> {
        self.inner
    }
}

impl<T: fmt::Debug, const N: usize> fmt::Debug for IntoIter<T, N> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("IntoIter").field(&self.inner).finish()
    }
}

impl<T, const M: usize> Iterator for IntoIter<T, M> {
    type Item = T;

    #[inline]
    fn next(&mut self) -> Option<T> {
        self.inner.pop_front()
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        let len = self.inner.len();
        (len, Some(len))
    }

    #[inline]
    fn advance_by(&mut self, n: usize) -> Result<(), core::num::NonZero<usize>> {
        let len = self.inner.len;
        let rem = if len < n {
            self.inner.clear();
            n - len
        } else {
            self.inner.drain(..n);
            0
        };
        core::num::NonZero::new(rem).map_or(Ok(()), Err)
    }

    #[inline]
    fn count(self) -> usize {
        self.inner.len
    }

    fn try_fold<B, F, R>(&mut self, mut init: B, mut f: F) -> R
    where
        F: FnMut(B, Self::Item) -> R,
        R: core::ops::Try<Output = B>,
    {
        struct Guard<'a, T, const M: usize> {
            deque: &'a mut SmallDeque<T, M>,
            // `consumed <= deque.len` always holds.
            consumed: usize,
        }

        impl<T, const M: usize> Drop for Guard<'_, T, M> {
            fn drop(&mut self) {
                self.deque.len -= self.consumed;
                self.deque.head = self.deque.to_physical_idx(self.consumed);
            }
        }

        let mut guard = Guard {
            deque: &mut self.inner,
            consumed: 0,
        };

        let (head, tail) = guard.deque.as_slices();

        init = head
            .iter()
            .map(|elem| {
                guard.consumed += 1;
                // SAFETY: Because we incremented `guard.consumed`, the
                // deque effectively forgot the element, so we can take
                // ownership
                unsafe { ptr::read(elem) }
            })
            .try_fold(init, &mut f)?;

        tail.iter()
            .map(|elem| {
                guard.consumed += 1;
                // SAFETY: Same as above.
                unsafe { ptr::read(elem) }
            })
            .try_fold(init, &mut f)
    }

    #[inline]
    fn fold<B, F>(mut self, init: B, mut f: F) -> B
    where
        F: FnMut(B, Self::Item) -> B,
    {
        match self.try_fold(init, |b, item| Ok::<B, !>(f(b, item))) {
            Ok(b) => b,
            Err(e) => match e {},
        }
    }

    #[inline]
    fn last(mut self) -> Option<Self::Item> {
        self.inner.pop_back()
    }

    fn next_chunk<const N: usize>(
        &mut self,
    ) -> Result<[Self::Item; N], core::array::IntoIter<Self::Item, N>> {
        let mut raw_arr = core::mem::MaybeUninit::uninit_array();
        let raw_arr_ptr = raw_arr.as_mut_ptr().cast();
        let (head, tail) = self.inner.as_slices();

        if head.len() >= N {
            // SAFETY: By manually adjusting the head and length of the deque, we effectively
            // make it forget the first `N` elements, so taking ownership of them is safe.
            unsafe { ptr::copy_nonoverlapping(head.as_ptr(), raw_arr_ptr, N) };
            self.inner.head = self.inner.to_physical_idx(N);
            self.inner.len -= N;
            // SAFETY: We initialized the entire array with items from `head`
            return Ok(unsafe { raw_arr.transpose().assume_init() });
        }

        // SAFETY: Same argument as above.
        unsafe { ptr::copy_nonoverlapping(head.as_ptr(), raw_arr_ptr, head.len()) };
        let remaining = N - head.len();

        if tail.len() >= remaining {
            // SAFETY: Same argument as above.
            unsafe {
                ptr::copy_nonoverlapping(tail.as_ptr(), raw_arr_ptr.add(head.len()), remaining)
            };
            self.inner.head = self.inner.to_physical_idx(N);
            self.inner.len -= N;
            // SAFETY: We initialized the entire array with items from `head` and `tail`
            Ok(unsafe { raw_arr.transpose().assume_init() })
        } else {
            // SAFETY: Same argument as above.
            unsafe {
                ptr::copy_nonoverlapping(tail.as_ptr(), raw_arr_ptr.add(head.len()), tail.len())
            };
            let init = head.len() + tail.len();
            // We completely drained all the deques elements.
            self.inner.head = 0;
            self.inner.len = 0;
            // SAFETY: We copied all elements from both slices to the beginning of the array, so
            // the given range is initialized.
            Err(unsafe { core::array::IntoIter::new_unchecked(raw_arr, 0..init) })
        }
    }
}

impl<T, const N: usize> DoubleEndedIterator for IntoIter<T, N> {
    #[inline]
    fn next_back(&mut self) -> Option<T> {
        self.inner.pop_back()
    }

    #[inline]
    fn advance_back_by(&mut self, n: usize) -> Result<(), core::num::NonZero<usize>> {
        let len = self.inner.len;
        let rem = if len < n {
            self.inner.clear();
            n - len
        } else {
            self.inner.truncate(len - n);
            0
        };
        core::num::NonZero::new(rem).map_or(Ok(()), Err)
    }

    fn try_rfold<B, F, R>(&mut self, mut init: B, mut f: F) -> R
    where
        F: FnMut(B, Self::Item) -> R,
        R: core::ops::Try<Output = B>,
    {
        struct Guard<'a, T, const N: usize> {
            deque: &'a mut SmallDeque<T, N>,
            // `consumed <= deque.len` always holds.
            consumed: usize,
        }

        impl<T, const N: usize> Drop for Guard<'_, T, N> {
            fn drop(&mut self) {
                self.deque.len -= self.consumed;
            }
        }

        let mut guard = Guard {
            deque: &mut self.inner,
            consumed: 0,
        };

        let (head, tail) = guard.deque.as_slices();

        init = tail
            .iter()
            .map(|elem| {
                guard.consumed += 1;
                // SAFETY: See `try_fold`'s safety comment.
                unsafe { ptr::read(elem) }
            })
            .try_rfold(init, &mut f)?;

        head.iter()
            .map(|elem| {
                guard.consumed += 1;
                // SAFETY: Same as above.
                unsafe { ptr::read(elem) }
            })
            .try_rfold(init, &mut f)
    }

    #[inline]
    fn rfold<B, F>(mut self, init: B, mut f: F) -> B
    where
        F: FnMut(B, Self::Item) -> B,
    {
        match self.try_rfold(init, |b, item| Ok::<B, !>(f(b, item))) {
            Ok(b) => b,
            Err(e) => match e {},
        }
    }
}

impl<T, const N: usize> ExactSizeIterator for IntoIter<T, N> {
    #[inline]
    fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }
}

impl<T, const N: usize> core::iter::FusedIterator for IntoIter<T, N> {}

unsafe impl<T, const N: usize> core::iter::TrustedLen for IntoIter<T, N> {}

/// A draining iterator over the elements of a `SmallDeque`.
///
/// This `struct` is created by the [`drain`] method on [`SmallDeque`]. See its
/// documentation for more.
///
/// [`drain`]: SmallDeque::drain
pub struct Drain<'a, T: 'a, const N: usize> {
    // We can't just use a &mut SmallDeque<T, N>, as that would make Drain invariant over T
    // and we want it to be covariant instead
    deque: NonNull<SmallDeque<T, N>>,
    // drain_start is stored in deque.len
    drain_len: usize,
    // index into the logical array, not the physical one (always lies in [0..deque.len))
    idx: usize,
    // number of elements remaining after dropping the drain
    new_len: usize,
    remaining: usize,
    // Needed to make Drain covariant over T
    _marker: core::marker::PhantomData<&'a T>,
}

impl<'a, T, const N: usize> Drain<'a, T, N> {
    pub(super) unsafe fn new(
        deque: &'a mut SmallDeque<T, N>,
        drain_start: usize,
        drain_len: usize,
    ) -> Self {
        let orig_len = core::mem::replace(&mut deque.len, drain_start);
        let new_len = orig_len - drain_len;
        Drain {
            deque: NonNull::from(deque),
            drain_len,
            idx: drain_start,
            new_len,
            remaining: drain_len,
            _marker: core::marker::PhantomData,
        }
    }

    // Only returns pointers to the slices, as that's all we need
    // to drop them. May only be called if `self.remaining != 0`.
    unsafe fn as_slices(&mut self) -> (*mut [T], *mut [T]) {
        unsafe {
            let deque = self.deque.as_mut();

            // We know that `self.idx + self.remaining <= deque.len <= usize::MAX`, so this won't overflow.
            let logical_remaining_range = self.idx..self.idx + self.remaining;

            // SAFETY: `logical_remaining_range` represents the
            // range into the logical buffer of elements that
            // haven't been drained yet, so they're all initialized,
            // and `slice::range(start..end, end) == start..end`,
            // so the preconditions for `slice_ranges` are met.
            let (a_range, b_range) =
                deque.slice_ranges(logical_remaining_range.clone(), logical_remaining_range.end);
            (deque.buffer_range_mut(a_range), deque.buffer_range_mut(b_range))
        }
    }
}

impl<T: fmt::Debug, const N: usize> fmt::Debug for Drain<'_, T, N> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Drain")
            .field(&self.drain_len)
            .field(&self.idx)
            .field(&self.new_len)
            .field(&self.remaining)
            .finish()
    }
}

unsafe impl<T: Sync, const N: usize> Sync for Drain<'_, T, N> {}
unsafe impl<T: Send, const N: usize> Send for Drain<'_, T, N> {}

impl<T, const N: usize> Drop for Drain<'_, T, N> {
    fn drop(&mut self) {
        struct DropGuard<'r, 'a, T, const N: usize>(&'r mut Drain<'a, T, N>);

        let guard = DropGuard(self);

        if core::mem::needs_drop::<T>() && guard.0.remaining != 0 {
            unsafe {
                // SAFETY: We just checked that `self.remaining != 0`.
                let (front, back) = guard.0.as_slices();
                // since idx is a logical index, we don't need to worry about wrapping.
                guard.0.idx += front.len();
                guard.0.remaining -= front.len();
                ptr::drop_in_place(front);
                guard.0.remaining = 0;
                ptr::drop_in_place(back);
            }
        }

        // Dropping `guard` handles moving the remaining elements into place.
        impl<T, const N: usize> Drop for DropGuard<'_, '_, T, N> {
            #[inline]
            fn drop(&mut self) {
                if core::mem::needs_drop::<T>() && self.0.remaining != 0 {
                    unsafe {
                        // SAFETY: We just checked that `self.remaining != 0`.
                        let (front, back) = self.0.as_slices();
                        ptr::drop_in_place(front);
                        ptr::drop_in_place(back);
                    }
                }

                let source_deque = unsafe { self.0.deque.as_mut() };

                let drain_len = self.0.drain_len;
                let new_len = self.0.new_len;

                if T::IS_ZST {
                    // no need to copy around any memory if T is a ZST
                    source_deque.len = new_len;
                    return;
                }

                let head_len = source_deque.len; // #elements in front of the drain
                let tail_len = new_len - head_len; // #elements behind the drain

                // Next, we will fill the hole left by the drain with as few writes as possible.
                // The code below handles the following control flow and reduces the amount of
                // branches under the assumption that `head_len == 0 || tail_len == 0`, i.e.
                // draining at the front or at the back of the dequeue is especially common.
                //
                // H = "head index" = `deque.head`
                // h = elements in front of the drain
                // d = elements in the drain
                // t = elements behind the drain
                //
                // Note that the buffer may wrap at any point and the wrapping is handled by
                // `wrap_copy` and `to_physical_idx`.
                //
                // Case 1: if `head_len == 0 && tail_len == 0`
                // Everything was drained, reset the head index back to 0.
                //             H
                // [ . . . . . d d d d . . . . . ]
                //   H
                // [ . . . . . . . . . . . . . . ]
                //
                // Case 2: else if `tail_len == 0`
                // Don't move data or the head index.
                //         H
                // [ . . . h h h h d d d d . . . ]
                //         H
                // [ . . . h h h h . . . . . . . ]
                //
                // Case 3: else if `head_len == 0`
                // Don't move data, but move the head index.
                //         H
                // [ . . . d d d d t t t t . . . ]
                //                 H
                // [ . . . . . . . t t t t . . . ]
                //
                // Case 4: else if `tail_len <= head_len`
                // Move data, but not the head index.
                //       H
                // [ . . h h h h d d d d t t . . ]
                //       H
                // [ . . h h h h t t . . . . . . ]
                //
                // Case 5: else
                // Move data and the head index.
                //       H
                // [ . . h h d d d d t t t t . . ]
                //               H
                // [ . . . . . . h h t t t t . . ]

                // When draining at the front (`.drain(..n)`) or at the back (`.drain(n..)`),
                // we don't need to copy any data. The number of elements copied would be 0.
                if head_len != 0 && tail_len != 0 {
                    join_head_and_tail_wrapping(source_deque, drain_len, head_len, tail_len);
                    // Marking this function as cold helps LLVM to eliminate it entirely if
                    // this branch is never taken.
                    // We use `#[cold]` instead of `#[inline(never)]`, because inlining this
                    // function into the general case (`.drain(n..m)`) is fine.
                    // See `tests/codegen/vecdeque-drain.rs` for a test.
                    #[cold]
                    fn join_head_and_tail_wrapping<T, const N: usize>(
                        source_deque: &mut SmallDeque<T, N>,
                        drain_len: usize,
                        head_len: usize,
                        tail_len: usize,
                    ) {
                        // Pick whether to move the head or the tail here.
                        let (src, dst, len);
                        if head_len < tail_len {
                            src = source_deque.head;
                            dst = source_deque.to_physical_idx(drain_len);
                            len = head_len;
                        } else {
                            src = source_deque.to_physical_idx(head_len + drain_len);
                            dst = source_deque.to_physical_idx(head_len);
                            len = tail_len;
                        };

                        unsafe {
                            source_deque.wrap_copy(src, dst, len);
                        }
                    }
                }

                if new_len == 0 {
                    // Special case: If the entire dequeue was drained, reset the head back to 0,
                    // like `.clear()` does.
                    source_deque.head = 0;
                } else if head_len < tail_len {
                    // If we moved the head above, then we need to adjust the head index here.
                    source_deque.head = source_deque.to_physical_idx(drain_len);
                }
                source_deque.len = new_len;
            }
        }
    }
}

impl<T, const N: usize> Iterator for Drain<'_, T, N> {
    type Item = T;

    #[inline]
    fn next(&mut self) -> Option<T> {
        if self.remaining == 0 {
            return None;
        }
        let wrapped_idx = unsafe { self.deque.as_ref().to_physical_idx(self.idx) };
        self.idx += 1;
        self.remaining -= 1;
        Some(unsafe { self.deque.as_mut().buffer_read(wrapped_idx) })
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        let len = self.remaining;
        (len, Some(len))
    }
}

impl<T, const N: usize> DoubleEndedIterator for Drain<'_, T, N> {
    #[inline]
    fn next_back(&mut self) -> Option<T> {
        if self.remaining == 0 {
            return None;
        }
        self.remaining -= 1;
        let wrapped_idx = unsafe { self.deque.as_ref().to_physical_idx(self.idx + self.remaining) };
        Some(unsafe { self.deque.as_mut().buffer_read(wrapped_idx) })
    }
}

impl<T, const N: usize> ExactSizeIterator for Drain<'_, T, N> {}

impl<T, const N: usize> core::iter::FusedIterator for Drain<'_, T, N> {}

/// Specialization trait used for `SmallDeque::from_iter`
trait SpecFromIter<T, I> {
    fn spec_from_iter(iter: I) -> Self;
}

impl<T, I, const N: usize> SpecFromIter<T, I> for SmallDeque<T, N>
where
    I: Iterator<Item = T>,
{
    default fn spec_from_iter(iterator: I) -> Self {
        // Since converting is O(1) now, just re-use the `SmallVec` logic for
        // anything where we can't do something extra-special for `SmallDeque`,
        // especially as that could save us some monomorphization work
        // if one uses the same iterators (like slice ones) with both.
        SmallVec::from_iter(iterator).into()
    }
}

impl<T, const N: usize> SpecFromIter<T, IntoIter<T, N>> for SmallDeque<T, N> {
    #[inline]
    fn spec_from_iter(iterator: IntoIter<T, N>) -> Self {
        iterator.into_smalldeque()
    }
}

// Specialization trait used for SmallDeque::extend
trait SpecExtend<T, I> {
    fn spec_extend(&mut self, iter: I);
}

impl<T, I, const N: usize> SpecExtend<T, I> for SmallDeque<T, N>
where
    I: Iterator<Item = T>,
{
    default fn spec_extend(&mut self, mut iter: I) {
        // This function should be the moral equivalent of:
        //
        // for item in iter {
        //     self.push_back(item);
        // }

        // May only be called if `deque.len() < deque.capacity()`
        unsafe fn push_unchecked<T, const N: usize>(deque: &mut SmallDeque<T, N>, element: T) {
            // SAFETY: Because of the precondition, it's guaranteed that there is space
            // in the logical array after the last element.
            unsafe { deque.buffer_write(deque.to_physical_idx(deque.len), element) };
            // This can't overflow because `deque.len() < deque.capacity() <= usize::MAX`.
            deque.len += 1;
        }

        while let Some(element) = iter.next() {
            let (lower, _) = iter.size_hint();
            self.reserve(lower.saturating_add(1));

            // SAFETY: We just reserved space for at least one element.
            unsafe { push_unchecked(self, element) };

            // Inner loop to avoid repeatedly calling `reserve`.
            while self.len < self.capacity() {
                let Some(element) = iter.next() else {
                    return;
                };
                // SAFETY: The loop condition guarantees that `self.len() < self.capacity()`.
                unsafe { push_unchecked(self, element) };
            }
        }
    }
}

impl<T, I, const N: usize> SpecExtend<T, I> for SmallDeque<T, N>
where
    I: core::iter::TrustedLen<Item = T>,
{
    default fn spec_extend(&mut self, iter: I) {
        // This is the case for a TrustedLen iterator.
        let (low, high) = iter.size_hint();
        if let Some(additional) = high {
            debug_assert_eq!(
                low,
                additional,
                "TrustedLen iterator's size hint is not exact: {:?}",
                (low, high)
            );
            self.reserve(additional);

            let written = unsafe {
                self.write_iter_wrapping(self.to_physical_idx(self.len), iter, additional)
            };

            debug_assert_eq!(
                additional, written,
                "The number of items written to SmallDeque doesn't match the TrustedLen size hint"
            );
        } else {
            // Per TrustedLen contract a `None` upper bound means that the iterator length
            // truly exceeds usize::MAX, which would eventually lead to a capacity overflow anyway.
            // Since the other branch already panics eagerly (via `reserve()`) we do the same here.
            // This avoids additional codegen for a fallback code path which would eventually
            // panic anyway.
            panic!("capacity overflow");
        }
    }
}

impl<'a, T: 'a, I, const N: usize> SpecExtend<&'a T, I> for SmallDeque<T, N>
where
    I: Iterator<Item = &'a T>,
    T: Copy,
{
    default fn spec_extend(&mut self, iterator: I) {
        self.spec_extend(iterator.copied())
    }
}

impl<'a, T: 'a, const N: usize> SpecExtend<&'a T, core::slice::Iter<'a, T>> for SmallDeque<T, N>
where
    T: Copy,
{
    fn spec_extend(&mut self, iterator: core::slice::Iter<'a, T>) {
        let slice = iterator.as_slice();
        self.reserve(slice.len());

        unsafe {
            self.copy_slice(self.to_physical_idx(self.len), slice);
            self.len += slice.len();
        }
    }
}

#[cfg(test)]
mod tests {
    use core::iter::TrustedLen;

    use smallvec::SmallVec;

    use super::*;

    #[test]
    fn test_swap_front_back_remove() {
        fn test(back: bool) {
            // This test checks that every single combination of tail position and length is tested.
            // Capacity 15 should be large enough to cover every case.
            let mut tester = SmallDeque::<_, 16>::with_capacity(15);
            let usable_cap = tester.capacity();
            let final_len = usable_cap / 2;

            for len in 0..final_len {
                let expected: SmallDeque<_, 16> = if back {
                    (0..len).collect()
                } else {
                    (0..len).rev().collect()
                };
                for head_pos in 0..usable_cap {
                    tester.head = head_pos;
                    tester.len = 0;
                    if back {
                        for i in 0..len * 2 {
                            tester.push_front(i);
                        }
                        for i in 0..len {
                            assert_eq!(tester.swap_remove_back(i), Some(len * 2 - 1 - i));
                        }
                    } else {
                        for i in 0..len * 2 {
                            tester.push_back(i);
                        }
                        for i in 0..len {
                            let idx = tester.len() - 1 - i;
                            assert_eq!(tester.swap_remove_front(idx), Some(len * 2 - 1 - i));
                        }
                    }
                    assert!(tester.head <= tester.capacity());
                    assert!(tester.len <= tester.capacity());
                    assert_eq!(tester, expected);
                }
            }
        }
        test(true);
        test(false);
    }

    #[test]
    fn test_insert() {
        // This test checks that every single combination of tail position, length, and
        // insertion position is tested. Capacity 15 should be large enough to cover every case.

        let mut tester = SmallDeque::<_, 16>::with_capacity(15);
        // can't guarantee we got 15, so have to get what we got.
        // 15 would be great, but we will definitely get 2^k - 1, for k >= 4, or else
        // this test isn't covering what it wants to
        let cap = tester.capacity();

        // len is the length *after* insertion
        let minlen = if cfg!(miri) { cap - 1 } else { 1 }; // Miri is too slow
        for len in minlen..cap {
            // 0, 1, 2, .., len - 1
            let expected = (0..).take(len).collect::<SmallDeque<_, 16>>();
            for head_pos in 0..cap {
                for to_insert in 0..len {
                    tester.head = head_pos;
                    tester.len = 0;
                    for i in 0..len {
                        if i != to_insert {
                            tester.push_back(i);
                        }
                    }
                    tester.insert(to_insert, to_insert);
                    assert!(tester.head <= tester.capacity());
                    assert!(tester.len <= tester.capacity());
                    assert_eq!(tester, expected);
                }
            }
        }
    }

    #[test]
    fn test_get() {
        let mut tester = SmallDeque::<_, 16>::new();
        tester.push_back(1);
        tester.push_back(2);
        tester.push_back(3);

        assert_eq!(tester.len(), 3);

        assert_eq!(tester.get(1), Some(&2));
        assert_eq!(tester.get(2), Some(&3));
        assert_eq!(tester.get(0), Some(&1));
        assert_eq!(tester.get(3), None);

        tester.remove(0);

        assert_eq!(tester.len(), 2);
        assert_eq!(tester.get(0), Some(&2));
        assert_eq!(tester.get(1), Some(&3));
        assert_eq!(tester.get(2), None);
    }

    #[test]
    fn test_get_mut() {
        let mut tester = SmallDeque::<_, 16>::new();
        tester.push_back(1);
        tester.push_back(2);
        tester.push_back(3);

        assert_eq!(tester.len(), 3);

        if let Some(elem) = tester.get_mut(0) {
            assert_eq!(*elem, 1);
            *elem = 10;
        }

        if let Some(elem) = tester.get_mut(2) {
            assert_eq!(*elem, 3);
            *elem = 30;
        }

        assert_eq!(tester.get(0), Some(&10));
        assert_eq!(tester.get(2), Some(&30));
        assert_eq!(tester.get_mut(3), None);

        tester.remove(2);

        assert_eq!(tester.len(), 2);
        assert_eq!(tester.get(0), Some(&10));
        assert_eq!(tester.get(1), Some(&2));
        assert_eq!(tester.get(2), None);
    }

    #[test]
    fn test_swap() {
        let mut tester = SmallDeque::<_, 3>::new();
        tester.push_back(1);
        tester.push_back(2);
        tester.push_back(3);

        assert_eq!(tester, [1, 2, 3]);

        tester.swap(0, 0);
        assert_eq!(tester, [1, 2, 3]);
        tester.swap(0, 1);
        assert_eq!(tester, [2, 1, 3]);
        tester.swap(2, 1);
        assert_eq!(tester, [2, 3, 1]);
        tester.swap(1, 2);
        assert_eq!(tester, [2, 1, 3]);
        tester.swap(0, 2);
        assert_eq!(tester, [3, 1, 2]);
        tester.swap(2, 2);
        assert_eq!(tester, [3, 1, 2]);
    }

    #[test]
    #[should_panic = "assertion failed: j < self.len()"]
    fn test_swap_panic() {
        let mut tester = SmallDeque::<_>::new();
        tester.push_back(1);
        tester.push_back(2);
        tester.push_back(3);
        tester.swap(2, 3);
    }

    #[test]
    fn test_reserve_exact() {
        let mut tester: SmallDeque<i32, 1> = SmallDeque::with_capacity(1);
        assert_eq!(tester.capacity(), 1);
        tester.reserve_exact(50);
        assert_eq!(tester.capacity(), 50);
        tester.reserve_exact(40);
        // reserving won't shrink the buffer
        assert_eq!(tester.capacity(), 50);
        tester.reserve_exact(200);
        assert_eq!(tester.capacity(), 200);
    }

    #[test]
    #[should_panic = "capacity overflow"]
    fn test_reserve_exact_panic() {
        let mut tester: SmallDeque<i32> = SmallDeque::new();
        tester.reserve_exact(usize::MAX);
    }

    #[test]
    fn test_contains() {
        let mut tester = SmallDeque::<_>::new();
        tester.push_back(1);
        tester.push_back(2);
        tester.push_back(3);

        assert!(tester.contains(&1));
        assert!(tester.contains(&3));
        assert!(!tester.contains(&0));
        assert!(!tester.contains(&4));
        tester.remove(0);
        assert!(!tester.contains(&1));
        assert!(tester.contains(&2));
        assert!(tester.contains(&3));
    }

    #[test]
    fn test_rotate_left_right() {
        let mut tester: SmallDeque<_> = (1..=10).collect();
        tester.reserve(1);

        assert_eq!(tester.len(), 10);

        tester.rotate_left(0);
        assert_eq!(tester, [1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);

        tester.rotate_right(0);
        assert_eq!(tester, [1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);

        tester.rotate_left(3);
        assert_eq!(tester, [4, 5, 6, 7, 8, 9, 10, 1, 2, 3]);

        tester.rotate_right(5);
        assert_eq!(tester, [9, 10, 1, 2, 3, 4, 5, 6, 7, 8]);

        tester.rotate_left(tester.len());
        assert_eq!(tester, [9, 10, 1, 2, 3, 4, 5, 6, 7, 8]);

        tester.rotate_right(tester.len());
        assert_eq!(tester, [9, 10, 1, 2, 3, 4, 5, 6, 7, 8]);

        tester.rotate_left(1);
        assert_eq!(tester, [10, 1, 2, 3, 4, 5, 6, 7, 8, 9]);
    }

    #[test]
    #[should_panic = "assertion failed: n <= self.len()"]
    fn test_rotate_left_panic() {
        let mut tester: SmallDeque<_> = (1..=10).collect();
        tester.rotate_left(tester.len() + 1);
    }

    #[test]
    #[should_panic = "assertion failed: n <= self.len()"]
    fn test_rotate_right_panic() {
        let mut tester: SmallDeque<_> = (1..=10).collect();
        tester.rotate_right(tester.len() + 1);
    }

    #[test]
    fn test_binary_search() {
        // If the givin SmallDeque is not sorted, the returned result is unspecified and meaningless,
        // as this method performs a binary search.

        let tester: SmallDeque<_, 11> = [0, 1, 1, 2, 3, 5, 8, 13, 21, 34, 55].into();

        assert_eq!(tester.binary_search(&0), Ok(0));
        assert_eq!(tester.binary_search(&5), Ok(5));
        assert_eq!(tester.binary_search(&55), Ok(10));
        assert_eq!(tester.binary_search(&4), Err(5));
        assert_eq!(tester.binary_search(&-1), Err(0));
        assert!(matches!(tester.binary_search(&1), Ok(1..=2)));

        let tester: SmallDeque<_, 14> = [1, 2, 2, 2, 2, 3, 3, 3, 3, 3, 3, 3, 3, 3].into();
        assert_eq!(tester.binary_search(&1), Ok(0));
        assert!(matches!(tester.binary_search(&2), Ok(1..=4)));
        assert!(matches!(tester.binary_search(&3), Ok(5..=13)));
        assert_eq!(tester.binary_search(&-2), Err(0));
        assert_eq!(tester.binary_search(&0), Err(0));
        assert_eq!(tester.binary_search(&4), Err(14));
        assert_eq!(tester.binary_search(&5), Err(14));
    }

    #[test]
    fn test_binary_search_by() {
        // If the givin SmallDeque is not sorted, the returned result is unspecified and meaningless,
        // as this method performs a binary search.

        let tester: SmallDeque<isize, 11> = [0, 1, 1, 2, 3, 5, 8, 13, 21, 34, 55].into();

        assert_eq!(tester.binary_search_by(|x| x.cmp(&0)), Ok(0));
        assert_eq!(tester.binary_search_by(|x| x.cmp(&5)), Ok(5));
        assert_eq!(tester.binary_search_by(|x| x.cmp(&55)), Ok(10));
        assert_eq!(tester.binary_search_by(|x| x.cmp(&4)), Err(5));
        assert_eq!(tester.binary_search_by(|x| x.cmp(&-1)), Err(0));
        assert!(matches!(tester.binary_search_by(|x| x.cmp(&1)), Ok(1..=2)));
    }

    #[test]
    fn test_binary_search_key() {
        // If the givin SmallDeque is not sorted, the returned result is unspecified and meaningless,
        // as this method performs a binary search.

        let tester: SmallDeque<_, 13> = [
            (-1, 0),
            (2, 10),
            (6, 5),
            (7, 1),
            (8, 10),
            (10, 2),
            (20, 3),
            (24, 5),
            (25, 18),
            (28, 13),
            (31, 21),
            (32, 4),
            (54, 25),
        ]
        .into();

        assert_eq!(tester.binary_search_by_key(&-1, |&(a, _b)| a), Ok(0));
        assert_eq!(tester.binary_search_by_key(&8, |&(a, _b)| a), Ok(4));
        assert_eq!(tester.binary_search_by_key(&25, |&(a, _b)| a), Ok(8));
        assert_eq!(tester.binary_search_by_key(&54, |&(a, _b)| a), Ok(12));
        assert_eq!(tester.binary_search_by_key(&-2, |&(a, _b)| a), Err(0));
        assert_eq!(tester.binary_search_by_key(&1, |&(a, _b)| a), Err(1));
        assert_eq!(tester.binary_search_by_key(&4, |&(a, _b)| a), Err(2));
        assert_eq!(tester.binary_search_by_key(&13, |&(a, _b)| a), Err(6));
        assert_eq!(tester.binary_search_by_key(&55, |&(a, _b)| a), Err(13));
        assert_eq!(tester.binary_search_by_key(&100, |&(a, _b)| a), Err(13));

        let tester: SmallDeque<_, 13> = [
            (0, 0),
            (2, 1),
            (6, 1),
            (5, 1),
            (3, 1),
            (1, 2),
            (2, 3),
            (4, 5),
            (5, 8),
            (8, 13),
            (1, 21),
            (2, 34),
            (4, 55),
        ]
        .into();

        assert_eq!(tester.binary_search_by_key(&0, |&(_a, b)| b), Ok(0));
        assert!(matches!(tester.binary_search_by_key(&1, |&(_a, b)| b), Ok(1..=4)));
        assert_eq!(tester.binary_search_by_key(&8, |&(_a, b)| b), Ok(8));
        assert_eq!(tester.binary_search_by_key(&13, |&(_a, b)| b), Ok(9));
        assert_eq!(tester.binary_search_by_key(&55, |&(_a, b)| b), Ok(12));
        assert_eq!(tester.binary_search_by_key(&-1, |&(_a, b)| b), Err(0));
        assert_eq!(tester.binary_search_by_key(&4, |&(_a, b)| b), Err(7));
        assert_eq!(tester.binary_search_by_key(&56, |&(_a, b)| b), Err(13));
        assert_eq!(tester.binary_search_by_key(&100, |&(_a, b)| b), Err(13));
    }

    #[test]
    fn make_contiguous_big_head() {
        let mut tester = SmallDeque::<_>::with_capacity(15);

        for i in 0..3 {
            tester.push_back(i);
        }

        for i in 3..10 {
            tester.push_front(i);
        }

        // 012......9876543
        assert_eq!(tester.capacity(), 15);
        assert_eq!((&[9, 8, 7, 6, 5, 4, 3] as &[_], &[0, 1, 2] as &[_]), tester.as_slices());

        let expected_start = tester.as_slices().1.len();
        tester.make_contiguous();
        assert_eq!(tester.head, expected_start);
        assert_eq!((&[9, 8, 7, 6, 5, 4, 3, 0, 1, 2] as &[_], &[] as &[_]), tester.as_slices());
    }

    #[test]
    fn make_contiguous_big_tail() {
        let mut tester = SmallDeque::<_>::with_capacity(15);

        for i in 0..8 {
            tester.push_back(i);
        }

        for i in 8..10 {
            tester.push_front(i);
        }

        // 01234567......98
        let expected_start = 0;
        tester.make_contiguous();
        assert_eq!(tester.head, expected_start);
        assert_eq!((&[9, 8, 0, 1, 2, 3, 4, 5, 6, 7] as &[_], &[] as &[_]), tester.as_slices());
    }

    #[test]
    fn make_contiguous_small_free() {
        let mut tester = SmallDeque::<_>::with_capacity(16);

        for i in b'A'..b'I' {
            tester.push_back(i as char);
        }

        for i in b'I'..b'N' {
            tester.push_front(i as char);
        }

        assert_eq!(tester, ['M', 'L', 'K', 'J', 'I', 'A', 'B', 'C', 'D', 'E', 'F', 'G', 'H']);

        // ABCDEFGH...MLKJI
        let expected_start = 0;
        tester.make_contiguous();
        assert_eq!(tester.head, expected_start);
        assert_eq!(
            (
                &['M', 'L', 'K', 'J', 'I', 'A', 'B', 'C', 'D', 'E', 'F', 'G', 'H'] as &[_],
                &[] as &[_]
            ),
            tester.as_slices()
        );

        tester.clear();
        for i in b'I'..b'N' {
            tester.push_back(i as char);
        }

        for i in b'A'..b'I' {
            tester.push_front(i as char);
        }

        // IJKLM...HGFEDCBA
        let expected_start = 3;
        tester.make_contiguous();
        assert_eq!(tester.head, expected_start);
        assert_eq!(
            (
                &['H', 'G', 'F', 'E', 'D', 'C', 'B', 'A', 'I', 'J', 'K', 'L', 'M'] as &[_],
                &[] as &[_]
            ),
            tester.as_slices()
        );
    }

    #[test]
    fn make_contiguous_head_to_end() {
        let mut tester = SmallDeque::<_>::with_capacity(16);

        for i in b'A'..b'L' {
            tester.push_back(i as char);
        }

        for i in b'L'..b'Q' {
            tester.push_front(i as char);
        }

        assert_eq!(
            tester,
            ['P', 'O', 'N', 'M', 'L', 'A', 'B', 'C', 'D', 'E', 'F', 'G', 'H', 'I', 'J', 'K']
        );

        // ABCDEFGHIJKPONML
        let expected_start = 0;
        tester.make_contiguous();
        assert_eq!(tester.head, expected_start);
        assert_eq!(
            (
                &['P', 'O', 'N', 'M', 'L', 'A', 'B', 'C', 'D', 'E', 'F', 'G', 'H', 'I', 'J', 'K']
                    as &[_],
                &[] as &[_]
            ),
            tester.as_slices()
        );

        tester.clear();
        for i in b'L'..b'Q' {
            tester.push_back(i as char);
        }

        for i in b'A'..b'L' {
            tester.push_front(i as char);
        }

        // LMNOPKJIHGFEDCBA
        let expected_start = 0;
        tester.make_contiguous();
        assert_eq!(tester.head, expected_start);
        assert_eq!(
            (
                &['K', 'J', 'I', 'H', 'G', 'F', 'E', 'D', 'C', 'B', 'A', 'L', 'M', 'N', 'O', 'P']
                    as &[_],
                &[] as &[_]
            ),
            tester.as_slices()
        );
    }

    #[test]
    fn make_contiguous_head_to_end_2() {
        // Another test case for #79808, taken from #80293.

        let mut dq = SmallDeque::<_>::from_iter(0..6);
        dq.pop_front();
        dq.pop_front();
        dq.push_back(6);
        dq.push_back(7);
        dq.push_back(8);
        dq.make_contiguous();
        let collected: Vec<_> = dq.iter().copied().collect();
        assert_eq!(dq.as_slices(), (&collected[..], &[] as &[_]));
    }

    #[test]
    fn test_remove() {
        // This test checks that every single combination of tail position, length, and
        // removal position is tested. Capacity 15 should be large enough to cover every case.

        let mut tester = SmallDeque::<_>::with_capacity(15);
        // can't guarantee we got 15, so have to get what we got.
        // 15 would be great, but we will definitely get 2^k - 1, for k >= 4, or else
        // this test isn't covering what it wants to
        let cap = tester.capacity();

        // len is the length *after* removal
        let minlen = if cfg!(miri) { cap - 2 } else { 0 }; // Miri is too slow
        for len in minlen..cap - 1 {
            // 0, 1, 2, .., len - 1
            let expected = (0..).take(len).collect::<SmallDeque<_>>();
            for head_pos in 0..cap {
                for to_remove in 0..=len {
                    tester.head = head_pos;
                    tester.len = 0;
                    for i in 0..len {
                        if i == to_remove {
                            tester.push_back(1234);
                        }
                        tester.push_back(i);
                    }
                    if to_remove == len {
                        tester.push_back(1234);
                    }
                    tester.remove(to_remove);
                    assert!(tester.head <= tester.capacity());
                    assert!(tester.len <= tester.capacity());
                    assert_eq!(tester, expected);
                }
            }
        }
    }

    #[test]
    fn test_range() {
        let mut tester: SmallDeque<usize> = SmallDeque::<_>::with_capacity(7);

        let cap = tester.capacity();
        let minlen = if cfg!(miri) { cap - 1 } else { 0 }; // Miri is too slow
        for len in minlen..=cap {
            for head in 0..=cap {
                for start in 0..=len {
                    for end in start..=len {
                        tester.head = head;
                        tester.len = 0;
                        for i in 0..len {
                            tester.push_back(i);
                        }

                        // Check that we iterate over the correct values
                        let range: SmallDeque<_> = tester.range(start..end).copied().collect();
                        let expected: SmallDeque<_> = (start..end).collect();
                        assert_eq!(range, expected);
                    }
                }
            }
        }
    }

    #[test]
    fn test_range_mut() {
        let mut tester: SmallDeque<usize> = SmallDeque::with_capacity(7);

        let cap = tester.capacity();
        for len in 0..=cap {
            for head in 0..=cap {
                for start in 0..=len {
                    for end in start..=len {
                        tester.head = head;
                        tester.len = 0;
                        for i in 0..len {
                            tester.push_back(i);
                        }

                        let head_was = tester.head;
                        let len_was = tester.len;

                        // Check that we iterate over the correct values
                        let range: SmallDeque<_> =
                            tester.range_mut(start..end).map(|v| *v).collect();
                        let expected: SmallDeque<_> = (start..end).collect();
                        assert_eq!(range, expected);

                        // We shouldn't have changed the capacity or made the
                        // head or tail out of bounds
                        assert_eq!(tester.capacity(), cap);
                        assert_eq!(tester.head, head_was);
                        assert_eq!(tester.len, len_was);
                    }
                }
            }
        }
    }

    #[test]
    fn test_drain() {
        let mut tester: SmallDeque<usize> = SmallDeque::with_capacity(7);

        let cap = tester.capacity();
        for len in 0..=cap {
            for head in 0..cap {
                for drain_start in 0..=len {
                    for drain_end in drain_start..=len {
                        tester.head = head;
                        tester.len = 0;
                        for i in 0..len {
                            tester.push_back(i);
                        }

                        // Check that we drain the correct values
                        let drained: SmallDeque<_> = tester.drain(drain_start..drain_end).collect();
                        let drained_expected: SmallDeque<_> = (drain_start..drain_end).collect();
                        assert_eq!(drained, drained_expected);

                        // We shouldn't have changed the capacity or made the
                        // head or tail out of bounds
                        assert_eq!(tester.capacity(), cap);
                        assert!(tester.head <= tester.capacity());
                        assert!(tester.len <= tester.capacity());

                        // We should see the correct values in the SmallDeque
                        let expected: SmallDeque<_> =
                            (0..drain_start).chain(drain_end..len).collect();
                        assert_eq!(expected, tester);
                    }
                }
            }
        }
    }

    #[test]
    fn test_split_off() {
        // This test checks that every single combination of tail position, length, and
        // split position is tested. Capacity 15 should be large enough to cover every case.

        let mut tester = SmallDeque::with_capacity(15);
        // can't guarantee we got 15, so have to get what we got.
        // 15 would be great, but we will definitely get 2^k - 1, for k >= 4, or else
        // this test isn't covering what it wants to
        let cap = tester.capacity();

        // len is the length *before* splitting
        let minlen = if cfg!(miri) { cap - 1 } else { 0 }; // Miri is too slow
        for len in minlen..cap {
            // index to split at
            for at in 0..=len {
                // 0, 1, 2, .., at - 1 (may be empty)
                let expected_self = (0..).take(at).collect::<SmallDeque<_>>();
                // at, at + 1, .., len - 1 (may be empty)
                let expected_other = (at..).take(len - at).collect::<SmallDeque<_>>();

                for head_pos in 0..cap {
                    tester.head = head_pos;
                    tester.len = 0;
                    for i in 0..len {
                        tester.push_back(i);
                    }
                    let result = tester.split_off(at);
                    assert!(tester.head <= tester.capacity());
                    assert!(tester.len <= tester.capacity());
                    assert!(result.head <= result.capacity());
                    assert!(result.len <= result.capacity());
                    assert_eq!(tester, expected_self);
                    assert_eq!(result, expected_other);
                }
            }
        }
    }

    #[test]
    fn test_from_smallvec() {
        for cap in 0..35 {
            for len in 0..=cap {
                let mut vec = SmallVec::<[_; 16]>::with_capacity(cap);
                vec.extend(0..len);

                let vd = SmallDeque::from(vec.clone());
                assert_eq!(vd.len(), vec.len());
                assert!(vd.into_iter().eq(vec));
            }
        }
    }

    #[test]
    fn test_extend_basic() {
        test_extend_impl(false);
    }

    #[ignore = "trusted_len extension is broken, needs further analysis"]
    #[test]
    fn test_extend_trusted_len() {
        test_extend_impl(true);
    }

    fn test_extend_impl(trusted_len: bool) {
        struct SmallDequeTester {
            test: SmallDeque<usize>,
            expected: SmallDeque<usize>,
            trusted_len: bool,
        }

        impl SmallDequeTester {
            fn new(trusted_len: bool) -> Self {
                Self {
                    test: SmallDeque::new(),
                    expected: SmallDeque::new(),
                    trusted_len,
                }
            }

            fn test_extend<I>(&mut self, iter: I)
            where
                I: Iterator<Item = usize> + TrustedLen + Clone,
            {
                struct BasicIterator<I>(I);
                impl<I> Iterator for BasicIterator<I>
                where
                    I: Iterator<Item = usize>,
                {
                    type Item = usize;

                    fn next(&mut self) -> Option<Self::Item> {
                        self.0.next()
                    }
                }

                if self.trusted_len {
                    self.test.extend(iter.clone());
                } else {
                    self.test.extend(BasicIterator(iter.clone()));
                }

                for item in iter {
                    self.expected.push_back(item)
                }

                dbg!(&self.test, &self.expected);

                assert_eq!(self.test, self.expected);
            }

            fn drain<R: RangeBounds<usize> + Clone>(&mut self, range: R) {
                self.test.drain(range.clone());
                self.expected.drain(range);

                assert_eq!(self.test, self.expected);
            }

            fn clear(&mut self) {
                self.test.clear();
                self.expected.clear();
            }

            fn remaining_capacity(&self) -> usize {
                self.test.capacity() - self.test.len()
            }
        }

        let mut tester = SmallDequeTester::new(trusted_len);

        // Initial capacity
        tester.test_extend(0..tester.remaining_capacity());

        // Grow
        tester.test_extend(1024..2048);

        // Wrap around
        tester.drain(..128);

        tester.test_extend(0..tester.remaining_capacity());

        // Continue
        tester.drain(256..);
        tester.test_extend(4096..8196);

        tester.clear();

        // Start again
        tester.test_extend(0..32);
    }

    #[test]
    fn test_from_array() {
        fn test<const N: usize>() {
            let mut array: [usize; N] = [0; N];

            for (i, v) in array.iter_mut().enumerate() {
                *v = i;
            }

            let deq: SmallDeque<_, N> = array.into();

            for i in 0..N {
                assert_eq!(deq[i], i);
            }

            assert_eq!(deq.len(), N);
        }
        test::<0>();
        test::<1>();
        test::<2>();
        test::<32>();
        test::<35>();
    }

    #[test]
    fn test_smallvec_from_smalldeque() {
        fn create_vec_and_test_convert(capacity: usize, offset: usize, len: usize) {
            let mut vd = SmallDeque::<_, 16>::with_capacity(capacity);
            for _ in 0..offset {
                vd.push_back(0);
                vd.pop_front();
            }
            vd.extend(0..len);

            let vec: SmallVec<_> = SmallVec::from(vd.clone());
            assert_eq!(vec.len(), vd.len());
            assert!(vec.into_iter().eq(vd));
        }

        // Miri is too slow
        let max_pwr = if cfg!(miri) { 5 } else { 7 };

        for cap_pwr in 0..max_pwr {
            // Make capacity as a (2^x)-1, so that the ring size is 2^x
            let cap = (2i32.pow(cap_pwr) - 1) as usize;

            // In these cases there is enough free space to solve it with copies
            for len in 0..((cap + 1) / 2) {
                // Test contiguous cases
                for offset in 0..(cap - len) {
                    create_vec_and_test_convert(cap, offset, len)
                }

                // Test cases where block at end of buffer is bigger than block at start
                for offset in (cap - len)..(cap - (len / 2)) {
                    create_vec_and_test_convert(cap, offset, len)
                }

                // Test cases where block at start of buffer is bigger than block at end
                for offset in (cap - (len / 2))..cap {
                    create_vec_and_test_convert(cap, offset, len)
                }
            }

            // Now there's not (necessarily) space to straighten the ring with simple copies,
            // the ring will use swapping when:
            // (cap + 1 - offset) > (cap + 1 - len) && (len - (cap + 1 - offset)) > (cap + 1 - len))
            //  right block size  >   free space    &&      left block size       >    free space
            for len in ((cap + 1) / 2)..cap {
                // Test contiguous cases
                for offset in 0..(cap - len) {
                    create_vec_and_test_convert(cap, offset, len)
                }

                // Test cases where block at end of buffer is bigger than block at start
                for offset in (cap - len)..(cap - (len / 2)) {
                    create_vec_and_test_convert(cap, offset, len)
                }

                // Test cases where block at start of buffer is bigger than block at end
                for offset in (cap - (len / 2))..cap {
                    create_vec_and_test_convert(cap, offset, len)
                }
            }
        }
    }

    #[test]
    fn test_clone_from() {
        use smallvec::smallvec;

        let m = smallvec![1; 8];
        let n = smallvec![2; 12];
        let limit = if cfg!(miri) { 4 } else { 8 }; // Miri is too slow
        for pfv in 0..limit {
            for pfu in 0..limit {
                for longer in 0..2 {
                    let (vr, ur) = if longer == 0 { (&m, &n) } else { (&n, &m) };
                    let mut v = SmallDeque::<_>::from(vr.clone());
                    for _ in 0..pfv {
                        v.push_front(1);
                    }
                    let mut u = SmallDeque::<_>::from(ur.clone());
                    for _ in 0..pfu {
                        u.push_front(2);
                    }
                    v.clone_from(&u);
                    assert_eq!(&v, &u);
                }
            }
        }
    }

    #[test]
    fn test_vec_deque_truncate_drop() {
        static mut DROPS: u32 = 0;
        #[derive(Clone)]
        struct Elem(#[allow(dead_code)] i32);
        impl Drop for Elem {
            fn drop(&mut self) {
                unsafe {
                    DROPS += 1;
                }
            }
        }

        let v = vec![Elem(1), Elem(2), Elem(3), Elem(4), Elem(5)];
        for push_front in 0..=v.len() {
            let v = v.clone();
            let mut tester = SmallDeque::<_>::with_capacity(5);
            for (index, elem) in v.into_iter().enumerate() {
                if index < push_front {
                    tester.push_front(elem);
                } else {
                    tester.push_back(elem);
                }
            }
            assert_eq!(unsafe { DROPS }, 0);
            tester.truncate(3);
            assert_eq!(unsafe { DROPS }, 2);
            tester.truncate(0);
            assert_eq!(unsafe { DROPS }, 5);
            unsafe {
                DROPS = 0;
            }
        }
    }

    #[test]
    fn issue_53529() {
        let mut dst = SmallDeque::<_>::new();
        dst.push_front(Box::new(1));
        dst.push_front(Box::new(2));
        assert_eq!(*dst.pop_back().unwrap(), 1);

        let mut src = SmallDeque::<_>::new();
        src.push_front(Box::new(2));
        dst.append(&mut src);
        for a in dst {
            assert_eq!(*a, 2);
        }
    }

    #[test]
    fn issue_80303() {
        use core::{
            hash::{Hash, Hasher},
            iter,
            num::Wrapping,
        };

        // This is a valid, albeit rather bad hash function implementation.
        struct SimpleHasher(Wrapping<u64>);

        impl Hasher for SimpleHasher {
            fn finish(&self) -> u64 {
                self.0 .0
            }

            fn write(&mut self, bytes: &[u8]) {
                // This particular implementation hashes value 24 in addition to bytes.
                // Such an implementation is valid as Hasher only guarantees equivalence
                // for the exact same set of calls to its methods.
                for &v in iter::once(&24).chain(bytes) {
                    self.0 = Wrapping(31) * self.0 + Wrapping(u64::from(v));
                }
            }
        }

        fn hash_code(value: impl Hash) -> u64 {
            let mut hasher = SimpleHasher(Wrapping(1));
            value.hash(&mut hasher);
            hasher.finish()
        }

        // This creates two deques for which values returned by as_slices
        // method differ.
        let vda: SmallDeque<u8> = (0..10).collect();
        let mut vdb = SmallDeque::with_capacity(10);
        vdb.extend(5..10);
        (0..5).rev().for_each(|elem| vdb.push_front(elem));
        assert_ne!(vda.as_slices(), vdb.as_slices());
        assert_eq!(vda, vdb);
        assert_eq!(hash_code(vda), hash_code(vdb));
    }
}
