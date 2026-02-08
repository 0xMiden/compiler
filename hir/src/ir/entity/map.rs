use alloc::borrow::Borrow;
use core::{cell::Cell, fmt, hash::Hash, mem::MaybeUninit, ptr::NonNull};

use intrusive_collections::KeyAdapter;

use super::{
    EntityMut, EntityParent, EntityRef, EntityWithParent, RawEntityMetadata, RawEntityRef,
    UnsafeIntrusiveMapEntityRef,
};

type EntityAdapter<T> =
    super::adapter::EntityAdapter<T, IntrusiveLink, intrusive_collections::rbtree::LinkOps>;

pub trait EntityWithKey: super::Entity {
    type Key: Eq + Ord + Hash + Clone + 'static;
    type Value;

    fn key(&self) -> Self::Key;
    fn value(&self) -> &Self::Value;
}

impl<'a, T: EntityWithKey> KeyAdapter<'a> for EntityAdapter<T> {
    type Key = <T as EntityWithKey>::Key;

    fn get_key(&self, s: &'a RawEntityMetadata<T, IntrusiveLink>) -> Self::Key {
        s.borrow().key()
    }
}

pub struct IntrusiveLink {
    link: intrusive_collections::rbtree::Link,
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
    pub(self) fn set_parent<T>(&self, parent: Option<UnsafeIntrusiveMapEntityRef<T>>) {
        if let Some(parent) = parent {
            assert!(
                self.link.is_linked(),
                "must add entity to parent entity map before setting parent"
            );
            self.parent.set(UnsafeIntrusiveMapEntityRef::as_ptr(&parent).cast());
        } else if self.parent.get().is_null() {
            panic!("no parent previously set");
        } else {
            self.parent.set(core::ptr::null());
        }
    }

    pub fn parent<T>(&self) -> Option<UnsafeIntrusiveMapEntityRef<T>> {
        let parent = self.parent.get();
        if parent.is_null() {
            // EntityMap is orphaned
            None
        } else {
            Some(unsafe { UnsafeIntrusiveMapEntityRef::from_raw(parent.cast()) })
        }
    }
}

pub struct EntityMap<T> {
    map: intrusive_collections::rbtree::RBTree<EntityAdapter<T>>,
}

impl<T: EntityWithKey> Eq for EntityMap<T>
where
    <T as EntityWithKey>::Value: Eq,
    for<'a> EntityAdapter<T>: KeyAdapter<'a, Key = <T as EntityWithKey>::Key>,
{
}
impl<T: EntityWithKey> PartialEq for EntityMap<T>
where
    <T as EntityWithKey>::Value: PartialEq,
    for<'a> EntityAdapter<T>: KeyAdapter<'a, Key = <T as EntityWithKey>::Key>,
{
    fn eq(&self, other: &Self) -> bool {
        if self.len() != other.len() {
            return false;
        }
        let mut lhs = self.front();
        while let Some(lentry) = lhs.get() {
            let key = lentry.key();
            let rentry = { other.find(&key).as_pointer() };
            if let Some(rentry) = rentry {
                let rentry = rentry.borrow();
                let lvalue = <T as EntityWithKey>::value(&lentry);
                let rvalue = <T as EntityWithKey>::value(&rentry);
                if !lvalue.eq(rvalue) {
                    return false;
                }
            } else {
                return false;
            }
            lhs.move_next();
        }
        true
    }
}
impl<T: core::hash::Hash> core::hash::Hash for EntityMap<T> {
    fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
        for item in self.map.iter() {
            item.borrow().hash(state);
        }
    }
}
impl<T> Default for EntityMap<T> {
    fn default() -> Self {
        Self {
            map: Default::default(),
        }
    }
}
impl<T> EntityMap<T> {
    /// Construct a new, empty [EntityMap]
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns true if this map is empty
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// Returns the number of entities in this map
    pub fn len(&self) -> usize {
        let mut cursor = self.map.front();
        let mut usize = 0;
        while !cursor.is_null() {
            usize += 1;
            cursor.move_next();
        }
        usize
    }

    #[doc(hidden)]
    pub fn cursor(&self) -> EntityMapCursor<'_, T> {
        EntityMapCursor {
            cursor: self.map.cursor(),
        }
    }

    /// Get an [EntityCursor] pointing to the first entity in the map, or the null object if
    /// the map is empty
    pub fn front(&self) -> EntityMapCursor<'_, T> {
        EntityMapCursor {
            cursor: self.map.front(),
        }
    }

    /// Get an [EntityCursor] pointing to the last entity in the map, or the null object if
    /// the map is empty
    pub fn back(&self) -> EntityMapCursor<'_, T> {
        EntityMapCursor {
            cursor: self.map.back(),
        }
    }

    /// Get an iterator over the entities in this map
    ///
    /// The iterator returned produces [EntityRef]s for each item in the map, with their lifetime
    /// bound to the map itself, not the iterator.
    pub fn iter(&self) -> EntityMapIter<'_, T> {
        EntityMapIter {
            cursor: self.cursor(),
            started: false,
        }
    }

    /// Get a cursor to the item pointed to by `ptr`.
    ///
    /// # Safety
    ///
    /// This function may only be called when it is known that `ptr` refers to an entity which is
    /// linked into this map. This operation will panic if the entity is not linked into any map,
    /// and may result in undefined behavior if the operation is linked into a different map.
    pub unsafe fn cursor_from_ptr(
        &self,
        ptr: UnsafeIntrusiveMapEntityRef<T>,
    ) -> EntityMapCursor<'_, T> {
        unsafe {
            let raw = UnsafeIntrusiveMapEntityRef::into_inner(ptr).as_ptr();
            EntityMapCursor {
                cursor: self.map.cursor_from_ptr(raw),
            }
        }
    }
}

impl<T> EntityMap<T>
where
    T: EntityWithParent,
{
    pub(crate) fn parent(&self) -> UnsafeIntrusiveMapEntityRef<<T as EntityWithParent>::Parent> {
        let offset = <<T as EntityWithParent>::Parent as EntityParent<T>>::offset();
        let ptr = self as *const EntityMap<T>;
        unsafe {
            let parent = ptr.byte_sub(offset).cast::<<T as EntityWithParent>::Parent>();
            UnsafeIntrusiveMapEntityRef::from_raw(parent)
        }
    }
}

trait EntityMapTraits<T: EntityWithKey>: Sized {
    fn cursor_mut(&mut self) -> EntityMapCursorMut<'_, T>;

    /// Get a mutable cursor to the item pointed to by `ptr`.
    ///
    /// # Safety
    ///
    /// This function may only be called when it is known that `ptr` refers to an entity which is
    /// linked into this map. This operation will panic if the entity is not linked into any map,
    /// and may result in undefined behavior if the operation is linked into a different map.
    unsafe fn cursor_mut_from_ptr(
        &mut self,
        ptr: UnsafeIntrusiveMapEntityRef<T>,
    ) -> EntityMapCursorMut<'_, T>;

    /// Get an [EntityCursorMut] pointing to the first entity in the map, or the null object if
    /// the map is empty
    fn front_mut(&mut self) -> EntityMapCursorMut<'_, T>;

    /// Get an [EntityCursorMut] pointing to the last entity in the map, or the null object if
    /// the map is empty
    fn back_mut(&mut self) -> EntityMapCursorMut<'_, T>;

    fn find<'b, 'a: 'b, Q>(&'a self, key: &Q) -> EntityMapCursor<'a, T>
    where
        Q: ?Sized + Ord,
        <EntityAdapter<T> as KeyAdapter<'b>>::Key: Borrow<Q>;

    fn find_mut<'b, 'a: 'b, Q>(&'a mut self, key: &Q) -> EntityMapCursorMut<'a, T>
    where
        Q: ?Sized + Ord,
        <EntityAdapter<T> as KeyAdapter<'b>>::Key: Borrow<Q>;

    fn remove(cursor: &mut EntityMapCursorMut<'_, T>) -> Option<UnsafeIntrusiveMapEntityRef<T>>;

    fn replace_with(
        cursor: &mut EntityMapCursorMut<'_, T>,
        entity: UnsafeIntrusiveMapEntityRef<T>,
    ) -> Result<UnsafeIntrusiveMapEntityRef<T>, UnsafeIntrusiveMapEntityRef<T>>;

    /// Insert `entity` into this map
    fn insert(map: &mut EntityMap<T>, entity: UnsafeIntrusiveMapEntityRef<T>);

    /// Removes all items from this map.
    ///
    /// This will unlink all entities currently in the map, which requires iterating through all
    /// elements in the map. If the entities may be used again, this ensures that their intrusive
    /// link is properly unlinked.
    fn clear(&mut self);

    /// Takes all the elements out of the [EntityMap], leaving it empty.
    ///
    /// The taken elements are returned as a new [EntityMap].
    fn take(&mut self) -> Self;
}

impl<T: EntityMapItem> EntityMapTraits<T> for EntityMap<T> {
    default fn cursor_mut(&mut self) -> EntityMapCursorMut<'_, T> {
        EntityMapCursorMut {
            cursor: self.map.cursor_mut(),
            parent: core::ptr::null(),
        }
    }

    default unsafe fn cursor_mut_from_ptr(
        &mut self,
        ptr: UnsafeIntrusiveMapEntityRef<T>,
    ) -> EntityMapCursorMut<'_, T> {
        let raw = UnsafeIntrusiveMapEntityRef::into_inner(ptr).as_ptr();
        unsafe {
            EntityMapCursorMut {
                cursor: self.map.cursor_mut_from_ptr(raw),
                parent: core::ptr::null(),
            }
        }
    }

    /// Get an [EntityCursorMut] pointing to the first entity in the map, or the null object if
    /// the map is empty
    default fn front_mut(&mut self) -> EntityMapCursorMut<'_, T> {
        EntityMapCursorMut {
            cursor: self.map.front_mut(),
            parent: core::ptr::null(),
        }
    }

    /// Get an [EntityCursorMut] pointing to the last entity in the map, or the null object if
    /// the map is empty
    default fn back_mut(&mut self) -> EntityMapCursorMut<'_, T> {
        EntityMapCursorMut {
            cursor: self.map.back_mut(),
            parent: core::ptr::null(),
        }
    }

    default fn find<'b, 'a: 'b, Q>(&'a self, key: &Q) -> EntityMapCursor<'a, T>
    where
        Q: ?Sized + Ord,
        <EntityAdapter<T> as KeyAdapter<'b>>::Key: Borrow<Q>,
    {
        EntityMapCursor {
            cursor: self.map.find(key),
        }
    }

    default fn find_mut<'b, 'a: 'b, Q>(&'a mut self, key: &Q) -> EntityMapCursorMut<'a, T>
    where
        Q: ?Sized + Ord,
        <EntityAdapter<T> as KeyAdapter<'b>>::Key: Borrow<Q>,
    {
        EntityMapCursorMut {
            cursor: self.map.find_mut(key),
            parent: core::ptr::null(),
        }
    }

    default fn remove(
        cursor: &mut EntityMapCursorMut<'_, T>,
    ) -> Option<UnsafeIntrusiveMapEntityRef<T>> {
        let entity = cursor.cursor.remove()?;
        <T as EntityMapItem>::on_removed(entity, cursor);
        Some(entity)
    }

    default fn replace_with(
        cursor: &mut EntityMapCursorMut<'_, T>,
        entity: UnsafeIntrusiveMapEntityRef<T>,
    ) -> Result<UnsafeIntrusiveMapEntityRef<T>, UnsafeIntrusiveMapEntityRef<T>> {
        let removed = cursor.cursor.replace_with(entity)?;
        <T as EntityMapItem>::on_removed(removed, cursor);
        <T as EntityMapItem>::on_inserted(entity, cursor);
        Ok(removed)
    }

    default fn insert(map: &mut EntityMap<T>, entity: UnsafeIntrusiveMapEntityRef<T>) {
        let cursor = map.map.insert(entity);
        let mut cursor = EntityMapCursorMut {
            cursor,
            parent: core::ptr::null(),
        };
        <T as EntityMapItem>::on_inserted(entity, &mut cursor);
    }

    default fn clear(&mut self) {
        let mut cursor = self.front_mut();
        while !cursor.is_null() {
            Self::remove(&mut cursor);
        }
    }

    default fn take(&mut self) -> Self {
        let map = self.map.take();
        let mut cursor = map.front();
        while let Some(entity) = cursor.clone_pointer() {
            <T as EntityMapItem>::on_removed(entity, &mut self.front_mut());
            cursor.move_next();
        }
        Self { map }
    }
}

impl<T> EntityMapTraits<T> for EntityMap<T>
where
    T: EntityMapItem + EntityWithParent,
    <T as EntityWithParent>::Parent: EntityParent<T>,
{
    fn cursor_mut(&mut self) -> EntityMapCursorMut<'_, T> {
        let parent = UnsafeIntrusiveMapEntityRef::into_raw(self.parent()).cast();
        EntityMapCursorMut {
            cursor: self.map.cursor_mut(),
            parent,
        }
    }

    unsafe fn cursor_mut_from_ptr(
        &mut self,
        ptr: UnsafeIntrusiveMapEntityRef<T>,
    ) -> EntityMapCursorMut<'_, T> {
        let parent = UnsafeIntrusiveMapEntityRef::into_raw(self.parent()).cast();
        let raw = UnsafeIntrusiveMapEntityRef::into_inner(ptr).as_ptr();
        EntityMapCursorMut {
            cursor: unsafe { self.map.cursor_mut_from_ptr(raw) },
            parent,
        }
    }

    fn back_mut(&mut self) -> EntityMapCursorMut<'_, T> {
        let parent = UnsafeIntrusiveMapEntityRef::into_raw(self.parent()).cast();
        EntityMapCursorMut {
            cursor: self.map.back_mut(),
            parent,
        }
    }

    fn front_mut(&mut self) -> EntityMapCursorMut<'_, T> {
        let parent = UnsafeIntrusiveMapEntityRef::into_raw(self.parent()).cast();
        EntityMapCursorMut {
            cursor: self.map.front_mut(),
            parent,
        }
    }

    fn find<'b, 'a: 'b, Q>(&'a self, key: &Q) -> EntityMapCursor<'a, T>
    where
        Q: ?Sized + Ord,
        <EntityAdapter<T> as KeyAdapter<'b>>::Key: Borrow<Q>,
    {
        EntityMapCursor {
            cursor: self.map.find(key),
        }
    }

    fn find_mut<'b, 'a: 'b, Q>(&'a mut self, key: &Q) -> EntityMapCursorMut<'a, T>
    where
        Q: ?Sized + Ord,
        <EntityAdapter<T> as KeyAdapter<'b>>::Key: Borrow<Q>,
    {
        let parent = UnsafeIntrusiveMapEntityRef::into_raw(self.parent()).cast();
        EntityMapCursorMut {
            cursor: self.map.find_mut(key),
            parent,
        }
    }

    #[track_caller]
    fn remove(cursor: &mut EntityMapCursorMut<'_, T>) -> Option<UnsafeIntrusiveMapEntityRef<T>> {
        let entity = cursor.cursor.remove()?;
        entity.set_parent(None);
        <T as EntityMapItem>::on_removed(entity, cursor);
        Some(entity)
    }

    fn replace_with(
        cursor: &mut EntityMapCursorMut<'_, T>,
        entity: UnsafeIntrusiveMapEntityRef<T>,
    ) -> Result<UnsafeIntrusiveMapEntityRef<T>, UnsafeIntrusiveMapEntityRef<T>> {
        let removed = cursor.cursor.replace_with(entity)?;
        removed.set_parent(None);
        <T as EntityMapItem>::on_removed(removed, cursor);
        let parent = cursor.parent().expect("cannot insert items in an orphaned entity map");
        entity.set_parent(Some(parent));
        <T as EntityMapItem>::on_inserted(entity, cursor);
        Ok(removed)
    }

    fn insert(map: &mut EntityMap<T>, entity: UnsafeIntrusiveMapEntityRef<T>) {
        let parent = map.parent();
        let cursor = map.map.insert(entity);
        entity.set_parent(Some(parent));
        let parent = UnsafeIntrusiveMapEntityRef::into_raw(parent).cast();
        let mut cursor = EntityMapCursorMut { cursor, parent };
        <T as EntityMapItem>::on_inserted(entity, &mut cursor);
    }

    fn clear(&mut self) {
        let mut cursor = self.front_mut();
        while !cursor.is_null() {
            Self::remove(&mut cursor);
        }
    }

    #[track_caller]
    fn take(&mut self) -> Self {
        let map = self.map.take();
        let mut map_cursor = map.front();
        while let Some(entity) = map_cursor.clone_pointer() {
            entity.set_parent(None);
            <T as EntityMapItem>::on_removed(entity, &mut self.front_mut());
            map_cursor.move_next();
        }
        Self { map }
    }
}

impl<T: EntityMapItem> EntityMap<T> {
    #[doc(hidden)]
    pub fn cursor_mut(&mut self) -> EntityMapCursorMut<'_, T> {
        <Self as EntityMapTraits<T>>::cursor_mut(self)
    }

    /// Get a mutable cursor to the item pointed to by `ptr`.
    ///
    /// # Safety
    ///
    /// This function may only be called when it is known that `ptr` refers to an entity which is
    /// linked into this map. This operation will panic if the entity is not linked into any map,
    /// and may result in undefined behavior if the operation is linked into a different map.
    pub unsafe fn cursor_mut_from_ptr(
        &mut self,
        ptr: UnsafeIntrusiveMapEntityRef<T>,
    ) -> EntityMapCursorMut<'_, T> {
        unsafe { <Self as EntityMapTraits<T>>::cursor_mut_from_ptr(self, ptr) }
    }

    /// Get an [EntityCursorMut] pointing to the first entity in the map, or the null object if
    /// the map is empty
    pub fn front_mut(&mut self) -> EntityMapCursorMut<'_, T> {
        <Self as EntityMapTraits<T>>::front_mut(self)
    }

    /// Get an [EntityCursorMut] pointing to the last entity in the map, or the null object if
    /// the map is empty
    pub fn back_mut(&mut self) -> EntityMapCursorMut<'_, T> {
        <Self as EntityMapTraits<T>>::back_mut(self)
    }

    /// Insert `entity` into this map
    pub fn insert(&mut self, entity: UnsafeIntrusiveMapEntityRef<T>) {
        <Self as EntityMapTraits<T>>::insert(self, entity)
    }

    /// Removes all items from this map.
    ///
    /// This will unlink all entities currently in the map, which requires iterating through all
    /// elements in the map. If the entities may be used again, this ensures that their intrusive
    /// link is properly unlinked.
    pub fn clear(&mut self) {
        <Self as EntityMapTraits<T>>::clear(self)
    }

    /// Empties the map without properly unlinking the intrusive links of the items in the map.
    ///
    /// Since this does not unlink any objects, any attempts to link these objects into another
    /// [EntityMap] will fail but will not cause any memory unsafety. To unlink those objects
    /// manually, you must call the `force_unlink` function on the link.
    pub fn fast_clear(&mut self) {
        self.map.fast_clear();
    }

    /// Takes all the elements out of the [EntityMap], leaving it empty.
    ///
    /// The taken elements are returned as a new [EntityMap].
    #[track_caller]
    pub fn take(&mut self) -> Self {
        <Self as EntityMapTraits<T>>::take(self)
    }

    /// Returns true if `key` is present in this map
    pub fn contains<'a, Q>(&self, key: &Q) -> bool
    where
        Q: ?Sized + Ord,
        <EntityAdapter<T> as KeyAdapter<'a>>::Key: Borrow<Q>,
    {
        !self.map.find(key).is_null()
    }

    /// Get a cursor to the entry in this map stored under `key`.
    ///
    /// If no such entry exists, the null cursor is returned.
    pub fn find<'b, 'a: 'b, Q>(&'a self, key: &Q) -> EntityMapCursor<'a, T>
    where
        Q: ?Sized + Ord,
        <EntityAdapter<T> as KeyAdapter<'b>>::Key: Borrow<Q>,
    {
        <Self as EntityMapTraits<T>>::find(self, key)
    }

    /// Get a mutable cursor to the entry in this map stored under `key`.
    ///
    /// If no such entry exists, the null cursor is returned.
    pub fn find_mut<'b, 'a: 'b, Q>(&'a mut self, key: &Q) -> EntityMapCursorMut<'a, T>
    where
        Q: ?Sized + Ord,
        <EntityAdapter<T> as KeyAdapter<'b>>::Key: Borrow<Q>,
    {
        <Self as EntityMapTraits<T>>::find_mut(self, key)
    }
}

impl<T: EntityWithKey> fmt::Debug for EntityMap<T>
where
    <T as EntityWithKey>::Key: fmt::Debug,
    <T as EntityWithKey>::Value: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut builder = f.debug_map();
        for entity in self.iter() {
            builder.entry(&entity.key(), &entity.value());
        }
        builder.finish()
    }
}

impl<T: EntityMapItem> FromIterator<UnsafeIntrusiveMapEntityRef<T>> for EntityMap<T> {
    fn from_iter<I>(iter: I) -> Self
    where
        I: IntoIterator<Item = UnsafeIntrusiveMapEntityRef<T>>,
    {
        let mut map = EntityMap::<T>::default();
        for handle in iter {
            map.insert(handle);
        }
        map
    }
}

impl<T> IntoIterator for EntityMap<T> {
    type IntoIter = intrusive_collections::rbtree::IntoIter<EntityAdapter<T>>;
    type Item = UnsafeIntrusiveMapEntityRef<T>;

    fn into_iter(self) -> Self::IntoIter {
        self.map.into_iter()
    }
}

impl<'a, T> IntoIterator for &'a EntityMap<T> {
    type IntoIter = EntityMapIter<'a, T>;
    type Item = EntityRef<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

/// A cursor which provides read-only access to an [EntityMap].
pub struct EntityMapCursor<'a, T> {
    cursor: intrusive_collections::rbtree::Cursor<'a, EntityAdapter<T>>,
}
impl<'a, T> EntityMapCursor<'a, T> {
    /// Returns true if this cursor is pointing to the null object
    #[inline]
    pub fn is_null(&self) -> bool {
        self.cursor.is_null()
    }

    /// Get a shared reference to the entity under the cursor.
    ///
    /// Returns `None` if the cursor is currently pointing to the null object.
    ///
    /// NOTE: This returns an [EntityRef] whose lifetime is bound to the underlying [EntityMap],
    /// _not_ the [EntityCursor], since the cursor cannot mutate the map.
    #[track_caller]
    pub fn get(&self) -> Option<EntityRef<'a, T>> {
        Some(self.cursor.get()?.entity.borrow())
    }

    /// Get the [UnsafeIntrusiveMapEntityRef] corresponding to the entity under the cursor.
    ///
    /// Returns `None` if the cursor is pointing to the null object.
    #[inline]
    pub fn as_pointer(&self) -> Option<UnsafeIntrusiveMapEntityRef<T>> {
        self.cursor.clone_pointer()
    }

    /// Consume the cursor and convert it into a borrow of the current entity, or `None` if null.
    #[inline]
    #[track_caller]
    pub fn into_borrow(self) -> Option<EntityRef<'a, T>> {
        Some(self.cursor.get()?.borrow())
    }

    /// Moves the cursor to the next element of the [EntityMap].
    ///
    /// If the cursor is pointing to the null object then this will move it to the front of the
    /// [EntityMap]. If it is pointing to the back of the [EntityMap] then this will move it to
    /// the null object.
    #[inline]
    pub fn move_next(&mut self) {
        self.cursor.move_next();
    }

    /// Moves the cursor to the previous element of the [EntityMap].
    ///
    /// If the cursor is pointing to the null object then this will move it to the back of the
    /// [EntityMap]. If it is pointing to the front of the [EntityMap] then this will move it to
    /// the null object.
    #[inline]
    pub fn move_prev(&mut self) {
        self.cursor.move_prev();
    }

    /// Returns a cursor pointing to the next element of the [EntityMap].
    ///
    /// If the cursor is pointing to the null object then this will return a cursor pointing to the
    /// front of the [EntityMap]. If it is pointing to the last entity of the [EntityMap] then
    /// this will return a null cursor.
    #[inline]
    pub fn peek_next(&self) -> EntityMapCursor<'_, T> {
        EntityMapCursor {
            cursor: self.cursor.peek_next(),
        }
    }

    /// Returns a cursor pointing to the previous element of the [EntityMap].
    ///
    /// If the cursor is pointing to the null object then this will return a cursor pointing to
    /// the last entity in the [EntityMap]. If it is pointing to the front of the [EntityMap] then
    /// this will return a null cursor.
    #[inline]
    pub fn peek_prev(&self) -> EntityMapCursor<'_, T> {
        EntityMapCursor {
            cursor: self.cursor.peek_prev(),
        }
    }
}

/// A cursor which provides mutable access to an [EntityMap].
pub struct EntityMapCursorMut<'a, T> {
    cursor: intrusive_collections::rbtree::CursorMut<'a, EntityAdapter<T>>,
    parent: *const (),
}

impl<T> EntityMapCursorMut<'_, T>
where
    T: EntityWithParent,
{
    fn parent(&self) -> Option<UnsafeIntrusiveMapEntityRef<<T as EntityWithParent>::Parent>> {
        if self.parent.is_null() {
            None
        } else {
            Some(unsafe { UnsafeIntrusiveMapEntityRef::from_raw(self.parent.cast()) })
        }
    }
}

impl<'a, T: EntityMapItem> EntityMapCursorMut<'a, T> {
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
    pub fn as_cursor(&self) -> EntityMapCursor<'_, T> {
        EntityMapCursor {
            cursor: self.cursor.as_cursor(),
        }
    }

    /// Get the [UnsafeIntrusiveMapEntityRef] corresponding to the entity under the cursor.
    ///
    /// Returns `None` if the cursor is pointing to the null object.
    #[inline]
    pub fn as_pointer(&self) -> Option<UnsafeIntrusiveMapEntityRef<T>> {
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

    /// Moves the cursor to the next element of the [EntityMap].
    ///
    /// If the cursor is pointing to the null object then this will move it to the front of the
    /// [EntityMap]. If it is pointing to the back of the [EntityMap] then this will move it to
    /// the null object.
    #[inline]
    pub fn move_next(&mut self) {
        self.cursor.move_next();
    }

    /// Moves the cursor to the previous element of the [EntityMap].
    ///
    /// If the cursor is pointing to the null object then this will move it to the back of the
    /// [EntityMap]. If it is pointing to the front of the [EntityMap] then this will move it to
    /// the null object.
    #[inline]
    pub fn move_prev(&mut self) {
        self.cursor.move_prev();
    }

    /// Returns a cursor pointing to the next element of the [EntityMap].
    ///
    /// If the cursor is pointing to the null object then this will return a cursor pointing to the
    /// front of the [EntityMap]. If it is pointing to the last entity of the [EntityMap] then
    /// this will return a null cursor.
    #[inline]
    pub fn peek_next(&self) -> EntityMapCursor<'_, T> {
        EntityMapCursor {
            cursor: self.cursor.peek_next(),
        }
    }

    /// Returns a cursor pointing to the previous element of the [EntityMap].
    ///
    /// If the cursor is pointing to the null object then this will return a cursor pointing to
    /// the last entity in the [EntityMap]. If it is pointing to the front of the [EntityMap] then
    /// this will return a null cursor.
    #[inline]
    pub fn peek_prev(&self) -> EntityMapCursor<'_, T> {
        EntityMapCursor {
            cursor: self.cursor.peek_prev(),
        }
    }

    /// Removes the current entity from the [EntityMap].
    ///
    /// A pointer to the element that was removed is returned, and the cursor is moved to point to
    /// the next element in the [Entitymap].
    ///
    /// If the cursor is currently pointing to the null object then nothing is removed and `None` is
    /// returned.
    #[inline]
    #[track_caller]
    pub fn remove(&mut self) -> Option<UnsafeIntrusiveMapEntityRef<T>> {
        <EntityMap<T> as EntityMapTraits<T>>::remove(self)
    }

    /// Removes the current entity from the [EntityMap] and inserts another one in its place.
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
        value: UnsafeIntrusiveMapEntityRef<T>,
    ) -> Result<UnsafeIntrusiveMapEntityRef<T>, UnsafeIntrusiveMapEntityRef<T>> {
        <EntityMap<T> as EntityMapTraits<T>>::replace_with(self, value)
    }
}

pub struct EntityMapIter<'a, T> {
    cursor: EntityMapCursor<'a, T>,
    started: bool,
}
impl<T> core::iter::FusedIterator for EntityMapIter<'_, T> {}
impl<'a, T> Iterator for EntityMapIter<'a, T> {
    type Item = EntityRef<'a, T>;

    fn next(&mut self) -> Option<Self::Item> {
        // If we haven't started iterating yet, then we're on the null cursor, so move to the
        // front of the map now that we have started iterating.
        if !self.started {
            self.started = true;
            self.cursor.move_next();
        }
        let item = self.cursor.get()?;
        self.cursor.move_next();
        Some(item)
    }
}
impl<T> DoubleEndedIterator for EntityMapIter<'_, T> {
    fn next_back(&mut self) -> Option<Self::Item> {
        // If we haven't started iterating yet, then we're on the null cursor, so move to the
        // back of the map now that we have started iterating.
        if !self.started {
            self.started = true;
            self.cursor.move_prev();
        }
        let item = self.cursor.get()?;
        self.cursor.move_prev();
        Some(item)
    }
}

pub struct MaybeDefaultEntityMapIter<'a, T> {
    iter: Option<EntityMapIter<'a, T>>,
}
impl<T> Default for MaybeDefaultEntityMapIter<'_, T> {
    fn default() -> Self {
        Self { iter: None }
    }
}
impl<T> core::iter::FusedIterator for MaybeDefaultEntityMapIter<'_, T> {}
impl<'a, T> Iterator for MaybeDefaultEntityMapIter<'a, T> {
    type Item = EntityRef<'a, T>;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        self.iter.as_mut().and_then(|iter| iter.next())
    }
}
impl<T> DoubleEndedIterator for MaybeDefaultEntityMapIter<'_, T> {
    #[inline]
    fn next_back(&mut self) -> Option<Self::Item> {
        self.iter.as_mut().and_then(|iter| iter.next_back())
    }
}

impl<T: 'static> RawEntityRef<T, IntrusiveLink> {
    /// Create a new [UnsafeIntrusiveMapEntityRef] by allocating `value` in `arena`
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
    pub fn parent(&self) -> Option<UnsafeIntrusiveMapEntityRef<<T as EntityWithParent>::Parent>> {
        unsafe {
            let offset = core::mem::offset_of!(RawEntityMetadata<T, IntrusiveLink>, metadata);
            let current = self.inner.byte_add(offset).cast::<IntrusiveLink>();
            current.as_ref().parent()
        }
    }

    pub(self) fn set_parent(
        &self,
        parent: Option<UnsafeIntrusiveMapEntityRef<<T as EntityWithParent>::Parent>>,
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
        UnsafeIntrusiveMapEntityRef<<<T as EntityWithParent>::Parent as EntityWithParent>::Parent>,
    > {
        self.parent().and_then(|parent| parent.parent())
    }
}

impl<T> RawEntityRef<T, IntrusiveLink> {
    /// Returns true if this entity is linked into an intrusive map
    pub fn is_linked(&self) -> bool {
        unsafe {
            let offset = core::mem::offset_of!(RawEntityMetadata<T, IntrusiveLink>, metadata);
            let current = self.inner.byte_add(offset).cast::<IntrusiveLink>();
            current.as_ref().is_linked()
        }
    }

    /*

    /// Get the previous entity in the map of `T` containing the current entity
    ///
    /// For example, in a map of `Operation` in a `Block`, this would return the handle of the
    /// previous operation in the block, or `None` if there are no other ops before this one.
    pub fn prev(&self) -> Option<Self> {
        use intrusive_collections::rbtree::LinkOps;
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

    /// Get the next entity in the map of `T` containing the current entity
    ///
    /// For example, in a map of `Operation` in a `Block`, this would return the handle of the
    /// next operation in the block, or `None` if there are no other ops after this one.
    pub fn next(&self) -> Option<Self> {
        use intrusive_collections::rbtree::LinkOps;
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
     */

    #[inline]
    unsafe fn from_link_ptr(link: NonNull<IntrusiveLink>) -> Self {
        let offset = core::mem::offset_of!(RawEntityMetadata<T, IntrusiveLink>, metadata);
        let ptr = unsafe { link.byte_sub(offset).cast::<RawEntityMetadata<T, IntrusiveLink>>() };
        Self { inner: ptr }
    }
}

/// A trait implemented by any [Entity] that is storeable in an [EntityMap].
///
/// This trait defines callbacks that are executed any time the entity is added, removed, or
/// transferred between collections.
///
/// By default, these callbacks are no-ops.
#[allow(unused_variables)]
pub trait EntityMapItem: Sized + EntityWithKey {
    /// Invoked when this entity type is inserted into an intrusive map
    #[inline]
    fn on_inserted(
        this: UnsafeIntrusiveMapEntityRef<Self>,
        cursor: &mut EntityMapCursorMut<'_, Self>,
    ) {
    }
    /// Invoked when this entity type is removed from an intrusive map
    #[inline]
    fn on_removed(this: UnsafeIntrusiveMapEntityRef<Self>, map: &mut EntityMapCursorMut<'_, Self>) {
    }
    /// Invoked when a set of entities is moved from one intrusive map to another
    #[inline]
    fn on_transfer(
        this: UnsafeIntrusiveMapEntityRef<Self>,
        from: &mut EntityMap<Self>,
        to: &mut EntityMap<Self>,
    ) {
    }
}

impl<T: Sized + EntityWithKey> EntityMapItem for T {
    default fn on_inserted(
        _this: UnsafeIntrusiveMapEntityRef<Self>,
        _map: &mut EntityMapCursorMut<'_, Self>,
    ) {
    }

    default fn on_removed(
        _this: UnsafeIntrusiveMapEntityRef<Self>,
        _map: &mut EntityMapCursorMut<'_, Self>,
    ) {
    }

    default fn on_transfer(
        _this: UnsafeIntrusiveMapEntityRef<Self>,
        _from: &mut EntityMap<Self>,
        _to: &mut EntityMap<Self>,
    ) {
    }
}

unsafe impl<T> intrusive_collections::Adapter
    for super::adapter::EntityAdapter<T, IntrusiveLink, intrusive_collections::rbtree::LinkOps>
{
    type LinkOps = intrusive_collections::rbtree::LinkOps;
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
