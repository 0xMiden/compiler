mod group;
mod list;
mod storage;

use alloc::alloc::{AllocError, Layout};
use core::{
    any::Any,
    cell::{Cell, UnsafeCell},
    fmt,
    hash::Hash,
    mem::MaybeUninit,
    ops::{Deref, DerefMut},
    ptr::NonNull,
};

pub use self::{
    group::EntityGroup,
    list::{EntityCursor, EntityCursorMut, EntityIter, EntityList},
    storage::{EntityRange, EntityRangeMut, EntityStorage},
};
use crate::any::*;

/// A trait implemented by an IR entity that has a unique identifier
///
/// Currently, this is used only for [Value]s and [Block]s.
pub trait Entity: Any {
    type Id: EntityId;

    fn id(&self) -> Self::Id;
}

/// A trait implemented by an IR entity that can be stored in [EntityStorage].
pub trait StorableEntity {
    /// Get the absolute index of this entity in its container.
    fn index(&self) -> usize;
    /// Set the absolute index of this entity in its container.
    ///
    /// # Safety
    ///
    /// This is intended to be called only by the [EntityStorage] implementation, as it is
    /// responsible for maintaining indices of all items it is storing. However, entities commonly
    /// want to know their own index in storage, so this trait allows them to conceptually own the
    /// index, but delegate maintenance to [EntityStorage].
    unsafe fn set_index(&mut self, index: usize);
    /// Called when this entity is removed from [EntityStorage]
    #[inline(always)]
    fn unlink(&mut self) {}
}

/// A trait that must be implemented by the unique identifier for an [Entity]
pub trait EntityId: Copy + Clone + PartialEq + Eq + PartialOrd + Ord + Hash {
    fn as_usize(&self) -> usize;
}

/// An error raised when an aliasing violation is detected in the use of [EntityHandle]
#[non_exhaustive]
pub struct AliasingViolationError {
    #[cfg(debug_assertions)]
    location: &'static core::panic::Location<'static>,
    kind: AliasingViolationKind,
}

#[derive(Debug)]
enum AliasingViolationKind {
    /// Attempted to create an immutable alias for an entity that was mutably borrowed
    Immutable,
    /// Attempted to create a mutable alias for an entity that was immutably borrowed
    Mutable,
}

impl fmt::Display for AliasingViolationKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Immutable => f.write_str("already mutably borrowed"),
            Self::Mutable => f.write_str("already borrowed"),
        }
    }
}

impl fmt::Debug for AliasingViolationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut builder = f.debug_struct("AliasingViolationError");
        builder.field("kind", &self.kind);
        #[cfg(debug_assertions)]
        builder.field("location", &self.location);
        builder.finish()
    }
}
impl fmt::Display for AliasingViolationError {
    #[cfg(debug_assertions)]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} in file '{}' at line {} and column {}",
            &self.kind,
            self.location.file(),
            self.location.line(),
            self.location.column()
        )
    }

    #[cfg(not(debug_assertions))]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", &self.kind)
    }
}

/// A raw pointer to an IR entity that has no associated metadata
pub type UnsafeEntityRef<T> = RawEntityRef<T, ()>;

/// A raw pointer to an IR entity that has an intrusive linked-list link as its metadata
pub type UnsafeIntrusiveEntityRef<T> = RawEntityRef<T, intrusive_collections::LinkedListLink>;

/// A [RawEntityRef] is an unsafe smart pointer type for IR entities allocated in a [Context].
///
/// Along with the type of entity referenced, it can be instantiated with extra metadata of any
/// type. For example, [UnsafeIntrusiveEntityRef] stores an intrusive link in the entity metadata,
/// so that the entity can be added to an intrusive linked list without the entity needing to
/// know about the link - and without violating aliasing rules when navigating the list.
///
/// Unlike regular references, no reference to the underlying `T` is constructed until one is
/// needed, at which point the borrow (whether mutable or immutable) is dynamically checked to
/// ensure that it is valid according to Rust's aliasing rules.
///
/// As a result, a [RawEntityRef] is not considered an alias, and it is possible to acquire a
/// mutable reference to the underlying data even while other copies of the handle exist. Any
/// attempt to construct invalid aliases (immutable reference while a mutable reference exists, or
/// vice versa), will result in a runtime panic.
///
/// This is a tradeoff, as we do not get compile-time guarantees that such panics will not occur,
/// but in exchange we get a much more flexible and powerful IR structure.
///
/// # SAFETY
///
/// Unlike most smart-pointer types, e.g. `Rc`, [RAwEntityRef] does not provide any protection
/// against the underlying allocation being deallocated (i.e. the arena it points into is dropped).
/// This is by design, as the type is meant to be stored in objects inside the arena, and
/// _not_ dropped when the arena is dropped. This requires care when using it however, to ensure
/// that no [RawEntityRef] lives longer than the arena that allocated it.
///
/// For a safe entity reference, see [EntityRef], which binds a [RawEntityRef] to the lifetime
/// of the arena.
pub struct RawEntityRef<T: ?Sized, Metadata = ()> {
    inner: NonNull<RawEntityMetadata<T, Metadata>>,
}
impl<T: ?Sized, Metadata> Clone for RawEntityRef<T, Metadata> {
    fn clone(&self) -> Self {
        Self { inner: self.inner }
    }
}
impl<T: ?Sized, Metadata> RawEntityRef<T, Metadata> {
    /// Create a new [EntityHandle] from a raw pointer to the underlying [EntityObj].
    ///
    /// # SAFETY
    ///
    /// [EntityHandle] is designed to operate like an owned smart-pointer type, ala `Rc`. As a
    /// result, it expects that the underlying data _never moves_ after it is allocated, for as
    /// long as any outstanding [EntityHandle]s exist that might be used to access that data.
    ///
    /// Additionally, it is expected that all accesses to the underlying data flow through an
    /// [EntityHandle], as it is the foundation on which the soundness of [EntityHandle] is built.
    /// You must ensure that there no other references to the underlying data exist, or can be
    /// created, _except_ via [EntityHandle].
    ///
    /// You should generally not be using this API, as it is meant solely for constructing an
    /// [EntityHandle] immediately after allocating the underlying [EntityObj].
    #[inline]
    unsafe fn from_inner(inner: NonNull<RawEntityMetadata<T, Metadata>>) -> Self {
        Self { inner }
    }

    #[inline]
    unsafe fn from_ptr(ptr: *mut RawEntityMetadata<T, Metadata>) -> Self {
        debug_assert!(!ptr.is_null());
        Self::from_inner(NonNull::new_unchecked(ptr))
    }

    #[inline]
    fn into_inner(this: Self) -> NonNull<RawEntityMetadata<T, Metadata>> {
        this.inner
    }
}

impl<T: 'static, Metadata: 'static> RawEntityRef<T, Metadata> {
    /// Create a new [RawEntityRef] by allocating `value` with `metadata` in the given arena
    /// allocator.
    ///
    /// # SAFETY
    ///
    /// The resulting [RawEntityRef] must not outlive the arena. This is not enforced statically,
    /// it is up to the caller to uphold the invariants of this type.
    pub fn new_with_metadata(value: T, metadata: Metadata, arena: &blink_alloc::Blink) -> Self {
        unsafe {
            Self::from_inner(NonNull::new_unchecked(
                arena.put(RawEntityMetadata::new(value, metadata)),
            ))
        }
    }

    /// Create a [RawEntityRef] for an entity which may not be fully initialized, using the provided
    /// arena.
    ///
    /// # SAFETY
    ///
    /// The safety rules are much the same as [RawEntityRef::new], with the main difference
    /// being that the `T` does not have to be initialized yet. No references to the `T` will
    /// be created directly until [RawEntityRef::assume_init] is called.
    pub fn new_uninit_with_metadata(
        metadata: Metadata,
        arena: &blink_alloc::Blink,
    ) -> RawEntityRef<MaybeUninit<T>, Metadata> {
        unsafe {
            RawEntityRef::from_ptr(RawEntityRef::allocate_for_layout(
                metadata,
                Layout::new::<T>(),
                |layout| arena.allocator().allocate(layout),
                <*mut u8>::cast,
            ))
        }
    }
}

impl<T: 'static> RawEntityRef<T, ()> {
    pub fn new(value: T, arena: &blink_alloc::Blink) -> Self {
        RawEntityRef::new_with_metadata(value, (), arena)
    }

    pub fn new_uninit(arena: &blink_alloc::Blink) -> RawEntityRef<MaybeUninit<T>, ()> {
        RawEntityRef::new_uninit_with_metadata((), arena)
    }
}

impl<T, Metadata> RawEntityRef<MaybeUninit<T>, Metadata> {
    /// Converts to `RawEntityRef<T>`.
    ///
    /// # Safety
    ///
    /// Just like with [MaybeUninit::assume_init], it is up to the caller to guarantee that the
    /// value really is in an initialized state. Calling this when the content is not yet fully
    /// initialized causes immediate undefined behavior.
    #[inline]
    pub unsafe fn assume_init(self) -> RawEntityRef<T, Metadata> {
        let ptr = Self::into_inner(self);
        unsafe { RawEntityRef::from_inner(ptr.cast()) }
    }
}

impl<T: ?Sized, Metadata> RawEntityRef<T, Metadata> {
    /// Convert this handle into a raw pointer to the underlying entity.
    ///
    /// This should only be used in situations where the returned pointer will not be used to
    /// actually access the underlying entity. Use [get] or [get_mut] for that. [RawEntityRef]
    /// ensures that Rust's aliasing rules are not violated when using it, but if you use the
    /// returned pointer to do so, no such guarantee is provided, and undefined behavior can
    /// result.
    ///
    /// # Safety
    ///
    /// The returned pointer _must_ not be used to create a reference to the underlying entity
    /// unless you can guarantee that such a reference does not violate Rust's aliasing rules.
    ///
    /// Do not use the pointer to create a mutable reference if other references exist, and do
    /// not use the pointer to create an immutable reference if a mutable reference exists or
    /// might be created while the immutable reference lives.
    pub fn into_raw(this: Self) -> *const T {
        Self::as_ptr(&this)
    }

    pub fn as_ptr(this: &Self) -> *const T {
        let ptr: *mut RawEntityMetadata<T, Metadata> = NonNull::as_ptr(this.inner);

        // SAFETY: This cannot go through Deref::deref or RawEntityRef::inner because this
        // is required to retain raw/mut provenance such that e.g. `get_mut` can write through
        // the pointer after the RawEntityRef is recovered through `from_raw`
        let ptr = unsafe { core::ptr::addr_of_mut!((*ptr).entity.cell) };
        UnsafeCell::raw_get(ptr).cast_const()
    }

    /// Convert a pointer returned by [RawEntityRef::into_raw] back into a [RawEntityRef].
    ///
    /// # Safety
    ///
    /// * It is _only_ valid to call this method on a pointer returned by [RawEntityRef::into_raw].
    /// * The pointer must be a valid pointer for `T`
    pub unsafe fn from_raw(ptr: *const T) -> Self {
        let offset = unsafe { RawEntityMetadata::<T, Metadata>::data_offset(ptr) };

        // Reverse the offset to find the original EntityObj
        let entity_ptr = unsafe { ptr.byte_sub(offset) as *mut RawEntityMetadata<T, Metadata> };

        unsafe { Self::from_ptr(entity_ptr) }
    }

    /// Get a dynamically-checked immutable reference to the underlying `T`
    #[track_caller]
    pub fn borrow<'a, 'b: 'a>(&'a self) -> EntityRef<'b, T> {
        let ptr: *mut RawEntityMetadata<T, Metadata> = NonNull::as_ptr(self.inner);
        unsafe { (*core::ptr::addr_of!((*ptr).entity)).borrow() }
    }

    /// Get a dynamically-checked mutable reference to the underlying `T`
    #[track_caller]
    pub fn borrow_mut<'a, 'b: 'a>(&'a mut self) -> EntityMut<'b, T> {
        let ptr: *mut RawEntityMetadata<T, Metadata> = NonNull::as_ptr(self.inner);
        unsafe { (*core::ptr::addr_of!((*ptr).entity)).borrow_mut() }
    }

    /// Try to get a dynamically-checked mutable reference to the underlying `T`
    ///
    /// Returns `None` if the entity is already borrowed
    pub fn try_borrow_mut<'a, 'b: 'a>(&'a mut self) -> Option<EntityMut<'b, T>> {
        let ptr: *mut RawEntityMetadata<T, Metadata> = NonNull::as_ptr(self.inner);
        unsafe { (*core::ptr::addr_of!((*ptr).entity)).try_borrow_mut().ok() }
    }

    pub fn ptr_eq(this: &Self, other: &Self) -> bool {
        core::ptr::addr_eq(this.inner.as_ptr(), other.inner.as_ptr())
    }

    unsafe fn allocate_for_layout<F, F2>(
        metadata: Metadata,
        value_layout: Layout,
        allocate: F,
        mem_to_metadata: F2,
    ) -> *mut RawEntityMetadata<T, Metadata>
    where
        F: FnOnce(Layout) -> Result<NonNull<[u8]>, AllocError>,
        F2: FnOnce(*mut u8) -> *mut RawEntityMetadata<T, Metadata>,
    {
        use alloc::alloc::handle_alloc_error;

        let layout = raw_entity_metadata_layout_for_value_layout::<Metadata>(value_layout);
        unsafe {
            RawEntityRef::try_allocate_for_layout(metadata, value_layout, allocate, mem_to_metadata)
                .unwrap_or_else(|_| handle_alloc_error(layout))
        }
    }

    #[inline]
    unsafe fn try_allocate_for_layout<F, F2>(
        metadata: Metadata,
        value_layout: Layout,
        allocate: F,
        mem_to_metadata: F2,
    ) -> Result<*mut RawEntityMetadata<T, Metadata>, AllocError>
    where
        F: FnOnce(Layout) -> Result<NonNull<[u8]>, AllocError>,
        F2: FnOnce(*mut u8) -> *mut RawEntityMetadata<T, Metadata>,
    {
        let layout = raw_entity_metadata_layout_for_value_layout::<Metadata>(value_layout);
        let ptr = allocate(layout)?;
        let inner = mem_to_metadata(ptr.as_non_null_ptr().as_ptr());
        unsafe {
            debug_assert_eq!(Layout::for_value_raw(inner), layout);

            core::ptr::addr_of_mut!((*inner).metadata).write(metadata);
            core::ptr::addr_of_mut!((*inner).entity.borrow).write(Cell::new(BorrowFlag::UNUSED));
            #[cfg(debug_assertions)]
            core::ptr::addr_of_mut!((*inner).entity.borrowed_at).write(Cell::new(None));
        }

        Ok(inner)
    }
}

impl<From: ?Sized, Metadata: 'static> RawEntityRef<From, Metadata> {
    /// Casts this reference to the concrete type `T`, if the underlying value is a `T`.
    ///
    /// If the cast is not valid for this reference, `Err` is returned containing the original value.
    #[inline]
    pub fn try_downcast<To, Obj>(
        self,
    ) -> Result<RawEntityRef<To, Metadata>, RawEntityRef<Obj, Metadata>>
    where
        To: DowncastFromRef<From> + 'static,
        From: Is<Obj> + AsAny + 'static,
        Obj: ?Sized,
    {
        RawEntityRef::<To, Metadata>::try_downcast_from(self)
    }

    /// Casts this reference to the concrete type `T`, if the underlying value is a `T`.
    ///
    /// If the cast is not valid for this reference, `Err` is returned containing the original value.
    #[inline]
    pub fn try_downcast_ref<To, Obj>(&self) -> Option<RawEntityRef<To, Metadata>>
    where
        To: DowncastFromRef<From> + 'static,
        From: Is<Obj> + AsAny + 'static,
        Obj: ?Sized,
    {
        RawEntityRef::<To, Metadata>::try_downcast_from_ref(self)
    }

    /// Casts this reference to the concrete type `T`, if the underlying value is a `T`.
    ///
    /// Panics if the cast is not valid for this reference.
    #[inline]
    #[track_caller]
    pub fn downcast<To, Obj>(self) -> RawEntityRef<To, Metadata>
    where
        To: DowncastFromRef<From> + 'static,
        From: Is<Obj> + AsAny + 'static,
        Obj: ?Sized,
    {
        RawEntityRef::<To, Metadata>::downcast_from(self)
    }

    /// Casts this reference to the concrete type `T`, if the underlying value is a `T`.
    ///
    /// Panics if the cast is not valid for this reference.
    #[inline]
    #[track_caller]
    pub fn downcast_ref<To, Obj>(&self) -> RawEntityRef<To, Metadata>
    where
        To: DowncastFromRef<From> + 'static,
        From: Is<Obj> + AsAny + 'static,
        Obj: ?Sized,
    {
        RawEntityRef::<To, Metadata>::downcast_from_ref(self)
    }
}

impl<To, Metadata: 'static> RawEntityRef<To, Metadata> {
    pub fn try_downcast_from<From, Obj>(
        from: RawEntityRef<From, Metadata>,
    ) -> Result<Self, RawEntityRef<Obj, Metadata>>
    where
        From: ?Sized + Is<Obj> + AsAny + 'static,
        To: DowncastFromRef<From> + 'static,
        Obj: ?Sized,
    {
        let borrow = from.borrow();
        if let Some(to) = borrow.as_any().downcast_ref() {
            Ok(unsafe { RawEntityRef::from_raw(to) })
        } else {
            Err(from)
        }
    }

    pub fn try_downcast_from_ref<From, Obj>(from: &RawEntityRef<From, Metadata>) -> Option<Self>
    where
        From: ?Sized + Is<Obj> + AsAny + 'static,
        To: DowncastFromRef<From> + 'static,
        Obj: ?Sized,
    {
        let borrow = from.borrow();
        if let Some(to) = borrow.as_any().downcast_ref() {
            Some(unsafe { RawEntityRef::from_raw(to) })
        } else {
            None
        }
    }

    #[track_caller]
    pub fn downcast_from<From, Obj>(from: RawEntityRef<From, Metadata>) -> Self
    where
        From: ?Sized + Is<Obj> + AsAny + 'static,
        To: DowncastFromRef<From> + 'static,
        Obj: ?Sized,
    {
        let borrow = from.borrow();
        unsafe { RawEntityRef::from_raw(borrow.as_any().downcast_ref().expect("invalid cast")) }
    }

    #[track_caller]
    pub fn downcast_from_ref<From, Obj>(from: &RawEntityRef<From, Metadata>) -> Self
    where
        From: ?Sized + Is<Obj> + AsAny + 'static,
        To: DowncastFromRef<From> + 'static,
        Obj: ?Sized,
    {
        let borrow = from.borrow();
        unsafe { RawEntityRef::from_raw(borrow.as_any().downcast_ref().expect("invalid cast")) }
    }
}

impl<From: ?Sized, Metadata: 'static> RawEntityRef<From, Metadata> {
    /// Casts this reference to the an unsized type `Trait`, if `From` implements `Trait`
    ///
    /// If the cast is not valid for this reference, `Err` is returned containing the original value.
    #[inline]
    pub fn upcast<To>(self) -> RawEntityRef<To, Metadata>
    where
        To: ?Sized,
        From: core::marker::Unsize<To> + AsAny + 'static,
    {
        unsafe { RawEntityRef::<To, Metadata>::from_inner(self.inner) }
    }
}

impl<T, U, Metadata> core::ops::CoerceUnsized<RawEntityRef<U, Metadata>>
    for RawEntityRef<T, Metadata>
where
    T: ?Sized + core::marker::Unsize<U>,
    U: ?Sized,
{
}
impl<T: ?Sized, Metadata> Eq for RawEntityRef<T, Metadata> {}
impl<T: ?Sized, Metadata> PartialEq for RawEntityRef<T, Metadata> {
    fn eq(&self, other: &Self) -> bool {
        Self::ptr_eq(self, other)
    }
}
impl<T: ?Sized, Metadata> core::hash::Hash for RawEntityRef<T, Metadata> {
    fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
        self.inner.hash(state);
    }
}
impl<T: ?Sized, Metadata> fmt::Pointer for RawEntityRef<T, Metadata> {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Pointer::fmt(&Self::as_ptr(self), f)
    }
}

impl<T: ?Sized + fmt::Debug, Metadata> fmt::Debug for RawEntityRef<T, Metadata> {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.borrow(), f)
    }
}
impl<T: ?Sized + crate::formatter::PrettyPrint, Metadata> crate::formatter::PrettyPrint
    for RawEntityRef<T, Metadata>
{
    #[inline]
    fn render(&self) -> crate::formatter::Document {
        self.borrow().render()
    }
}
impl<T: ?Sized + StorableEntity, Metadata> StorableEntity for RawEntityRef<T, Metadata> {
    #[inline]
    fn index(&self) -> usize {
        self.borrow().index()
    }

    #[inline]
    unsafe fn set_index(&mut self, index: usize) {
        unsafe {
            self.borrow_mut().set_index(index);
        }
    }

    #[inline]
    fn unlink(&mut self) {
        self.borrow_mut().unlink()
    }
}

/// A guard that ensures a reference to an IR entity cannot be mutably aliased
pub struct EntityRef<'b, T: ?Sized + 'b> {
    value: NonNull<T>,
    borrow: BorrowRef<'b>,
}
impl<T: ?Sized> core::ops::Deref for EntityRef<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        // SAFETY: the value is accessible as long as we hold our borrow.
        unsafe { self.value.as_ref() }
    }
}
impl<'b, T: ?Sized> EntityRef<'b, T> {
    #[inline]
    pub fn map<U: ?Sized, F>(orig: Self, f: F) -> EntityRef<'b, U>
    where
        F: FnOnce(&T) -> &U,
    {
        EntityRef {
            value: NonNull::from(f(&*orig)),
            borrow: orig.borrow,
        }
    }
}

impl<'b, T, U> core::ops::CoerceUnsized<EntityRef<'b, U>> for EntityRef<'b, T>
where
    T: ?Sized + core::marker::Unsize<U>,
    U: ?Sized,
{
}

impl<T: ?Sized + fmt::Debug> fmt::Debug for EntityRef<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        (**self).fmt(f)
    }
}
impl<T: ?Sized + fmt::Display> fmt::Display for EntityRef<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        (**self).fmt(f)
    }
}
impl<T: ?Sized + crate::formatter::PrettyPrint> crate::formatter::PrettyPrint for EntityRef<'_, T> {
    #[inline]
    fn render(&self) -> crate::formatter::Document {
        (**self).render()
    }
}
impl<T: ?Sized + Eq> Eq for EntityRef<'_, T> {}
impl<T: ?Sized + PartialEq> PartialEq for EntityRef<'_, T> {
    fn eq(&self, other: &Self) -> bool {
        **self == **other
    }
}
impl<T: ?Sized + PartialOrd> PartialOrd for EntityRef<'_, T> {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        (**self).partial_cmp(&**other)
    }

    fn ge(&self, other: &Self) -> bool {
        **self >= **other
    }

    fn gt(&self, other: &Self) -> bool {
        **self > **other
    }

    fn le(&self, other: &Self) -> bool {
        **self <= **other
    }

    fn lt(&self, other: &Self) -> bool {
        **self < **other
    }
}
impl<T: ?Sized + Ord> Ord for EntityRef<'_, T> {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        (**self).cmp(&**other)
    }
}
impl<T: ?Sized + Hash> Hash for EntityRef<'_, T> {
    fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
        (**self).hash(state);
    }
}

/// A guard that provides exclusive access to an IR entity
pub struct EntityMut<'b, T: ?Sized> {
    /// The raw pointer to the underlying data
    ///
    /// This is a pointer rather than a `&'b mut T` to avoid `noalias` violations, because a
    /// `EntityMut` argument doesn't hold exclusivity for its whole scope, only until it drops.
    value: NonNull<T>,
    /// This value provides the drop glue for tracking that the underlying allocation is
    /// mutably borrowed, but it is otherwise not read.
    #[allow(unused)]
    borrow: BorrowRefMut<'b>,
    /// `NonNull` is covariant over `T`, so we need to reintroduce invariance via phantom data
    _marker: core::marker::PhantomData<&'b mut T>,
}
impl<'b, T: ?Sized> EntityMut<'b, T> {
    /// Splits an `EntityMut` into multiple `EntityMut`s for different components of the borrowed
    /// data.
    ///
    /// The underlying entity will remain mutably borrowed until both returned `EntityMut`s go out
    /// of scope.
    ///
    /// The entity is already mutably borrowed, so this cannot fail.
    ///
    /// This is an associated function that needs to be used as `EntityMut::map_split(...)`, so as
    /// to avoid conflicting with any method of the same name accessible via the `Deref` impl.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use midenc_hir2::*;
    /// use blink_alloc::Blink;
    ///
    /// let alloc = Blink::default();
    /// let mut entity = UnsafeEntityRef::new([1, 2, 3, 4], &alloc);
    /// let borrow = entity.borrow_mut();
    /// let (mut begin, mut end) = EntityMut::map_split(borrow, |slice| slice.split_at_mut(2));
    /// assert_eq!(*begin, [1, 2]);
    /// assert_eq!(*end, [3, 4]);
    /// begin.copy_from_slice(&[4, 3]);
    /// end.copy_from_slice(&[2, 1]);
    /// ```
    #[inline]
    pub fn map_split<U: ?Sized, V: ?Sized, F>(
        mut orig: Self,
        f: F,
    ) -> (EntityMut<'b, U>, EntityMut<'b, V>)
    where
        F: FnOnce(&mut T) -> (&mut U, &mut V),
    {
        let borrow = orig.borrow.clone();
        let (a, b) = f(&mut *orig);
        (
            EntityMut {
                value: NonNull::from(a),
                borrow,
                _marker: core::marker::PhantomData,
            },
            EntityMut {
                value: NonNull::from(b),
                borrow: orig.borrow,
                _marker: core::marker::PhantomData,
            },
        )
    }
}
impl<T: ?Sized> Deref for EntityMut<'_, T> {
    type Target = T;

    #[inline]
    fn deref(&self) -> &T {
        // SAFETY: the value is accessible as long as we hold our borrow.
        unsafe { self.value.as_ref() }
    }
}
impl<T: ?Sized> DerefMut for EntityMut<'_, T> {
    #[inline]
    fn deref_mut(&mut self) -> &mut T {
        // SAFETY: the value is accessible as long as we hold our borrow.
        unsafe { self.value.as_mut() }
    }
}

impl<'b, T, U> core::ops::CoerceUnsized<EntityMut<'b, U>> for EntityMut<'b, T>
where
    T: ?Sized + core::marker::Unsize<U>,
    U: ?Sized,
{
}

impl<T: ?Sized + fmt::Debug> fmt::Debug for EntityMut<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        (**self).fmt(f)
    }
}
impl<T: ?Sized + fmt::Display> fmt::Display for EntityMut<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        (**self).fmt(f)
    }
}
impl<T: ?Sized + crate::formatter::PrettyPrint> crate::formatter::PrettyPrint for EntityMut<'_, T> {
    #[inline]
    fn render(&self) -> crate::formatter::Document {
        (**self).render()
    }
}
impl<T: ?Sized + Eq> Eq for EntityMut<'_, T> {}
impl<T: ?Sized + PartialEq> PartialEq for EntityMut<'_, T> {
    fn eq(&self, other: &Self) -> bool {
        **self == **other
    }
}
impl<T: ?Sized + PartialOrd> PartialOrd for EntityMut<'_, T> {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        (**self).partial_cmp(&**other)
    }

    fn ge(&self, other: &Self) -> bool {
        **self >= **other
    }

    fn gt(&self, other: &Self) -> bool {
        **self > **other
    }

    fn le(&self, other: &Self) -> bool {
        **self <= **other
    }

    fn lt(&self, other: &Self) -> bool {
        **self < **other
    }
}
impl<T: ?Sized + Ord> Ord for EntityMut<'_, T> {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        (**self).cmp(&**other)
    }
}
impl<T: ?Sized + Hash> Hash for EntityMut<'_, T> {
    fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
        (**self).hash(state);
    }
}

// This type wraps the entity data with extra metadata we want to associate with the entity, but
// separately from it, so that pointers to the metadata do not cause aliasing violations if the
// entity itself is borrowed.
//
// The kind of metadata stored here is unconstrained, but in practice should be limited to things
// that you _need_ to be able to access from a `RawEntityRef`, without aliasing the entity. For now
// the main reason we use this is for the intrusive link used to store entities in an intrusive
// linked list. We don't want traversing the intrusive list to require borrowing the entity, only
// the link, unless we explicitly want to borrow the entity, thus we use the metadata field here
// to hold the link.
//
// This has to be `pub` for implementing the traits required for the intrusive collections
// integration, but its internals are hidden outside this module, and we hide it from the generated
// docs as well.
#[repr(C)]
#[doc(hidden)]
pub struct RawEntityMetadata<T: ?Sized, Metadata> {
    metadata: Metadata,
    entity: RawEntity<T>,
}
impl<T, Metadata> RawEntityMetadata<T, Metadata> {
    pub(crate) fn new(value: T, metadata: Metadata) -> Self {
        Self {
            metadata,
            entity: RawEntity::new(value),
        }
    }
}
impl<T: ?Sized, Metadata> RawEntityMetadata<T, Metadata> {
    pub(self) fn borrow(&self) -> EntityRef<'_, T> {
        let ptr = self as *const Self;
        unsafe { (*core::ptr::addr_of!((*ptr).entity)).borrow() }
    }

    pub(self) fn borrow_mut(&self) -> EntityMut<'_, T> {
        let ptr = (self as *const Self).cast_mut();
        unsafe { (*core::ptr::addr_of_mut!((*ptr).entity)).borrow_mut() }
    }

    #[inline]
    const fn metadata_offset() -> usize {
        core::mem::offset_of!(RawEntityMetadata<(), Metadata>, metadata)
    }

    /// Get the offset within a `RawEntityMetadata` for the payload behind a pointer.
    ///
    /// # Safety
    ///
    /// The pointer must point to (and have valid metadata for) a previously valid instance of T, but
    /// the T is allowed to be dropped.
    unsafe fn data_offset(ptr: *const T) -> usize {
        use core::mem::align_of_val_raw;

        // Align the unsized value to the end of the RawEntityMetadata.
        // Because RawEntityMetadata/RawEntity is repr(C), it will always be the last field in memory.
        //
        // SAFETY: since the only unsized types possible are slices, trait objects, and extern types,
        // the input safety requirement is currently enough to satisfy the requirements of
        // align_of_val_raw; but this is an implementation detail of the language that is unstable
        unsafe { RawEntityMetadata::<(), Metadata>::data_offset_align(align_of_val_raw(ptr)) }
    }

    #[inline]
    fn data_offset_align(align: usize) -> usize {
        let layout = Layout::new::<RawEntityMetadata<(), Metadata>>();
        layout.size() + layout.padding_needed_for(align)
    }
}

fn raw_entity_metadata_layout_for_value_layout<Metadata>(layout: Layout) -> Layout {
    Layout::new::<RawEntityMetadata<(), Metadata>>()
        .extend(layout)
        .unwrap()
        .0
        .pad_to_align()
}

/// A [RawEntity] wraps an entity to be allocated in a [Context], and provides dynamic borrow-
/// checking functionality for [UnsafeEntityRef], thereby protecting the entity by ensuring that
/// all accesses adhere to Rust's aliasing rules.
#[repr(C)]
struct RawEntity<T: ?Sized> {
    borrow: Cell<BorrowFlag>,
    #[cfg(debug_assertions)]
    borrowed_at: Cell<Option<&'static core::panic::Location<'static>>>,
    cell: UnsafeCell<T>,
}

impl<T> RawEntity<T> {
    pub fn new(value: T) -> Self {
        Self {
            borrow: Cell::new(BorrowFlag::UNUSED),
            #[cfg(debug_assertions)]
            borrowed_at: Cell::new(None),
            cell: UnsafeCell::new(value),
        }
    }
}

impl<T: ?Sized> RawEntity<T> {
    #[track_caller]
    #[inline]
    pub fn borrow(&self) -> EntityRef<'_, T> {
        match self.try_borrow() {
            Ok(b) => b,
            Err(err) => panic_aliasing_violation(err),
        }
    }

    #[inline]
    #[cfg_attr(debug_assertions, track_caller)]
    pub fn try_borrow(&self) -> Result<EntityRef<'_, T>, AliasingViolationError> {
        match BorrowRef::new(&self.borrow) {
            Some(b) => {
                #[cfg(debug_assertions)]
                {
                    // `borrowed_at` is always the *first* active borrow
                    if b.borrow.get() == BorrowFlag(1) {
                        self.borrowed_at.set(Some(core::panic::Location::caller()));
                    }
                }

                // SAFETY: `BorrowRef` ensures that there is only immutable access to the value
                // while borrowed.
                let value = unsafe { NonNull::new_unchecked(self.cell.get()) };
                Ok(EntityRef { value, borrow: b })
            }
            None => Err(AliasingViolationError {
                #[cfg(debug_assertions)]
                location: self.borrowed_at.get().unwrap(),
                kind: AliasingViolationKind::Immutable,
            }),
        }
    }

    #[inline]
    #[track_caller]
    pub fn borrow_mut(&self) -> EntityMut<'_, T> {
        match self.try_borrow_mut() {
            Ok(b) => b,
            Err(err) => panic_aliasing_violation(err),
        }
    }

    #[inline]
    #[cfg_attr(feature = "debug_refcell", track_caller)]
    pub fn try_borrow_mut(&self) -> Result<EntityMut<'_, T>, AliasingViolationError> {
        match BorrowRefMut::new(&self.borrow) {
            Some(b) => {
                #[cfg(debug_assertions)]
                {
                    self.borrowed_at.set(Some(core::panic::Location::caller()));
                }

                // SAFETY: `BorrowRefMut` guarantees unique access.
                let value = unsafe { NonNull::new_unchecked(self.cell.get()) };
                Ok(EntityMut {
                    value,
                    borrow: b,
                    _marker: core::marker::PhantomData,
                })
            }
            None => Err(AliasingViolationError {
                // If a borrow occurred, then we must already have an outstanding borrow,
                // so `borrowed_at` will be `Some`
                #[cfg(debug_assertions)]
                location: self.borrowed_at.get().unwrap(),
                kind: AliasingViolationKind::Mutable,
            }),
        }
    }
}

struct BorrowRef<'b> {
    borrow: &'b Cell<BorrowFlag>,
}
impl<'b> BorrowRef<'b> {
    #[inline]
    fn new(borrow: &'b Cell<BorrowFlag>) -> Option<Self> {
        let b = borrow.get().wrapping_add(1);
        if !b.is_reading() {
            // Incrementing borrow can result in a non-reading value (<= 0) in these cases:
            // 1. It was < 0, i.e. there are writing borrows, so we can't allow a read borrow due to
            //    Rust's reference aliasing rules
            // 2. It was isize::MAX (the max amount of reading borrows) and it overflowed into
            //    isize::MIN (the max amount of writing borrows) so we can't allow an additional
            //    read borrow because isize can't represent so many read borrows (this can only
            //    happen if you mem::forget more than a small constant amount of `EntityRef`s, which
            //    is not good practice)
            None
        } else {
            // Incrementing borrow can result in a reading value (> 0) in these cases:
            // 1. It was = 0, i.e. it wasn't borrowed, and we are taking the first read borrow
            // 2. It was > 0 and < isize::MAX, i.e. there were read borrows, and isize is large
            //    enough to represent having one more read borrow
            borrow.set(b);
            Some(Self { borrow })
        }
    }
}
impl Drop for BorrowRef<'_> {
    #[inline]
    fn drop(&mut self) {
        let borrow = self.borrow.get();
        debug_assert!(borrow.is_reading());
        self.borrow.set(borrow - 1);
    }
}
impl Clone for BorrowRef<'_> {
    #[inline]
    fn clone(&self) -> Self {
        // Since this Ref exists, we know the borrow flag
        // is a reading borrow.
        let borrow = self.borrow.get();
        debug_assert!(borrow.is_reading());
        // Prevent the borrow counter from overflowing into
        // a writing borrow.
        assert!(borrow != BorrowFlag::MAX);
        self.borrow.set(borrow + 1);
        BorrowRef {
            borrow: self.borrow,
        }
    }
}

struct BorrowRefMut<'b> {
    borrow: &'b Cell<BorrowFlag>,
}
impl Drop for BorrowRefMut<'_> {
    #[inline]
    fn drop(&mut self) {
        let borrow = self.borrow.get();
        debug_assert!(borrow.is_writing());
        self.borrow.set(borrow + 1);
    }
}
impl<'b> BorrowRefMut<'b> {
    #[inline]
    fn new(borrow: &'b Cell<BorrowFlag>) -> Option<Self> {
        // NOTE: Unlike BorrowRefMut::clone, new is called to create the initial
        // mutable reference, and so there must currently be no existing
        // references. Thus, while clone increments the mutable refcount, here
        // we explicitly only allow going from UNUSED to UNUSED - 1.
        match borrow.get() {
            BorrowFlag::UNUSED => {
                borrow.set(BorrowFlag::UNUSED - 1);
                Some(Self { borrow })
            }
            _ => None,
        }
    }

    // Clones a `BorrowRefMut`.
    //
    // This is only valid if each `BorrowRefMut` is used to track a mutable
    // reference to a distinct, nonoverlapping range of the original object.
    // This isn't in a Clone impl so that code doesn't call this implicitly.
    #[inline]
    fn clone(&self) -> Self {
        let borrow = self.borrow.get();
        debug_assert!(borrow.is_writing());
        // Prevent the borrow counter from underflowing.
        assert!(borrow != BorrowFlag::MIN);
        self.borrow.set(borrow - 1);
        Self {
            borrow: self.borrow,
        }
    }
}

/// Positive values represent the number of outstanding immutable borrows, while negative values
/// represent the number of outstanding mutable borrows. Multiple mutable borrows can only be
/// active simultaneously if they refer to distinct, non-overlapping components of an entity.
#[derive(Default, Debug, Copy, Clone, PartialEq, Eq)]
#[repr(transparent)]
struct BorrowFlag(isize);
impl BorrowFlag {
    const MAX: Self = Self(isize::MAX);
    const MIN: Self = Self(isize::MIN);
    const UNUSED: Self = Self(0);

    pub fn is_writing(&self) -> bool {
        self.0 < Self::UNUSED.0
    }

    pub fn is_reading(&self) -> bool {
        self.0 > Self::UNUSED.0
    }

    #[inline]
    pub const fn wrapping_add(self, rhs: isize) -> Self {
        Self(self.0.wrapping_add(rhs))
    }
}
impl core::ops::Add<isize> for BorrowFlag {
    type Output = BorrowFlag;

    #[inline]
    fn add(self, rhs: isize) -> Self::Output {
        Self(self.0 + rhs)
    }
}
impl core::ops::Sub<isize> for BorrowFlag {
    type Output = BorrowFlag;

    #[inline]
    fn sub(self, rhs: isize) -> Self::Output {
        Self(self.0 - rhs)
    }
}

// This ensures the panicking code is outlined from `borrow` and `borrow_mut` for `EntityObj`.
#[cfg_attr(not(panic = "abort"), inline(never))]
#[track_caller]
#[cold]
fn panic_aliasing_violation(err: AliasingViolationError) -> ! {
    panic!("{err:?}")
}
