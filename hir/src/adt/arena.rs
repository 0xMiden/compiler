use alloc::{
    alloc::{self as sysalloc, Layout},
    boxed::Box,
    rc::Rc,
    vec::Vec,
};
use core::{
    cell::RefCell,
    mem::MaybeUninit,
    ptr::NonNull,
    sync::atomic::{AtomicUsize, Ordering},
};

use intrusive_collections::{intrusive_adapter, LinkedListLink};

use crate::adt::SizedTypeProperties;

/// A typed arena with the following properties:
///
/// * Amortized growth (i.e. memory is allocated in chunks, allocated as capacity runs out)
/// * Append-only (items can't be deleted from the arena)
/// * Pinned storage (items never move once stored in the arena)
/// * Thanks to the previous points, can be allocated from while holding references to items held in
///   the arena
/// * Can be sent between threads (though it is not `Sync`)
/// * Default instance allocates no memory, so is cheap to create
/// * Can be very efficiently extended from iterators, particularly for stdlib collections/types
/// * `Vec<T>` and `Box<[T]>` values can be used to extend the arena without any allocations or
///   copies - the arena takes direct ownership over their backing storage as chunks in the arena.
///
pub struct Arena<T> {
    chunks: RefCell<ChunkList<T>>,
    min_capacity: usize,
}

unsafe impl<T: Send> Send for Arena<T> {}

impl<T> Default for Arena<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> Arena<T> {
    const DEFAULT_CHUNK_SIZE: usize = 64;

    /// Create an empty arena with no allocated capacity
    pub fn new() -> Self {
        Self {
            chunks: Default::default(),
            min_capacity: Self::DEFAULT_CHUNK_SIZE,
        }
    }

    /// Allocates an arena with capacity for at least `capacity` items.
    ///
    /// The actual allocated capacity may be larger.
    pub fn with_capacity(capacity: usize) -> Self {
        let mut chunks = ChunkList::default();
        chunks.push_back(ChunkHeader::new(capacity));
        Self {
            chunks: RefCell::new(chunks),
            min_capacity: core::cmp::max(capacity, Self::DEFAULT_CHUNK_SIZE),
        }
    }

    /// Allocate `item` in the arena, returning a non-null pointer to the allocation.
    ///
    /// If `T` is a zero-sized type, this returns [NonNull::dangling], which is a well-aligned
    /// pointer, but not necessarily a valid one. As far as I can tell, it is technically allowed to
    /// construct a reference from that pointer when the type is zero-sized, as zero-sized types do
    /// not refer to any memory at all (thus the reference is meaningless). That said, you should
    /// probably _not_ rely on that.
    pub fn push(&self, item: T) -> NonNull<T> {
        if T::IS_ZST {
            return NonNull::dangling();
        }

        let mut chunks = self.chunks.borrow_mut();
        if chunks.back().get().is_none_or(|chunk| chunk.available_capacity() == 0) {
            chunks.push_back(ChunkHeader::new(self.min_capacity));
        }
        let chunk = unsafe { chunks.back().get().unwrap_unchecked() };
        chunk.alloc(item)
    }

    /// Get a pointer to the `index`th item stored in the arena, or `None` if the index is invalid.
    ///
    /// # Safety
    ///
    /// This function is unsafe for two reasons:
    ///
    /// * The caller is responsible for knowing the indices of items in the arena when using this.
    ///   This is not hard to do, but the second point below requires it.
    /// * The caller must ensure that, should any reference be created from the returned pointer,
    ///   that the aliasing rules of Rust are upheld, i.e. it is undefined behavior to create a
    ///   reference if there outstanding mutable references (and vice versa).
    pub unsafe fn get(&self, index: usize) -> Option<NonNull<T>> {
        let chunks = self.chunks.borrow();
        let mut cursor = chunks.front();
        let mut current_index = 0;
        while current_index <= index {
            let chunk = cursor.clone_pointer()?;
            cursor.move_next();
            let chunk_len = chunk.len();
            let next_index = current_index + chunk_len;
            if next_index > index {
                // We found our chunk
                let offset = index - current_index;
                return Some(unsafe { chunk.data().add(offset).cast() });
            } else {
                // Try the next one
                current_index = next_index;
            }
        }

        None
    }

    /// Allocates `items` in the arena contiguously, returning a non-null pointer to the allocation.
    ///
    /// NOTE: This may potentially waste capacity of the currently allocated chunk, if the given
    /// items do not fit in its available capacity, but this is considered a minor issue for now.
    pub fn extend<I>(&self, items: I) -> NonNull<[T]>
    where
        I: IntoIterator<Item = T>,
    {
        if T::IS_ZST {
            return NonNull::slice_from_raw_parts(NonNull::dangling(), 0);
        }

        let mut chunks = self.chunks.borrow_mut();
        items.extend_arena(&mut chunks)
    }
}

impl<T> FromIterator<T> for Arena<T> {
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self {
        let arena = Self::new();
        arena.extend(iter);
        arena
    }
}

impl<T> IntoIterator for Arena<T> {
    type IntoIter = IntoIter<T>;
    type Item = T;

    fn into_iter(mut self) -> Self::IntoIter {
        IntoIter {
            chunks: self.chunks.get_mut().take(),
            current_chunk: None,
            current_len: 0,
            current_index: 0,
        }
    }
}

#[doc(hidden)]
pub struct IntoIter<T> {
    chunks: ChunkList<T>,
    current_chunk: Option<Rc<ChunkHeader<T>>>,
    current_len: usize,
    current_index: usize,
}
impl<T> core::iter::FusedIterator for IntoIter<T> {}
impl<T> ExactSizeIterator for IntoIter<T> {
    fn len(&self) -> usize {
        let remaining_in_current = self.current_len - self.current_index;
        let remaining_in_rest =
            self.chunks.iter().map(|chunk| chunk.len.load(Ordering::Relaxed)).sum::<usize>();
        remaining_in_current + remaining_in_rest
    }
}
impl<T> Iterator for IntoIter<T> {
    type Item = T;

    fn next(&mut self) -> Option<Self::Item> {
        let current_chunk = match self.current_chunk.as_ref() {
            Some(current_chunk) if self.current_index < self.current_len => {
                Rc::clone(current_chunk)
            }
            _ => {
                loop {
                    // When we take a chunk off the list, we take ownership over the length as
                    // well, setting that of the chunk to zero. This is to ensure that if a
                    // panic occurs, we still drop all of the items pending in the iterator
                    // without violating any memory safety guarantees.
                    let current_chunk = self.chunks.pop_front()?;
                    self.current_chunk = Some(current_chunk.clone());
                    self.current_len = current_chunk.len.swap(0, Ordering::Relaxed);
                    self.current_index = 0;
                    if self.current_len > 0 {
                        break current_chunk;
                    }
                }
            }
        };

        let item = unsafe {
            let ptr = current_chunk.data().add(self.current_index).cast::<T>();
            ptr.read()
        };

        self.current_index += 1;
        Some(item)
    }
}

impl<T> Drop for IntoIter<T> {
    fn drop(&mut self) {
        // Drop any items in the current chunk that we're responsible for dropping
        if let Some(current_chunk) = self.current_chunk.take() {
            if core::mem::needs_drop::<T>() {
                let ptr = current_chunk.data();
                while self.current_index < self.current_len {
                    unsafe {
                        let ptr = ptr.add(self.current_index);
                        core::ptr::drop_in_place(ptr.as_ptr());
                        self.current_index += 1;
                    }
                }
            }
        }

        // Drop any leftover chunks
        self.chunks.clear();
    }
}

intrusive_adapter!(ChunkHeaderAdapter<T> = Rc<ChunkHeader<T>>: ChunkHeader<T> { link: LinkedListLink });

type ChunkList<T> = intrusive_collections::LinkedList<ChunkHeaderAdapter<T>>;

struct ChunkHeader<T> {
    link: LinkedListLink,
    /// Pointer to the allocated chunk
    chunk: NonNull<u8>,
    /// Allocated capacity of the chunk in units of T
    ///
    /// To obtain the allocated size of the chunk, you must use `Layout::array::<T>(self.capacity)`
    capacity: usize,
    /// The number of elements that have been stored in this chunk
    len: AtomicUsize,
    /// The alignment offset from `self.chunk` where the first element starts
    offset: usize,
    _marker: core::marker::PhantomData<T>,
}

impl<T> ChunkHeader<T> {
    pub fn new(capacity: usize) -> Rc<Self> {
        if T::IS_ZST {
            let chunk = NonNull::<T>::dangling();
            Rc::new(Self {
                link: LinkedListLink::new(),
                chunk: chunk.cast(),
                capacity: usize::MAX,
                len: Default::default(),
                offset: 0,
                _marker: core::marker::PhantomData,
            })
        } else {
            let layout = Self::layout(capacity);
            let chunk = unsafe { sysalloc::alloc(layout) };
            match NonNull::new(chunk) {
                Some(chunk) => {
                    let offset = chunk.align_offset(core::mem::align_of::<T>());
                    Rc::new(Self {
                        link: LinkedListLink::new(),
                        chunk,
                        capacity,
                        len: Default::default(),
                        offset,
                        _marker: core::marker::PhantomData,
                    })
                }
                None => sysalloc::handle_alloc_error(layout),
            }
        }
    }

    pub fn alloc(&self, item: T) -> NonNull<T> {
        // Reserve the slot in which we're going to write `item`
        let index = self.len.fetch_add(1, Ordering::Release);
        assert!(
            index < self.capacity,
            "unguarded call to `alloc` without checking capacity of chunk"
        );

        unsafe {
            let ptr = self.data().add(index).cast::<T>();
            let uninit_item = ptr.as_uninit_mut();
            uninit_item.write(item);
            ptr
        }
    }

    pub fn alloc_slice(&self, len: usize) -> NonNull<[T]> {
        // Reserve the slot(s) in which we're going to write the slice elements
        let index = self.len.fetch_add(len, Ordering::Release);
        assert!(
            index + len <= self.capacity,
            "unguarded call to `alloc_slice` without checking capacity of chunk"
        );

        unsafe {
            let ptr = self.data().add(index);
            NonNull::slice_from_raw_parts(ptr.cast::<T>(), len)
        }
    }

    /// Get a pointer to the first element of this chunk
    pub fn data(&self) -> NonNull<MaybeUninit<T>> {
        unsafe { self.chunk.byte_add(self.offset).cast() }
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.len.load(Ordering::Acquire)
    }

    pub fn available_capacity(&self) -> usize {
        self.capacity - self.len()
    }

    #[inline]
    fn layout(capacity: usize) -> Layout {
        Layout::array::<T>(capacity).expect("invalid capacity")
    }
}

impl<T> Drop for ChunkHeader<T> {
    fn drop(&mut self) {
        // We do not allocate any memory for zero-sized types
        if T::IS_ZST {
            return;
        }

        // Drop any initialized items in this chunk, if T has drop glue
        if core::mem::needs_drop::<T>() {
            let data = self.data().cast::<T>();
            for index in 0..self.len.load(Ordering::Relaxed) {
                unsafe {
                    let data = data.add(index);
                    core::ptr::drop_in_place(data.as_ptr());
                }
            }
        }

        // Deallocate chunk
        unsafe {
            sysalloc::dealloc(self.chunk.as_ptr(), Self::layout(self.capacity));
        }
    }
}

trait SpecArenaExtend: IntoIterator {
    fn extend_arena(self, chunks: &mut ChunkList<Self::Item>) -> NonNull<[Self::Item]>;
}

impl<I> SpecArenaExtend for I
where
    I: IntoIterator,
{
    default fn extend_arena(self, chunks: &mut ChunkList<Self::Item>) -> NonNull<[Self::Item]> {
        // We don't know the final capacity, so let Vec figure it out, and then take ownership
        // over its allocation and create a chunk representing it. We will place this chunk
        // before the current (unused) chunk, so that remaining capacity in that chunk can
        // continue to be filled
        self.into_iter().collect::<Vec<_>>().into_boxed_slice().extend_arena(chunks)
    }
}

impl<I> SpecArenaExtend for I
where
    I: IntoIterator,
    <I as IntoIterator>::IntoIter: ExactSizeIterator + core::iter::TrustedLen,
{
    default fn extend_arena(self, chunks: &mut ChunkList<Self::Item>) -> NonNull<[Self::Item]> {
        // We know the exact capacity required, see if we can use our current chunk, or allocate
        // a new one.
        let iter = self.into_iter();
        let len = iter.len();
        assert_ne!(len, usize::MAX, "invalid iterator: input too large");
        let capacity = core::cmp::max(len, Arena::<<I as IntoIterator>::Item>::DEFAULT_CHUNK_SIZE);
        let mut cursor = chunks.back_mut();

        // Allocate the backing memory for the slice
        let ptr = if cursor.is_null() {
            let chunk = ChunkHeader::<<I as IntoIterator>::Item>::new(capacity);
            let ptr = chunk.alloc_slice(len);
            cursor.insert_after(chunk);
            ptr
        } else {
            let chunk = unsafe { cursor.get().unwrap_unchecked() };
            if chunk.available_capacity() < len {
                // We don't have enough capacity in the current chunk, allocate a new one
                let chunk = ChunkHeader::<<I as IntoIterator>::Item>::new(capacity);
                let ptr = chunk.alloc_slice(len);
                // Place the chunk we just allocated before the last one, since it still has
                // available capacity
                cursor.insert_before(chunk);
                ptr
            } else {
                // We have enough capacity, use it
                chunk.alloc_slice(len)
            }
        };

        // Write the items
        let items = unsafe { ptr.as_uninit_slice_mut() };
        for (i, item) in iter.enumerate() {
            items[i].write(item);
        }

        ptr
    }
}

impl<T> SpecArenaExtend for Vec<T> {
    #[inline]
    fn extend_arena(self, chunks: &mut ChunkList<Self::Item>) -> NonNull<[T]> {
        self.into_boxed_slice().extend_arena(chunks)
    }
}

impl<T> SpecArenaExtend for Box<[T]> {
    fn extend_arena(self, chunks: &mut ChunkList<Self::Item>) -> NonNull<[T]> {
        let capacity = self.len();
        let ptr = unsafe { NonNull::new_unchecked(Box::into_raw(self)) };
        let mut cursor = chunks.back_mut();
        cursor.insert_before(Rc::new(ChunkHeader {
            link: LinkedListLink::new(),
            chunk: ptr.as_non_null_ptr().cast(),
            capacity,
            len: AtomicUsize::new(capacity),
            offset: 0,
            _marker: core::marker::PhantomData,
        }));
        ptr
    }
}

impl<T, const N: usize> SpecArenaExtend for [T; N] {
    fn extend_arena(self, chunks: &mut ChunkList<Self::Item>) -> NonNull<[T]> {
        // We know the exact capacity required, see if we can use our current chunk, or allocate
        // a new one.
        let mut cursor = chunks.back_mut();
        let capacity = core::cmp::max(N, Arena::<T>::DEFAULT_CHUNK_SIZE);

        // Allocate the backing memory for the slice
        let ptr = if cursor.is_null() {
            let chunk = ChunkHeader::<T>::new(capacity);
            let ptr = chunk.alloc_slice(N);
            cursor.insert_after(chunk);
            ptr
        } else {
            let chunk = unsafe { cursor.get().unwrap_unchecked() };
            if chunk.available_capacity() < N {
                // We don't have enough capacity in the current chunk, allocate a new one
                let chunk = ChunkHeader::<T>::new(capacity);
                let ptr = chunk.alloc_slice(N);
                // Place the chunk we just allocated before the last one, since it still has
                // available capacity
                cursor.insert_before(chunk);
                ptr
            } else {
                // We have enough capacity, use it
                chunk.alloc_slice(N)
            }
        };

        // Write the items
        let items = unsafe { ptr.as_uninit_slice_mut() };
        for (i, item) in self.into_iter().enumerate() {
            items[i].write(item);
        }

        ptr
    }
}
