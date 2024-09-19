use core::{fmt, mem::MaybeUninit, ptr::NonNull};

use super::{EntityMut, EntityRef, RawEntityMetadata, RawEntityRef, UnsafeIntrusiveEntityRef};

pub struct EntityList<T> {
    list: intrusive_collections::linked_list::LinkedList<EntityAdapter<T>>,
}
impl<T> Default for EntityList<T> {
    fn default() -> Self {
        Self {
            list: Default::default(),
        }
    }
}
impl<T> EntityList<T> {
    /// Construct a new, empty [EntityList]
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns true if this list is empty
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    /// Returns the number of entities in this list
    pub fn len(&self) -> usize {
        let mut cursor = self.list.front();
        let mut usize = 0;
        while !cursor.is_null() {
            usize += 1;
            cursor.move_next();
        }
        usize
    }

    /// Prepend `entity` to this list
    pub fn push_front(&mut self, entity: UnsafeIntrusiveEntityRef<T>) {
        self.list.push_front(entity);
    }

    /// Append `entity` to this list
    pub fn push_back(&mut self, entity: UnsafeIntrusiveEntityRef<T>) {
        self.list.push_back(entity);
    }

    /// Remove the entity at the front of the list, returning its [TrackedEntityHandle]
    ///
    /// Returns `None` if the list is empty.
    pub fn pop_front(&mut self) -> Option<UnsafeIntrusiveEntityRef<T>> {
        self.list.pop_back()
    }

    /// Remove the entity at the back of the list, returning its [TrackedEntityHandle]
    ///
    /// Returns `None` if the list is empty.
    pub fn pop_back(&mut self) -> Option<UnsafeIntrusiveEntityRef<T>> {
        self.list.pop_back()
    }

    #[doc(hidden)]
    pub fn cursor(&self) -> EntityCursor<'_, T> {
        EntityCursor {
            cursor: self.list.cursor(),
        }
    }

    #[doc(hidden)]
    pub fn cursor_mut(&mut self) -> EntityCursorMut<'_, T> {
        EntityCursorMut {
            cursor: self.list.cursor_mut(),
        }
    }

    /// Get an [EntityCursor] pointing to the first entity in the list, or the null object if
    /// the list is empty
    pub fn front(&self) -> EntityCursor<'_, T> {
        EntityCursor {
            cursor: self.list.front(),
        }
    }

    /// Get an [EntityCursorMut] pointing to the first entity in the list, or the null object if
    /// the list is empty
    pub fn front_mut(&mut self) -> EntityCursorMut<'_, T> {
        EntityCursorMut {
            cursor: self.list.front_mut(),
        }
    }

    /// Get an [EntityCursor] pointing to the last entity in the list, or the null object if
    /// the list is empty
    pub fn back(&self) -> EntityCursor<'_, T> {
        EntityCursor {
            cursor: self.list.back(),
        }
    }

    /// Get an [EntityCursorMut] pointing to the last entity in the list, or the null object if
    /// the list is empty
    pub fn back_mut(&mut self) -> EntityCursorMut<'_, T> {
        EntityCursorMut {
            cursor: self.list.back_mut(),
        }
    }

    /// Get an iterator over the entities in this list
    ///
    /// The iterator returned produces [EntityRef]s for each item in the list, with their lifetime
    /// bound to the list itself, not the iterator.
    pub fn iter(&self) -> EntityIter<'_, T> {
        EntityIter {
            cursor: self.cursor(),
            started: false,
        }
    }

    /// Removes all items from this list.
    ///
    /// This will unlink all entities currently in the list, which requires iterating through all
    /// elements in the list. If the entities may be used again, this ensures that their intrusive
    /// link is properly unlinked.
    pub fn clear(&mut self) {
        self.list.clear();
    }

    /// Empties the list without properly unlinking the intrusive links of the items in the list.
    ///
    /// Since this does not unlink any objects, any attempts to link these objects into another
    /// [EntityList] will fail but will not cause any memory unsafety. To unlink those objects
    /// manually, you must call the `force_unlink` function on the link.
    pub fn fast_clear(&mut self) {
        self.list.fast_clear();
    }

    /// Takes all the elements out of the [EntityList], leaving it empty.
    ///
    /// The taken elements are returned as a new [EntityList].
    pub fn take(&mut self) -> Self {
        Self {
            list: self.list.take(),
        }
    }

    /// Get a cursor to the item pointed to by `ptr`.
    ///
    /// # Safety
    ///
    /// This function may only be called when it is known that `ptr` refers to an entity which is
    /// linked into this list. This operation will panic if the entity is not linked into any list,
    /// and may result in undefined behavior if the operation is linked into a different list.
    pub unsafe fn cursor_from_ptr(&self, ptr: UnsafeIntrusiveEntityRef<T>) -> EntityCursor<'_, T> {
        unsafe {
            let raw = UnsafeIntrusiveEntityRef::into_inner(ptr).as_ptr();
            EntityCursor {
                cursor: self.list.cursor_from_ptr(raw),
            }
        }
    }

    /// Get a mutable cursor to the item pointed to by `ptr`.
    ///
    /// # Safety
    ///
    /// This function may only be called when it is known that `ptr` refers to an entity which is
    /// linked into this list. This operation will panic if the entity is not linked into any list,
    /// and may result in undefined behavior if the operation is linked into a different list.
    pub unsafe fn cursor_mut_from_ptr(
        &mut self,
        ptr: UnsafeIntrusiveEntityRef<T>,
    ) -> EntityCursorMut<'_, T> {
        let raw = UnsafeIntrusiveEntityRef::into_inner(ptr).as_ptr();
        unsafe {
            EntityCursorMut {
                cursor: self.list.cursor_mut_from_ptr(raw),
            }
        }
    }
}

impl<T: fmt::Debug> fmt::Debug for EntityList<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut builder = f.debug_list();
        for entity in self.iter() {
            builder.entry(&entity);
        }
        builder.finish()
    }
}

impl<T> FromIterator<UnsafeIntrusiveEntityRef<T>> for EntityList<T> {
    fn from_iter<I>(iter: I) -> Self
    where
        I: IntoIterator<Item = UnsafeIntrusiveEntityRef<T>>,
    {
        let mut list = EntityList::<T>::default();
        for handle in iter {
            list.push_back(handle);
        }
        list
    }
}

impl<T> IntoIterator for EntityList<T> {
    type IntoIter = intrusive_collections::linked_list::IntoIter<EntityAdapter<T>>;
    type Item = UnsafeIntrusiveEntityRef<T>;

    fn into_iter(self) -> Self::IntoIter {
        self.list.into_iter()
    }
}

impl<'a, T> IntoIterator for &'a EntityList<T> {
    type IntoIter = EntityIter<'a, T>;
    type Item = EntityRef<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

/// A cursor which provides read-only access to an [EntityList].
pub struct EntityCursor<'a, T> {
    cursor: intrusive_collections::linked_list::Cursor<'a, EntityAdapter<T>>,
}
impl<'a, T> EntityCursor<'a, T> {
    /// Returns true if this cursor is pointing to the null object
    #[inline]
    pub fn is_null(&self) -> bool {
        self.cursor.is_null()
    }

    /// Get a shared reference to the entity under the cursor.
    ///
    /// Returns `None` if the cursor is currently pointing to the null object.
    ///
    /// NOTE: This returns an [EntityRef] whose lifetime is bound to the underlying [EntityList],
    /// _not_ the [EntityCursor], since the cursor cannot mutate the list.
    pub fn get(&self) -> Option<EntityRef<'a, T>> {
        self.cursor.get().map(|obj| obj.entity.borrow())
    }

    /// Get the [TrackedEntityHandle] corresponding to the entity under the cursor.
    ///
    /// Returns `None` if the cursor is pointing to the null object.
    #[inline]
    pub fn as_pointer(&self) -> Option<UnsafeIntrusiveEntityRef<T>> {
        self.cursor.clone_pointer()
    }

    /// Moves the cursor to the next element of the [EntityList].
    ///
    /// If the cursor is pointing to the null object then this will move it to the front of the
    /// [EntityList]. If it is pointing to the back of the [EntityList] then this will move it to
    /// the null object.
    #[inline]
    pub fn move_next(&mut self) {
        self.cursor.move_next();
    }

    /// Moves the cursor to the previous element of the [EntityList].
    ///
    /// If the cursor is pointing to the null object then this will move it to the back of the
    /// [EntityList]. If it is pointing to the front of the [EntityList] then this will move it to
    /// the null object.
    #[inline]
    pub fn move_prev(&mut self) {
        self.cursor.move_prev();
    }

    /// Returns a cursor pointing to the next element of the [EntityList].
    ///
    /// If the cursor is pointing to the null object then this will return a cursor pointing to the
    /// front of the [EntityList]. If it is pointing to the last entity of the [EntityList] then
    /// this will return a null cursor.
    #[inline]
    pub fn peek_next(&self) -> EntityCursor<'_, T> {
        EntityCursor {
            cursor: self.cursor.peek_next(),
        }
    }

    /// Returns a cursor pointing to the previous element of the [EntityList].
    ///
    /// If the cursor is pointing to the null object then this will return a cursor pointing to
    /// the last entity in the [EntityList]. If it is pointing to the front of the [EntityList] then
    /// this will return a null cursor.
    #[inline]
    pub fn peek_prev(&self) -> EntityCursor<'_, T> {
        EntityCursor {
            cursor: self.cursor.peek_prev(),
        }
    }
}

/// A cursor which provides mutable access to an [EntityList].
pub struct EntityCursorMut<'a, T> {
    cursor: intrusive_collections::linked_list::CursorMut<'a, EntityAdapter<T>>,
}
impl<'a, T> EntityCursorMut<'a, T> {
    /// Returns true if this cursor is pointing to the null object
    #[inline]
    pub fn is_null(&self) -> bool {
        self.cursor.is_null()
    }

    /// Get a shared reference to the entity under the cursor.
    ///
    /// Returns `None` if the cursor is currently pointing to the null object.
    ///
    /// NOTE: This binds the lifetime of the [EntityRef] to the cursor, to ensure that the cursor
    /// is frozen while the entity is being borrowed. This ensures that only one reference at a
    /// time is being handed out by this cursor.
    pub fn get(&self) -> Option<EntityRef<'_, T>> {
        self.cursor.get().map(|obj| obj.entity.borrow())
    }

    /// Get a mutable reference to the entity under the cursor.
    ///
    /// Returns `None` if the cursor is currently pointing to the null object.
    ///
    /// Not only does this mutably borrow the cursor, the lifetime of the [EntityMut] is bound to
    /// that of the cursor, which means it cannot outlive the cursor, and also prevents the cursor
    /// from being accessed in any way until the mutable reference is dropped. This makes it
    /// impossible to try and alias the underlying entity using the cursor.
    pub fn get_mut(&mut self) -> Option<EntityMut<'_, T>> {
        self.cursor.get().map(|obj| obj.entity.borrow_mut())
    }

    /// Returns a read-only cursor pointing to the current element.
    ///
    /// The lifetime of the returned [EntityCursor] is bound to that of the [EntityCursorMut], which
    /// means it cannot outlive the [EntityCursorMut] and that the [EntityCursorMut] is frozen for
    /// the lifetime of the [EntityCursor].
    pub fn as_cursor(&self) -> EntityCursor<'_, T> {
        EntityCursor {
            cursor: self.cursor.as_cursor(),
        }
    }

    /// Get the [TrackedEntityHandle] corresponding to the entity under the cursor.
    ///
    /// Returns `None` if the cursor is pointing to the null object.
    #[inline]
    pub fn as_pointer(&self) -> Option<UnsafeIntrusiveEntityRef<T>> {
        self.cursor.as_cursor().clone_pointer()
    }

    /// Moves the cursor to the next element of the [EntityList].
    ///
    /// If the cursor is pointing to the null object then this will move it to the front of the
    /// [EntityList]. If it is pointing to the back of the [EntityList] then this will move it to
    /// the null object.
    #[inline]
    pub fn move_next(&mut self) {
        self.cursor.move_next();
    }

    /// Moves the cursor to the previous element of the [EntityList].
    ///
    /// If the cursor is pointing to the null object then this will move it to the back of the
    /// [EntityList]. If it is pointing to the front of the [EntityList] then this will move it to
    /// the null object.
    #[inline]
    pub fn move_prev(&mut self) {
        self.cursor.move_prev();
    }

    /// Returns a cursor pointing to the next element of the [EntityList].
    ///
    /// If the cursor is pointing to the null object then this will return a cursor pointing to the
    /// front of the [EntityList]. If it is pointing to the last entity of the [EntityList] then
    /// this will return a null cursor.
    #[inline]
    pub fn peek_next(&self) -> EntityCursor<'_, T> {
        EntityCursor {
            cursor: self.cursor.peek_next(),
        }
    }

    /// Returns a cursor pointing to the previous element of the [EntityList].
    ///
    /// If the cursor is pointing to the null object then this will return a cursor pointing to
    /// the last entity in the [EntityList]. If it is pointing to the front of the [EntityList] then
    /// this will return a null cursor.
    #[inline]
    pub fn peek_prev(&self) -> EntityCursor<'_, T> {
        EntityCursor {
            cursor: self.cursor.peek_prev(),
        }
    }

    /// Removes the current entity from the [EntityList].
    ///
    /// A pointer to the element that was removed is returned, and the cursor is moved to point to
    /// the next element in the [Entitylist].
    ///
    /// If the cursor is currently pointing to the null object then nothing is removed and `None` is
    /// returned.
    #[inline]
    pub fn remove(&mut self) -> Option<UnsafeIntrusiveEntityRef<T>> {
        self.cursor.remove()
    }

    /// Removes the current entity from the [EntityList] and inserts another one in its place.
    ///
    /// A pointer to the entity that was removed is returned, and the cursor is modified to point to
    /// the newly added entity.
    ///
    /// If the cursor is currently pointing to the null object then `Err` is returned containing the
    /// entity we failed to insert.
    ///
    /// # Panics
    /// Panics if the new entity is already linked to a different intrusive collection.
    #[inline]
    pub fn replace_with(
        &mut self,
        value: UnsafeIntrusiveEntityRef<T>,
    ) -> Result<UnsafeIntrusiveEntityRef<T>, UnsafeIntrusiveEntityRef<T>> {
        self.cursor.replace_with(value)
    }

    /// Inserts a new entity into the [EntityList], after the current cursor position.
    ///
    /// If the cursor is pointing at the null object then the entity is inserted at the start of the
    /// underlying [EntityList].
    ///
    /// # Panics
    ///
    /// Panics if the entity is already linked to a different [EntityList]
    #[inline]
    pub fn insert_after(&mut self, value: UnsafeIntrusiveEntityRef<T>) {
        self.cursor.insert_after(value)
    }

    /// Inserts a new entity into the [EntityList], before the current cursor position.
    ///
    /// If the cursor is pointing at the null object then the entity is inserted at the end of the
    /// underlying [EntityList].
    ///
    /// # Panics
    ///
    /// Panics if the entity is already linked to a different [EntityList]
    #[inline]
    pub fn insert_before(&mut self, value: UnsafeIntrusiveEntityRef<T>) {
        self.cursor.insert_before(value)
    }

    /// This splices `list` into the underlying list of `self` by inserting the elements of `list`
    /// after the current cursor position.
    ///
    /// For example, let's say we have the following list and cursor position:
    ///
    /// ```text,ignore
    /// [A, B, C]
    ///     ^-- cursor
    /// ```
    ///
    /// Splicing a new list, `[D, E, F]` after the cursor would result in:
    ///
    /// ```text,ignore
    /// [A, B, D, E, F, C]
    ///     ^-- cursor
    /// ```
    ///
    /// If the cursor is pointing at the null object, then `list` is appended to the start of the
    /// underlying [EntityList] for this cursor.
    #[inline]
    pub fn splice_after(&mut self, list: EntityList<T>) {
        self.cursor.splice_after(list.list)
    }

    /// This splices `list` into the underlying list of `self` by inserting the elements of `list`
    /// before the current cursor position.
    ///
    /// For example, let's say we have the following list and cursor position:
    ///
    /// ```text,ignore
    /// [A, B, C]
    ///     ^-- cursor
    /// ```
    ///
    /// Splicing a new list, `[D, E, F]` before the cursor would result in:
    ///
    /// ```text,ignore
    /// [A, D, E, F, B, C]
    ///              ^-- cursor
    /// ```
    ///
    /// If the cursor is pointing at the null object, then `list` is appended to the end of the
    /// underlying [EntityList] for this cursor.
    #[inline]
    pub fn splice_before(&mut self, list: EntityList<T>) {
        self.cursor.splice_before(list.list)
    }

    /// Splits the list into two after the current cursor position.
    ///
    /// This will return a new list consisting of everything after the cursor, with the original
    /// list retaining everything before.
    ///
    /// If the cursor is pointing at the null object then the entire contents of the [EntityList]
    /// are moved.
    pub fn split_after(&mut self) -> EntityList<T> {
        let list = self.cursor.split_after();
        EntityList { list }
    }

    /// Splits the list into two before the current cursor position.
    ///
    /// This will return a new list consisting of everything before the cursor, with the original
    /// list retaining everything after.
    ///
    /// If the cursor is pointing at the null object then the entire contents of the [EntityList]
    /// are moved.
    pub fn split_before(&mut self) -> EntityList<T> {
        let list = self.cursor.split_before();
        EntityList { list }
    }
}

pub struct EntityIter<'a, T> {
    cursor: EntityCursor<'a, T>,
    started: bool,
}
impl<'a, T> core::iter::FusedIterator for EntityIter<'a, T> {}
impl<'a, T> Iterator for EntityIter<'a, T> {
    type Item = EntityRef<'a, T>;

    fn next(&mut self) -> Option<Self::Item> {
        // If we haven't started iterating yet, then we're on the null cursor, so move to the
        // front of the list now that we have started iterating.
        if !self.started {
            self.started = true;
            self.cursor.move_next();
        }
        let item = self.cursor.get()?;
        self.cursor.move_next();
        Some(item)
    }
}
impl<'a, T> DoubleEndedIterator for EntityIter<'a, T> {
    fn next_back(&mut self) -> Option<Self::Item> {
        // If we haven't started iterating yet, then we're on the null cursor, so move to the
        // back of the list now that we have started iterating.
        if !self.started {
            self.started = true;
            self.cursor.move_prev();
        }
        let item = self.cursor.get()?;
        self.cursor.move_prev();
        Some(item)
    }
}

type IntrusiveLink = intrusive_collections::LinkedListLink;

impl<T: 'static> RawEntityRef<T, IntrusiveLink> {
    /// Create a new [UnsafeIntrusiveEntityRef] by allocating `value` in `arena`
    ///
    /// # SAFETY
    ///
    /// This function has the same requirements around safety as [RawEntityRef::new].
    pub fn new(value: T, arena: &blink_alloc::Blink) -> Self {
        RawEntityRef::new_with_metadata(value, IntrusiveLink::new(), arena)
    }

    pub fn new_uninit(arena: &blink_alloc::Blink) -> RawEntityRef<MaybeUninit<T>, IntrusiveLink> {
        RawEntityRef::new_uninit_with_metadata(IntrusiveLink::new(), arena)
    }
}
impl<T> RawEntityRef<T, IntrusiveLink> {
    /// Returns true if this entity is linked into an intrusive list
    pub fn is_linked(&self) -> bool {
        unsafe {
            let offset = core::mem::offset_of!(RawEntityMetadata<T, IntrusiveLink>, metadata);
            let current = self.inner.byte_add(offset).cast::<IntrusiveLink>();
            current.as_ref().is_linked()
        }
    }

    /// Get the previous entity in the list of `T` containing the current entity
    ///
    /// For example, in a list of `Operation` in a `Block`, this would return the handle of the
    /// previous operation in the block, or `None` if there are no other ops before this one.
    pub fn prev(&self) -> Option<Self> {
        use intrusive_collections::linked_list::{LinkOps, LinkedListOps};
        unsafe {
            let offset = core::mem::offset_of!(RawEntityMetadata<T, IntrusiveLink>, metadata);
            let current = self.inner.byte_add(offset).cast();
            LinkOps.prev(current).map(|link_ptr| Self::from_link_ptr(link_ptr))
        }
    }

    /// Get the next entity in the list of `T` containing the current entity
    ///
    /// For example, in a list of `Operation` in a `Block`, this would return the handle of the
    /// next operation in the block, or `None` if there are no other ops after this one.
    pub fn next(&self) -> Option<Self> {
        use intrusive_collections::linked_list::{LinkOps, LinkedListOps};
        unsafe {
            let offset = core::mem::offset_of!(RawEntityMetadata<T, IntrusiveLink>, metadata);
            let current = self.inner.byte_add(offset).cast();
            LinkOps.next(current).map(|link_ptr| Self::from_link_ptr(link_ptr))
        }
    }

    #[inline]
    unsafe fn from_link_ptr(link: NonNull<IntrusiveLink>) -> Self {
        let offset = core::mem::offset_of!(RawEntityMetadata<T, IntrusiveLink>, metadata);
        let ptr = link.byte_sub(offset).cast::<RawEntityMetadata<T, IntrusiveLink>>();
        Self { inner: ptr }
    }
}

#[doc(hidden)]
pub struct DefaultPointerOps<T: ?Sized>(core::marker::PhantomData<T>);
impl<T: ?Sized> Copy for DefaultPointerOps<T> {}
impl<T: ?Sized> Clone for DefaultPointerOps<T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T: ?Sized> Default for DefaultPointerOps<T> {
    fn default() -> Self {
        Self::new()
    }
}
impl<T: ?Sized> DefaultPointerOps<T> {
    const fn new() -> Self {
        Self(core::marker::PhantomData)
    }
}

unsafe impl<T> intrusive_collections::PointerOps
    for DefaultPointerOps<UnsafeIntrusiveEntityRef<T>>
{
    type Pointer = UnsafeIntrusiveEntityRef<T>;
    type Value = RawEntityMetadata<T, IntrusiveLink>;

    #[inline]
    unsafe fn from_raw(&self, value: *const Self::Value) -> Self::Pointer {
        debug_assert!(!value.is_null() && value.is_aligned());
        UnsafeIntrusiveEntityRef::from_ptr(value.cast_mut())
    }

    #[inline]
    fn into_raw(&self, ptr: Self::Pointer) -> *const Self::Value {
        UnsafeIntrusiveEntityRef::into_inner(ptr).as_ptr().cast_const()
    }
}

/// An adapter for storing any `Entity` impl in a [intrusive_collections::LinkedList]
pub struct EntityAdapter<T> {
    link_ops: intrusive_collections::linked_list::LinkOps,
    ptr_ops: DefaultPointerOps<UnsafeIntrusiveEntityRef<T>>,
    marker: core::marker::PhantomData<T>,
}
impl<T> Copy for EntityAdapter<T> {}
impl<T> Clone for EntityAdapter<T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T> Default for EntityAdapter<T> {
    fn default() -> Self {
        Self::new()
    }
}
impl<T> EntityAdapter<T> {
    pub const fn new() -> Self {
        Self {
            link_ops: intrusive_collections::linked_list::LinkOps,
            ptr_ops: DefaultPointerOps::new(),
            marker: core::marker::PhantomData,
        }
    }
}

unsafe impl<T> intrusive_collections::Adapter for EntityAdapter<T> {
    type LinkOps = intrusive_collections::linked_list::LinkOps;
    type PointerOps = DefaultPointerOps<UnsafeIntrusiveEntityRef<T>>;

    unsafe fn get_value(
        &self,
        link: <Self::LinkOps as intrusive_collections::LinkOps>::LinkPtr,
    ) -> *const <Self::PointerOps as intrusive_collections::PointerOps>::Value {
        let raw_entity_ref = UnsafeIntrusiveEntityRef::<T>::from_link_ptr(link);
        raw_entity_ref.inner.as_ptr().cast_const()
    }

    unsafe fn get_link(
        &self,
        value: *const <Self::PointerOps as intrusive_collections::PointerOps>::Value,
    ) -> <Self::LinkOps as intrusive_collections::LinkOps>::LinkPtr {
        let raw_entity_ref = UnsafeIntrusiveEntityRef::from_ptr(value.cast_mut());
        let offset = RawEntityMetadata::<T, IntrusiveLink>::metadata_offset();
        raw_entity_ref.inner.byte_add(offset).cast()
    }

    fn link_ops(&self) -> &Self::LinkOps {
        &self.link_ops
    }

    fn link_ops_mut(&mut self) -> &mut Self::LinkOps {
        &mut self.link_ops
    }

    fn pointer_ops(&self) -> &Self::PointerOps {
        &self.ptr_ops
    }
}
