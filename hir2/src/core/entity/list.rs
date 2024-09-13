use core::fmt;

use super::{EntityAdapter, EntityRef, TrackedEntityHandle};

#[derive(Default)]
pub struct EntityList<T> {
    list: intrusive_collections::linked_list::LinkedList<EntityAdapter<T>>,
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
    pub fn push_front(&mut self, entity: TrackedEntityHandle<T>) {
        self.list.push_front(entity);
    }

    /// Append `entity` to this list
    pub fn push_back(&mut self, entity: TrackedEntityHandle<T>) {
        self.list.push_back(entity);
    }

    /// Remove the entity at the front of the list, returning its [TrackedEntityHandle]
    ///
    /// Returns `None` if the list is empty.
    pub fn pop_front(&mut self) -> Option<TrackedEntityHandle<T>> {
        self.list.pop_back()
    }

    /// Remove the entity at the back of the list, returning its [TrackedEntityHandle]
    ///
    /// Returns `None` if the list is empty.
    pub fn pop_back(&mut self) -> Option<TrackedEntityHandle<T>> {
        self.list.pop_back()
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
    pub fn front_mut(&self) -> EntityCursorMut<'_, T> {
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
    pub fn back_mut(&self) -> EntityCursorMut<'_, T> {
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
            cursor: self.list.cursor(),
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

impl<T> FromIterator<TrackedEntityHandle<T>> for EntityList<T> {
    fn from_iter<T>(iter: T) -> Self
    where
        T: IntoIterator<Item = TrackedEntityHandle<T>>,
    {
        let mut list = EntityList::<T>::default();
        for handle in iter {
            list.push_back(handle);
        }
        list
    }
}

impl<T> IntoIterator for EntityList<T> {
    type IntoIter = intrusive_collections::linked_list::IntoIter<EntityAdapter>;
    type Item = TrackedEntityHandle<T>;

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
        match self.cursor.get() {
            Some(obj) => Some(obj.borrow()),
            None => None,
        }
    }

    /// Get the [TrackedEntityHandle] corresponding to the entity under the cursor.
    ///
    /// Returns `None` if the cursor is pointing to the null object.
    #[inline]
    pub fn as_pointer(&self) -> Option<TrackedEntityHandle<T>> {
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
        match self.cursor.get() {
            Some(obj) => Some(obj.borrow()),
            None => None,
        }
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
        match self.cursor.get() {
            Some(obj) => Some(obj.borrow_mut()),
            None => None,
        }
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
    pub fn as_pointer(&self) -> Option<TrackedEntityHandle<T>> {
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

    /// Removes the current entity from the [EntityList].
    ///
    /// A pointer to the element that was removed is returned, and the cursor is moved to point to
    /// the next element in the [Entitylist].
    ///
    /// If the cursor is currently pointing to the null object then nothing is removed and `None` is
    /// returned.
    #[inline]
    pub fn remove(&mut self) -> Option<TrackedEntityHandle<T>> {
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
        value: TrackedEntityHandle<T>,
    ) -> Result<TrackedEntityHandle<T>, TrackedEntityHandle<T>> {
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
    pub fn insert_after(&mut self, value: TrackedEntityHandle<T>) {
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
    pub fn insert_before(&mut self, value: TrackedEntityHandle<T>) {
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

    /// Consumes this cursor, and returns a reference to the entity that the cursor is currently
    /// pointing to.
    ///
    /// Unlike [get], the returned reference’s lifetime is tied to [EntityList]’s lifetime.
    ///
    /// This returns `None` if the cursor is currently pointing to the null object.
    ///
    /// NOTE: This function will panic if there are any outstanding mutable borrows of the
    /// underlying entity.
    pub fn into_ref(self) -> Option<EntityRef<'a, T>> {
        match self.cursor.get() {
            Some(obj) => Some(obj.borrow()),
            None => None,
        }
    }

    /// Consumes this cursor, and returns a mutable reference to the entity that the cursor is
    /// currently pointing to.
    ///
    /// Unlike [get_mut], the returned reference’s lifetime is tied to the [EntityList]’s lifetime.
    ///
    /// This returns `None` if the cursor is currently pointing to the null object.
    ///
    /// NOTE: This function will panic if there are any outstanding borrows of the underlying entity
    pub fn into_mut(self) -> Option<EntityMut<'a, T>> {
        match self.cursor.get() {
            Some(obj) => Some(obj.borrow_mut()),
            None => None,
        }
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
