use core::{cell::Cell, fmt, mem::MaybeUninit, ptr::NonNull};

use super::{
    EntityMut, EntityParent, EntityRef, EntityWithParent, RawEntityMetadata, RawEntityRef,
    UnsafeIntrusiveEntityRef,
};

type EntityAdapter<T> =
    super::adapter::EntityAdapter<T, IntrusiveLink, intrusive_collections::linked_list::LinkOps>;

pub struct IntrusiveLink {
    link: intrusive_collections::linked_list::Link,
    parent: Cell<*const ()>,
}

impl Default for IntrusiveLink {
    fn default() -> Self {
        Self {
            link: Default::default(),
            parent: Cell::new(core::ptr::null()),
        }
    }
}

impl IntrusiveLink {
    #[inline]
    pub fn is_linked(&self) -> bool {
        self.link.is_linked()
    }
}

impl IntrusiveLink {
    pub(self) fn set_parent<T>(&self, parent: Option<UnsafeIntrusiveEntityRef<T>>) {
        if let Some(parent) = parent {
            assert!(
                self.link.is_linked(),
                "must add entity to parent entity list before setting parent"
            );
            self.parent.set(UnsafeIntrusiveEntityRef::as_ptr(&parent).cast());
        } else if self.parent.get().is_null() {
            panic!("no parent previously set");
        } else {
            self.parent.set(core::ptr::null());
        }
    }

    pub fn parent<T>(&self) -> Option<UnsafeIntrusiveEntityRef<T>> {
        let parent = self.parent.get();
        if parent.is_null() {
            // EntityList is orphaned
            None
        } else {
            Some(unsafe { UnsafeIntrusiveEntityRef::from_raw(parent.cast()) })
        }
    }
}

pub struct EntityList<T> {
    list: intrusive_collections::linked_list::LinkedList<EntityAdapter<T>>,
}
impl<T: Eq> Eq for EntityList<T> {}
impl<T: PartialEq> PartialEq for EntityList<T> {
    fn eq(&self, other: &Self) -> bool {
        let mut lhs = self.list.front();
        let mut rhs = other.list.front();
        loop {
            match (lhs.get(), rhs.get()) {
                (Some(l), Some(r)) => {
                    if l.borrow() != r.borrow() {
                        break false;
                    }
                }
                (None, None) => break true,
                _ => break false,
            }
            lhs.move_next();
            rhs.move_next();
        }
    }
}
impl<T: core::hash::Hash> core::hash::Hash for EntityList<T> {
    fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
        for item in self.list.iter() {
            item.borrow().hash(state);
        }
    }
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

    #[doc(hidden)]
    pub fn cursor(&self) -> EntityListCursor<'_, T> {
        EntityListCursor {
            cursor: self.list.cursor(),
        }
    }

    /// Get an [EntityListCursor] pointing to the first entity in the list, or the null object if
    /// the list is empty
    pub fn front(&self) -> EntityListCursor<'_, T> {
        EntityListCursor {
            cursor: self.list.front(),
        }
    }

    /// Get an [EntityListCursor] pointing to the last entity in the list, or the null object if
    /// the list is empty
    pub fn back(&self) -> EntityListCursor<'_, T> {
        EntityListCursor {
            cursor: self.list.back(),
        }
    }

    /// Get an iterator over the entities in this list
    ///
    /// The iterator returned produces [EntityRef]s for each item in the list, with their lifetime
    /// bound to the list itself, not the iterator.
    pub fn iter(&self) -> EntityListIter<'_, T> {
        EntityListIter {
            cursor: self.cursor(),
            started: false,
        }
    }

    /// Get a cursor to the item pointed to by `ptr`.
    ///
    /// # Safety
    ///
    /// This function may only be called when it is known that `ptr` refers to an entity which is
    /// linked into this list. This operation will panic if the entity is not linked into any list,
    /// and may result in undefined behavior if the operation is linked into a different list.
    pub unsafe fn cursor_from_ptr(
        &self,
        ptr: UnsafeIntrusiveEntityRef<T>,
    ) -> EntityListCursor<'_, T> {
        unsafe {
            let raw = UnsafeIntrusiveEntityRef::into_inner(ptr).as_ptr();
            EntityListCursor {
                cursor: self.list.cursor_from_ptr(raw),
            }
        }
    }
}

impl<T> EntityList<T>
where
    T: EntityWithParent,
{
    pub(crate) fn parent(&self) -> UnsafeIntrusiveEntityRef<<T as EntityWithParent>::Parent> {
        let offset = <<T as EntityWithParent>::Parent as EntityParent<T>>::offset();
        let ptr = self as *const EntityList<T>;
        unsafe {
            let parent = ptr.byte_sub(offset).cast::<<T as EntityWithParent>::Parent>();
            UnsafeIntrusiveEntityRef::from_raw(parent)
        }
    }
}

trait EntityListTraits<T>: Sized {
    fn cursor_mut(&mut self) -> EntityListCursorMut<'_, T>;

    /// Get a mutable cursor to the item pointed to by `ptr`.
    ///
    /// # Safety
    ///
    /// This function may only be called when it is known that `ptr` refers to an entity which is
    /// linked into this list. This operation will panic if the entity is not linked into any list,
    /// and may result in undefined behavior if the operation is linked into a different list.
    unsafe fn cursor_mut_from_ptr(
        &mut self,
        ptr: UnsafeIntrusiveEntityRef<T>,
    ) -> EntityListCursorMut<'_, T>;

    /// Get an [EntityListCursorMut] pointing to the first entity in the list, or the null object if
    /// the list is empty
    fn front_mut(&mut self) -> EntityListCursorMut<'_, T>;

    /// Get an [EntityListCursorMut] pointing to the last entity in the list, or the null object if
    /// the list is empty
    fn back_mut(&mut self) -> EntityListCursorMut<'_, T>;

    fn remove(cursor: &mut EntityListCursorMut<'_, T>) -> Option<UnsafeIntrusiveEntityRef<T>>;

    fn replace_with(
        cursor: &mut EntityListCursorMut<'_, T>,
        entity: UnsafeIntrusiveEntityRef<T>,
    ) -> Result<UnsafeIntrusiveEntityRef<T>, UnsafeIntrusiveEntityRef<T>>;

    /// Prepend `entity` to this list
    fn push_front(list: &mut EntityList<T>, entity: UnsafeIntrusiveEntityRef<T>);

    /// Append `entity` to this list
    fn push_back(list: &mut EntityList<T>, entity: UnsafeIntrusiveEntityRef<T>);

    fn insert_after(cursor: &mut EntityListCursorMut<'_, T>, entity: UnsafeIntrusiveEntityRef<T>);

    fn insert_before(cursor: &mut EntityListCursorMut<'_, T>, entity: UnsafeIntrusiveEntityRef<T>);

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
    fn splice_after(cursor: &mut EntityListCursorMut<'_, T>, list: EntityList<T>);

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
    fn splice_before(cursor: &mut EntityListCursorMut<'_, T>, list: EntityList<T>);

    /// Splits the list into two after the current cursor position.
    ///
    /// This will return a new list consisting of everything after the cursor, with the original
    /// list retaining everything before.
    ///
    /// If the cursor is pointing at the null object then the entire contents of the [EntityList]
    /// are moved.
    fn split_after(cursor: &mut EntityListCursorMut<'_, T>) -> EntityList<T>;

    /// Splits the list into two before the current cursor position.
    ///
    /// This will return a new list consisting of everything before the cursor, with the original
    /// list retaining everything after.
    ///
    /// If the cursor is pointing at the null object then the entire contents of the [EntityList]
    /// are moved.
    fn split_before(cursor: &mut EntityListCursorMut<'_, T>) -> EntityList<T>;

    /// Remove the entity at the front of the list, returning its [UnsafeIntrusiveEntityRef]
    ///
    /// Returns `None` if the list is empty.
    fn pop_front(&mut self) -> Option<UnsafeIntrusiveEntityRef<T>>;

    /// Remove the entity at the back of the list, returning its [UnsafeIntrusiveEntityRef]
    ///
    /// Returns `None` if the list is empty.
    fn pop_back(&mut self) -> Option<UnsafeIntrusiveEntityRef<T>>;

    /// Removes all items from this list.
    ///
    /// This will unlink all entities currently in the list, which requires iterating through all
    /// elements in the list. If the entities may be used again, this ensures that their intrusive
    /// link is properly unlinked.
    fn clear(&mut self);

    /// Takes all the elements out of the [EntityList], leaving it empty.
    ///
    /// The taken elements are returned as a new [EntityList].
    fn take(&mut self) -> Self;
}

impl<T: EntityListItem> EntityListTraits<T> for EntityList<T> {
    default fn cursor_mut(&mut self) -> EntityListCursorMut<'_, T> {
        EntityListCursorMut {
            cursor: self.list.cursor_mut(),
            parent: core::ptr::null(),
        }
    }

    default unsafe fn cursor_mut_from_ptr(
        &mut self,
        ptr: UnsafeIntrusiveEntityRef<T>,
    ) -> EntityListCursorMut<'_, T> {
        let raw = UnsafeIntrusiveEntityRef::into_inner(ptr).as_ptr();
        unsafe {
            EntityListCursorMut {
                cursor: self.list.cursor_mut_from_ptr(raw),
                parent: core::ptr::null(),
            }
        }
    }

    /// Get an [EntityListCursorMut] pointing to the first entity in the list, or the null object if
    /// the list is empty
    default fn front_mut(&mut self) -> EntityListCursorMut<'_, T> {
        EntityListCursorMut {
            cursor: self.list.front_mut(),
            parent: core::ptr::null(),
        }
    }

    /// Get an [EntityListCursorMut] pointing to the last entity in the list, or the null object if
    /// the list is empty
    default fn back_mut(&mut self) -> EntityListCursorMut<'_, T> {
        EntityListCursorMut {
            cursor: self.list.back_mut(),
            parent: core::ptr::null(),
        }
    }

    default fn remove(
        cursor: &mut EntityListCursorMut<'_, T>,
    ) -> Option<UnsafeIntrusiveEntityRef<T>> {
        let entity = cursor.cursor.remove()?;
        <T as EntityListItem>::on_removed(entity, cursor);
        Some(entity)
    }

    default fn replace_with(
        cursor: &mut EntityListCursorMut<'_, T>,
        entity: UnsafeIntrusiveEntityRef<T>,
    ) -> Result<UnsafeIntrusiveEntityRef<T>, UnsafeIntrusiveEntityRef<T>> {
        let removed = cursor.cursor.replace_with(entity)?;
        <T as EntityListItem>::on_removed(removed, cursor);
        <T as EntityListItem>::on_inserted(entity, cursor);
        Ok(removed)
    }

    default fn push_front(list: &mut EntityList<T>, entity: UnsafeIntrusiveEntityRef<T>) {
        list.list.push_front(entity);
        <T as EntityListItem>::on_inserted(entity, &mut list.front_mut());
    }

    default fn push_back(list: &mut EntityList<T>, entity: UnsafeIntrusiveEntityRef<T>) {
        list.list.push_back(entity);
        <T as EntityListItem>::on_inserted(entity, &mut list.back_mut());
    }

    default fn insert_after(
        cursor: &mut EntityListCursorMut<'_, T>,
        entity: UnsafeIntrusiveEntityRef<T>,
    ) {
        cursor.cursor.insert_after(entity);
        <T as EntityListItem>::on_inserted(entity, cursor);
    }

    default fn insert_before(
        cursor: &mut EntityListCursorMut<'_, T>,
        entity: UnsafeIntrusiveEntityRef<T>,
    ) {
        cursor.cursor.insert_before(entity);
        <T as EntityListItem>::on_inserted(entity, cursor);
    }

    default fn splice_after(cursor: &mut EntityListCursorMut<'_, T>, mut list: EntityList<T>) {
        while let Some(entity) = list.list.pop_back() {
            Self::insert_after(cursor, entity)
        }
    }

    default fn splice_before(cursor: &mut EntityListCursorMut<'_, T>, mut list: EntityList<T>) {
        while let Some(entity) = list.list.pop_front() {
            Self::insert_before(cursor, entity)
        }
    }

    default fn split_after(cursor: &mut EntityListCursorMut<'_, T>) -> EntityList<T> {
        let list = cursor.cursor.split_after();
        let mut list_cursor = list.front();
        while let Some(entity) = list_cursor.clone_pointer() {
            <T as EntityListItem>::on_removed(entity, cursor);
            list_cursor.move_next();
        }
        Self { list }
    }

    default fn split_before(cursor: &mut EntityListCursorMut<'_, T>) -> EntityList<T> {
        let list = cursor.cursor.split_before();
        let mut list_cursor = list.front();
        while let Some(entity) = list_cursor.clone_pointer() {
            <T as EntityListItem>::on_removed(entity, cursor);
            list_cursor.move_next();
        }
        Self { list }
    }

    default fn pop_front(&mut self) -> Option<UnsafeIntrusiveEntityRef<T>> {
        let removed = self.list.pop_front()?;
        <T as EntityListItem>::on_removed(removed, &mut self.front_mut());
        Some(removed)
    }

    default fn pop_back(&mut self) -> Option<UnsafeIntrusiveEntityRef<T>> {
        let removed = self.list.pop_back()?;
        <T as EntityListItem>::on_removed(removed, &mut self.back_mut());
        Some(removed)
    }

    default fn clear(&mut self) {
        while self.pop_front().is_some() {}
    }

    default fn take(&mut self) -> Self {
        let list = self.list.take();
        let mut list_cursor = list.front();
        while let Some(entity) = list_cursor.clone_pointer() {
            <T as EntityListItem>::on_removed(entity, &mut self.front_mut());
            list_cursor.move_next();
        }
        Self { list }
    }
}

impl<T> EntityListTraits<T> for EntityList<T>
where
    T: EntityListItem + EntityWithParent,
    <T as EntityWithParent>::Parent: EntityParent<T>,
{
    fn cursor_mut(&mut self) -> EntityListCursorMut<'_, T> {
        let parent = UnsafeIntrusiveEntityRef::into_raw(self.parent()).cast();
        EntityListCursorMut {
            cursor: self.list.cursor_mut(),
            parent,
        }
    }

    unsafe fn cursor_mut_from_ptr(
        &mut self,
        ptr: UnsafeIntrusiveEntityRef<T>,
    ) -> EntityListCursorMut<'_, T> {
        let parent = UnsafeIntrusiveEntityRef::into_raw(self.parent()).cast();
        let raw = UnsafeIntrusiveEntityRef::into_inner(ptr).as_ptr();
        EntityListCursorMut {
            cursor: unsafe { self.list.cursor_mut_from_ptr(raw) },
            parent,
        }
    }

    fn back_mut(&mut self) -> EntityListCursorMut<'_, T> {
        let parent = UnsafeIntrusiveEntityRef::into_raw(self.parent()).cast();
        EntityListCursorMut {
            cursor: self.list.back_mut(),
            parent,
        }
    }

    fn front_mut(&mut self) -> EntityListCursorMut<'_, T> {
        let parent = UnsafeIntrusiveEntityRef::into_raw(self.parent()).cast();
        EntityListCursorMut {
            cursor: self.list.front_mut(),
            parent,
        }
    }

    #[track_caller]
    fn remove(cursor: &mut EntityListCursorMut<'_, T>) -> Option<UnsafeIntrusiveEntityRef<T>> {
        let entity = cursor.cursor.remove()?;
        entity.set_parent(None);
        <T as EntityListItem>::on_removed(entity, cursor);
        Some(entity)
    }

    fn replace_with(
        cursor: &mut EntityListCursorMut<'_, T>,
        entity: UnsafeIntrusiveEntityRef<T>,
    ) -> Result<UnsafeIntrusiveEntityRef<T>, UnsafeIntrusiveEntityRef<T>> {
        let removed = cursor.cursor.replace_with(entity)?;
        removed.set_parent(None);
        <T as EntityListItem>::on_removed(removed, cursor);
        let parent = cursor.parent().expect("cannot insert items in an orphaned entity list");
        entity.set_parent(Some(parent));
        <T as EntityListItem>::on_inserted(entity, cursor);
        Ok(removed)
    }

    fn push_front(list: &mut EntityList<T>, entity: UnsafeIntrusiveEntityRef<T>) {
        let parent = list.parent();
        list.list.push_front(entity);
        entity.set_parent(Some(parent));
        <T as EntityListItem>::on_inserted(entity, &mut list.front_mut());
    }

    fn push_back(list: &mut EntityList<T>, entity: UnsafeIntrusiveEntityRef<T>) {
        let parent = list.parent();
        list.list.push_back(entity);
        entity.set_parent(Some(parent));
        <T as EntityListItem>::on_inserted(entity, &mut list.back_mut());
    }

    fn insert_after(cursor: &mut EntityListCursorMut<'_, T>, entity: UnsafeIntrusiveEntityRef<T>) {
        cursor.cursor.insert_after(entity);
        let parent = cursor.parent().expect("cannot insert items in an orphaned entity list");
        entity.set_parent(Some(parent));
        <T as EntityListItem>::on_inserted(entity, cursor);
    }

    fn insert_before(cursor: &mut EntityListCursorMut<'_, T>, entity: UnsafeIntrusiveEntityRef<T>) {
        cursor.cursor.insert_before(entity);
        let parent = cursor.parent().expect("cannot insert items in an orphaned entity list");
        entity.set_parent(Some(parent));
        <T as EntityListItem>::on_inserted(entity, cursor);
    }

    fn splice_after(cursor: &mut EntityListCursorMut<'_, T>, mut list: EntityList<T>) {
        let parent = cursor.parent().expect("cannot insert items in an orphaned entity list");
        while let Some(entity) = list.list.pop_back() {
            cursor.cursor.insert_after(entity);
            entity.set_parent(Some(parent));
            <T as EntityListItem>::on_inserted(entity, cursor);
        }
    }

    fn splice_before(cursor: &mut EntityListCursorMut<'_, T>, mut list: EntityList<T>) {
        let parent = cursor.parent().expect("cannot insert items in an orphaned entity list");
        while let Some(entity) = list.list.pop_front() {
            cursor.cursor.insert_before(entity);
            entity.set_parent(Some(parent));
            <T as EntityListItem>::on_inserted(entity, cursor);
        }
    }

    fn split_after(cursor: &mut EntityListCursorMut<'_, T>) -> EntityList<T> {
        let list = cursor.cursor.split_after();
        let mut list_cursor = list.front();
        while let Some(entity) = list_cursor.clone_pointer() {
            entity.set_parent(None);
            <T as EntityListItem>::on_removed(entity, cursor);
            list_cursor.move_next();
        }
        Self { list }
    }

    fn split_before(cursor: &mut EntityListCursorMut<'_, T>) -> EntityList<T> {
        let list = cursor.cursor.split_before();
        let mut list_cursor = list.front();
        while let Some(entity) = list_cursor.clone_pointer() {
            entity.set_parent(None);
            <T as EntityListItem>::on_removed(entity, cursor);
            list_cursor.move_next();
        }
        Self { list }
    }

    fn pop_front(&mut self) -> Option<UnsafeIntrusiveEntityRef<T>> {
        let removed = self.list.pop_front()?;
        removed.set_parent(None);
        <T as EntityListItem>::on_removed(removed, &mut self.front_mut());
        Some(removed)
    }

    fn pop_back(&mut self) -> Option<UnsafeIntrusiveEntityRef<T>> {
        let removed = self.list.pop_back()?;
        removed.set_parent(None);
        <T as EntityListItem>::on_removed(removed, &mut self.back_mut());
        Some(removed)
    }

    fn clear(&mut self) {
        while self.pop_front().is_some() {}
    }

    #[track_caller]
    fn take(&mut self) -> Self {
        let list = self.list.take();
        let mut list_cursor = list.front();
        while let Some(entity) = list_cursor.clone_pointer() {
            entity.set_parent(None);
            <T as EntityListItem>::on_removed(entity, &mut self.front_mut());
            list_cursor.move_next();
        }
        Self { list }
    }
}

impl<T: EntityListItem> EntityList<T> {
    #[doc(hidden)]
    pub fn cursor_mut(&mut self) -> EntityListCursorMut<'_, T> {
        <Self as EntityListTraits<T>>::cursor_mut(self)
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
    ) -> EntityListCursorMut<'_, T> {
        unsafe { <Self as EntityListTraits<T>>::cursor_mut_from_ptr(self, ptr) }
    }

    /// Get an [EntityListCursorMut] pointing to the first entity in the list, or the null object if
    /// the list is empty
    pub fn front_mut(&mut self) -> EntityListCursorMut<'_, T> {
        <Self as EntityListTraits<T>>::front_mut(self)
    }

    /// Get an [EntityListCursorMut] pointing to the last entity in the list, or the null object if
    /// the list is empty
    pub fn back_mut(&mut self) -> EntityListCursorMut<'_, T> {
        <Self as EntityListTraits<T>>::back_mut(self)
    }

    /// Prepend `entity` to this list
    pub fn push_front(&mut self, entity: UnsafeIntrusiveEntityRef<T>) {
        <Self as EntityListTraits<T>>::push_front(self, entity)
    }

    /// Append `entity` to this list
    pub fn push_back(&mut self, entity: UnsafeIntrusiveEntityRef<T>) {
        <Self as EntityListTraits<T>>::push_back(self, entity)
    }

    /// Remove the entity at the front of the list, returning its [UnsafeIntrusiveEntityRef]
    ///
    /// Returns `None` if the list is empty.
    pub fn pop_front(&mut self) -> Option<UnsafeIntrusiveEntityRef<T>> {
        <Self as EntityListTraits<T>>::pop_front(self)
    }

    /// Remove the entity at the back of the list, returning its [UnsafeIntrusiveEntityRef]
    ///
    /// Returns `None` if the list is empty.
    pub fn pop_back(&mut self) -> Option<UnsafeIntrusiveEntityRef<T>> {
        <Self as EntityListTraits<T>>::pop_back(self)
    }

    /// Removes all items from this list.
    ///
    /// This will unlink all entities currently in the list, which requires iterating through all
    /// elements in the list. If the entities may be used again, this ensures that their intrusive
    /// link is properly unlinked.
    pub fn clear(&mut self) {
        <Self as EntityListTraits<T>>::clear(self)
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
    #[track_caller]
    pub fn take(&mut self) -> Self {
        <Self as EntityListTraits<T>>::take(self)
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

impl<T: EntityListItem> FromIterator<UnsafeIntrusiveEntityRef<T>> for EntityList<T> {
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
    type IntoIter = EntityListIter<'a, T>;
    type Item = EntityRef<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

/// A cursor which provides read-only access to an [EntityList].
pub struct EntityListCursor<'a, T> {
    cursor: intrusive_collections::linked_list::Cursor<'a, EntityAdapter<T>>,
}
impl<'a, T> EntityListCursor<'a, T> {
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
    /// _not_ the [EntityListCursor], since the cursor cannot mutate the list.
    #[track_caller]
    pub fn get(&self) -> Option<EntityRef<'a, T>> {
        Some(self.cursor.get()?.entity.borrow())
    }

    /// Get the [UnsafeIntrusiveEntityRef] corresponding to the entity under the cursor.
    ///
    /// Returns `None` if the cursor is pointing to the null object.
    #[inline]
    pub fn as_pointer(&self) -> Option<UnsafeIntrusiveEntityRef<T>> {
        self.cursor.clone_pointer()
    }

    /// Consume the cursor and convert it into a borrow of the current entity, or `None` if null.
    #[inline]
    #[track_caller]
    pub fn into_borrow(self) -> Option<EntityRef<'a, T>> {
        Some(self.cursor.get()?.borrow())
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
    pub fn peek_next(&self) -> EntityListCursor<'_, T> {
        EntityListCursor {
            cursor: self.cursor.peek_next(),
        }
    }

    /// Returns a cursor pointing to the previous element of the [EntityList].
    ///
    /// If the cursor is pointing to the null object then this will return a cursor pointing to
    /// the last entity in the [EntityList]. If it is pointing to the front of the [EntityList] then
    /// this will return a null cursor.
    #[inline]
    pub fn peek_prev(&self) -> EntityListCursor<'_, T> {
        EntityListCursor {
            cursor: self.cursor.peek_prev(),
        }
    }
}

/// A cursor which provides mutable access to an [EntityList].
pub struct EntityListCursorMut<'a, T> {
    cursor: intrusive_collections::linked_list::CursorMut<'a, EntityAdapter<T>>,
    parent: *const (),
}

impl<T> EntityListCursorMut<'_, T>
where
    T: EntityWithParent,
{
    fn parent(&self) -> Option<UnsafeIntrusiveEntityRef<<T as EntityWithParent>::Parent>> {
        if self.parent.is_null() {
            None
        } else {
            Some(unsafe { UnsafeIntrusiveEntityRef::from_raw(self.parent.cast()) })
        }
    }
}

impl<'a, T: EntityListItem> EntityListCursorMut<'a, T> {
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
    /// The lifetime of the returned [EntityListCursor] is bound to that of the
    /// [EntityListCursorMut], which means it cannot outlive the [EntityListCursorMut] and that the
    /// [EntityListCursorMut] is frozen for the lifetime of the [EntityListCursor].
    pub fn as_cursor(&self) -> EntityListCursor<'_, T> {
        EntityListCursor {
            cursor: self.cursor.as_cursor(),
        }
    }

    /// Get the [UnsafeIntrusiveEntityRef] corresponding to the entity under the cursor.
    ///
    /// Returns `None` if the cursor is pointing to the null object.
    #[inline]
    pub fn as_pointer(&self) -> Option<UnsafeIntrusiveEntityRef<T>> {
        self.cursor.as_cursor().clone_pointer()
    }

    /// Consume the cursor and convert it into a borrow of the current entity, or `None` if null.
    #[inline]
    pub fn into_borrow(self) -> Option<EntityRef<'a, T>> {
        self.cursor.into_ref().map(|item| item.borrow())
    }

    /// Consume the cursor and convert it into a mutable borrow of the current entity, or `None` if null.
    #[inline]
    pub fn into_borrow_mut(self) -> Option<EntityMut<'a, T>> {
        self.cursor.into_ref().map(|item| item.borrow_mut())
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
    pub fn peek_next(&self) -> EntityListCursor<'_, T> {
        EntityListCursor {
            cursor: self.cursor.peek_next(),
        }
    }

    /// Returns a cursor pointing to the previous element of the [EntityList].
    ///
    /// If the cursor is pointing to the null object then this will return a cursor pointing to
    /// the last entity in the [EntityList]. If it is pointing to the front of the [EntityList] then
    /// this will return a null cursor.
    #[inline]
    pub fn peek_prev(&self) -> EntityListCursor<'_, T> {
        EntityListCursor {
            cursor: self.cursor.peek_prev(),
        }
    }

    /// Removes the current entity from the [EntityList].
    ///
    /// A pointer to the element that was removed is returned, and the cursor is moved to point to
    /// the next element in the [EntityList].
    ///
    /// If the cursor is currently pointing to the null object then nothing is removed and `None` is
    /// returned.
    #[inline]
    #[track_caller]
    pub fn remove(&mut self) -> Option<UnsafeIntrusiveEntityRef<T>> {
        <EntityList<T> as EntityListTraits<T>>::remove(self)
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
        <EntityList<T> as EntityListTraits<T>>::replace_with(self, value)
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
        <EntityList<T> as EntityListTraits<T>>::insert_after(self, value)
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
        <EntityList<T> as EntityListTraits<T>>::insert_before(self, value)
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
        <EntityList<T> as EntityListTraits<T>>::splice_after(self, list)
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
        <EntityList<T> as EntityListTraits<T>>::splice_before(self, list)
    }

    /// Splits the list into two after the current cursor position.
    ///
    /// This will return a new list consisting of everything after the cursor, with the original
    /// list retaining everything before.
    ///
    /// If the cursor is pointing at the null object then the entire contents of the [EntityList]
    /// are moved.
    pub fn split_after(&mut self) -> EntityList<T> {
        <EntityList<T> as EntityListTraits<T>>::split_after(self)
    }

    /// Splits the list into two before the current cursor position.
    ///
    /// This will return a new list consisting of everything before the cursor, with the original
    /// list retaining everything after.
    ///
    /// If the cursor is pointing at the null object then the entire contents of the [EntityList]
    /// are moved.
    pub fn split_before(&mut self) -> EntityList<T> {
        <EntityList<T> as EntityListTraits<T>>::split_before(self)
    }
}

pub struct EntityListIter<'a, T> {
    cursor: EntityListCursor<'a, T>,
    started: bool,
}
impl<T> core::iter::FusedIterator for EntityListIter<'_, T> {}
impl<'a, T> Iterator for EntityListIter<'a, T> {
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
impl<T> DoubleEndedIterator for EntityListIter<'_, T> {
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

pub struct MaybeDefaultEntityListIter<'a, T> {
    iter: Option<EntityListIter<'a, T>>,
}
impl<T> Default for MaybeDefaultEntityListIter<'_, T> {
    fn default() -> Self {
        Self { iter: None }
    }
}
impl<T> core::iter::FusedIterator for MaybeDefaultEntityListIter<'_, T> {}
impl<'a, T> Iterator for MaybeDefaultEntityListIter<'a, T> {
    type Item = EntityRef<'a, T>;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        self.iter.as_mut().and_then(|iter| iter.next())
    }
}
impl<T> DoubleEndedIterator for MaybeDefaultEntityListIter<'_, T> {
    #[inline]
    fn next_back(&mut self) -> Option<Self::Item> {
        self.iter.as_mut().and_then(|iter| iter.next_back())
    }
}

impl<T: 'static> RawEntityRef<T, IntrusiveLink> {
    /// Create a new [UnsafeIntrusiveEntityRef] by allocating `value` in `arena`
    ///
    /// # SAFETY
    ///
    /// This function has the same requirements around safety as [RawEntityRef::new].
    pub fn new(value: T, arena: &blink_alloc::Blink) -> Self {
        RawEntityRef::new_with_metadata(value, IntrusiveLink::default(), arena)
    }

    pub fn new_uninit(arena: &blink_alloc::Blink) -> RawEntityRef<MaybeUninit<T>, IntrusiveLink> {
        RawEntityRef::new_uninit_with_metadata(IntrusiveLink::default(), arena)
    }
}

impl<T> RawEntityRef<T, IntrusiveLink>
where
    T: EntityWithParent,
{
    /// Returns the parent entity this entity is linked to, if linked.
    pub fn parent(&self) -> Option<UnsafeIntrusiveEntityRef<<T as EntityWithParent>::Parent>> {
        unsafe {
            let offset = core::mem::offset_of!(RawEntityMetadata<T, IntrusiveLink>, metadata);
            let current = self.inner.byte_add(offset).cast::<IntrusiveLink>();
            current.as_ref().parent()
        }
    }

    pub(self) fn set_parent(
        &self,
        parent: Option<UnsafeIntrusiveEntityRef<<T as EntityWithParent>::Parent>>,
    ) {
        unsafe {
            let offset = core::mem::offset_of!(RawEntityMetadata<T, IntrusiveLink>, metadata);
            let current = self.inner.byte_add(offset).cast::<IntrusiveLink>();
            current.as_ref().set_parent(parent);
        }
    }
}

impl<T> RawEntityRef<T, IntrusiveLink>
where
    T: EntityWithParent,
    <T as EntityWithParent>::Parent: EntityWithParent,
{
    pub fn grandparent(
        &self,
    ) -> Option<
        UnsafeIntrusiveEntityRef<<<T as EntityWithParent>::Parent as EntityWithParent>::Parent>,
    > {
        self.parent().and_then(|parent| parent.parent())
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
            let current = self.inner.byte_add(offset).cast::<IntrusiveLink>();
            if !current.as_ref().is_linked() {
                return None;
            }
            LinkOps.prev(current.cast()).map(|link_ptr| {
                let offset = core::mem::offset_of!(IntrusiveLink, link);
                let link_ptr = link_ptr.byte_sub(offset).cast::<IntrusiveLink>();
                Self::from_link_ptr(link_ptr)
            })
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
            let current = self.inner.byte_add(offset).cast::<IntrusiveLink>();
            if !current.as_ref().is_linked() {
                return None;
            }
            LinkOps.next(current.cast()).map(|link_ptr| {
                let offset = core::mem::offset_of!(IntrusiveLink, link);
                let link_ptr = link_ptr.byte_sub(offset).cast::<IntrusiveLink>();
                Self::from_link_ptr(link_ptr)
            })
        }
    }

    #[inline]
    unsafe fn from_link_ptr(link: NonNull<IntrusiveLink>) -> Self {
        let offset = core::mem::offset_of!(RawEntityMetadata<T, IntrusiveLink>, metadata);
        let ptr = unsafe { link.byte_sub(offset).cast::<RawEntityMetadata<T, IntrusiveLink>>() };
        Self { inner: ptr }
    }
}

/// A trait implemented by any [super::Entity] that is storeable in an [EntityList].
///
/// This trait defines callbacks that are executed any time the entity is added, removed, or
/// transferred between collections.
///
/// By default, these callbacks are no-ops.
#[allow(unused_variables)]
pub trait EntityListItem: Sized + super::Entity {
    /// Invoked when this entity type is inserted into an intrusive list
    #[inline]
    fn on_inserted(
        this: UnsafeIntrusiveEntityRef<Self>,
        cursor: &mut EntityListCursorMut<'_, Self>,
    ) {
    }
    /// Invoked when this entity type is removed from an intrusive list
    #[inline]
    fn on_removed(this: UnsafeIntrusiveEntityRef<Self>, list: &mut EntityListCursorMut<'_, Self>) {}
    /// Invoked when a set of entities is moved from one intrusive list to another
    #[inline]
    fn on_transfer(
        this: UnsafeIntrusiveEntityRef<Self>,
        from: &mut EntityList<Self>,
        to: &mut EntityList<Self>,
    ) {
    }
}

impl<T: Sized + super::Entity> EntityListItem for T {
    default fn on_inserted(
        _this: UnsafeIntrusiveEntityRef<Self>,
        _list: &mut EntityListCursorMut<'_, Self>,
    ) {
    }

    default fn on_removed(
        _this: UnsafeIntrusiveEntityRef<Self>,
        _list: &mut EntityListCursorMut<'_, Self>,
    ) {
    }

    default fn on_transfer(
        _this: UnsafeIntrusiveEntityRef<Self>,
        _from: &mut EntityList<Self>,
        _to: &mut EntityList<Self>,
    ) {
    }
}

unsafe impl<T> intrusive_collections::Adapter
    for super::adapter::EntityAdapter<T, IntrusiveLink, intrusive_collections::linked_list::LinkOps>
{
    type LinkOps = intrusive_collections::linked_list::LinkOps;
    type PointerOps = super::adapter::DefaultPointerOps<RawEntityRef<T, IntrusiveLink>>;

    unsafe fn get_value(
        &self,
        link: <Self::LinkOps as intrusive_collections::LinkOps>::LinkPtr,
    ) -> *const <Self::PointerOps as intrusive_collections::PointerOps>::Value {
        let offset = core::mem::offset_of!(IntrusiveLink, link);
        unsafe {
            let link_ptr = link.byte_sub(offset).cast::<IntrusiveLink>();
            let raw_entity_ref = RawEntityRef::<T, IntrusiveLink>::from_link_ptr(link_ptr);
            raw_entity_ref.inner.as_ptr().cast_const()
        }
    }

    unsafe fn get_link(
        &self,
        value: *const <Self::PointerOps as intrusive_collections::PointerOps>::Value,
    ) -> <Self::LinkOps as intrusive_collections::LinkOps>::LinkPtr {
        let raw_entity_ref = unsafe { RawEntityRef::from_ptr(value.cast_mut()) };
        let offset = RawEntityMetadata::<T, IntrusiveLink>::metadata_offset();
        unsafe { raw_entity_ref.inner.byte_add(offset).cast() }
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
