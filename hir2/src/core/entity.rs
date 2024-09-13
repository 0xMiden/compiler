mod list;

use core::{
    cell::{Cell, UnsafeCell},
    fmt,
    mem::MaybeUninit,
    ptr::NonNull,
};

pub use self::list::{EntityCursor, EntityCursorMut, EntityIter, EntityList};

pub trait Entity {
    type Id: EntityId;

    fn id(&self) -> Self::Key;
    unsafe fn set_id(&self, id: Self::Key);
}

pub trait EntityId: Copy + Clone + PartialEq + Eq + PartialOrd + Ord + Hash {
    fn as_usize(&self) -> usize;
    unsafe fn from_usize(raw: usize) -> Self;
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

/// An [EntityHandle] is a smart-pointer type for IR entities allocated in a [Context].
///
/// Unlike regular references, no reference to the underlying `T` is constructed until one is
/// needed, at which point the borrow (whether mutable or immutable) is dynamically checked to
/// ensure that it is valid according to Rust's aliasing rules.
///
/// As a result, an [EntityHandle] is not considered an alias, and it is possible to acquire a
/// mutable reference to the underlying data even while other copies of the handle exist. Any
/// attempt to construct invalid aliases (immutable reference while a mutable reference exists, or
/// vice versa), will result in a runtime panic.
///
/// This is a tradeoff, as we do not get compile-time guarantees that such panics will not occur,
/// but in exchange we get a much more flexible and powerful IR structure.
pub struct EntityHandle<T> {
    inner: NonNull<EntityObj<T>>,
}
impl<T> Clone for EntityHandle<T> {
    fn clone(&self) -> Self {
        Self { inner: self.inner }
    }
}
impl<T> EntityHandle<T> {
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
    pub(crate) unsafe fn new(ptr: NonNull<EntityObj<T>>) -> Self {
        Self { inner }
    }

    /// Get a dynamically-checked immutable reference to the underlying `T`
    pub fn get(&self) -> EntityRef<'_, T> {
        unsafe {
            let obj = self.inner.as_ref();
            obj.borrow()
        }
    }

    /// Get a dynamically-checked mutable reference to the underlying `T`
    pub fn get_mut(&mut self) -> EntityMut<'_, T> {
        unsafe {
            let obj = self.inner.as_ref();
            obj.borrow_mut()
        }
    }

    /// Convert this handle into a raw pointer to the underlying entity.
    ///
    /// This should only be used in situations where the returned pointer will not be used to
    /// actually access the underlying entity. Use [get] or [get_mut] for that. [EntityHandle]
    /// ensures that Rust's aliasing rules are not violated when using it, but if you use the
    /// returned pointer to do so, no such guarantee is provided, and undefined behavior can
    /// result.
    ///
    /// # SAFETY
    ///
    /// The returned pointer _must_ not be used to create a reference to the underlying entity
    /// unless you can guarantee that such a reference does not violate Rust's aliasing rules.
    ///
    /// Do not use the pointer to create a mutable reference if other references exist, and do
    /// not use the pointer to create an immutable reference if a mutable reference exists or
    /// might be created while the immutable reference lives.
    pub fn into_raw(self) -> NonNull<T> {
        unsafe { NonNull::new_unchecked(self.inner.as_ref().as_ptr()) }
    }
}

impl<T> EntityHandle<MaybeUninit<T>> {
    /// Create an [EntityHandle] for an entity which may not be fully initialized.
    ///
    /// # SAFETY
    ///
    /// The safety rules are much the same as [EntityHandle::new], with the main difference
    /// being that the `T` does not have to be initialized yet. No references to the `T` will
    /// be created directly until [EntityHandle::assume_init] is called.
    pub(crate) unsafe fn new_uninit(ptr: NonNull<EntityObj<MaybeUninit<T>>>) -> Self {
        Self { inner: ptr }
    }

    /// Converts to `EntityHandle<T>`.
    ///
    /// Just like with [MaybeUninit::assume_init], it is up to the caller to guarantee that the
    /// value really is in an initialized state. Calling this when the content is not yet fully
    /// initialized causes immediate undefined behavior.
    pub unsafe fn assume_init(self) -> EntityHandle<T> {
        EntityHandle {
            inner: self.inner.cast(),
        }
    }
}

/// A [TrackedEntityHandle] is like [EntityHandle], except it provides built-in support for
/// adding the entity to an [intrusive_collections::LinkedList] that doesn't require constructing
/// a reference to the entity itself, and thus potentially causing an aliasing violation. Instead,
/// the link is stored as part of the underlying allocation, but separate from the entity.
pub struct TrackedEntityHandle<T> {
    inner: NonNull<TrackedEntityObj<T>>,
}
impl<T> Clone for TrackedEntityHandle<T> {
    fn clone(&self) -> Self {
        Self { inner: self.inner }
    }
}
impl<T> TrackedEntityHandle<T> {
    /// Create a new [TrackedEntityHandle] from a raw pointer to the underlying [TrackedEntityObj].
    ///
    /// # SAFETY
    ///
    /// This function has the same requirements around safety as [EntityHandle::new].
    pub(crate) unsafe fn new(ptr: NonNull<TrackedEntityObj<T>>) -> Self {
        Self { inner }
    }

    /// Get a dynamically-checked immutable reference to the underlying `T`
    pub fn get(&self) -> EntityRef<'_, T> {
        unsafe {
            let obj = self.inner.as_ref();
            obj.entity.borrow()
        }
    }

    /// Get a dynamically-checked mutable reference to the underlying `T`
    pub fn get_mut(&mut self) -> EntityMut<'_, T> {
        unsafe {
            let obj = self.inner.as_ref();
            obj.entity.borrow_mut()
        }
    }

    /// Convert this handle into a raw pointer to the underlying entity.
    ///
    /// This should only be used in situations where the returned pointer will not be used to
    /// actually access the underlying entity. Use [get] or [get_mut] for that. [EntityHandle]
    /// ensures that Rust's aliasing rules are not violated when using it, but if you use the
    /// returned pointer to do so, no such guarantee is provided, and undefined behavior can
    /// result.
    ///
    /// # SAFETY
    ///
    /// The returned pointer _must_ not be used to create a reference to the underlying entity
    /// unless you can guarantee that such a reference does not violate Rust's aliasing rules.
    ///
    /// Do not use the pointer to create a mutable reference if other references exist, and do
    /// not use the pointer to create an immutable reference if a mutable reference exists or
    /// might be created while the immutable reference lives.
    pub fn into_raw(self) -> NonNull<T> {
        unsafe { NonNull::new_unchecked(self.inner.as_ref().entity.as_ptr()) }
    }
}

impl<T> TrackedEntityHandle<MaybeUninit<T>> {
    /// Create a [TrackedEntityHandle] for an entity which may not be fully initialized.
    ///
    /// # SAFETY
    ///
    /// The safety rules are much the same as [TrackedEntityHandle::new], with the main difference
    /// being that the `T` does not have to be initialized yet. No references to the `T` will
    /// be created directly until [TrackedEntityHandle::assume_init] is called.
    pub(crate) unsafe fn new_uninit(ptr: NonNull<TrackedEntityObj<MaybeUninit<T>>>) -> Self {
        Self { inner: ptr }
    }

    /// Converts to `TrackedEntityHandle<T>`.
    ///
    /// Just like with [MaybeUninit::assume_init], it is up to the caller to guarantee that the
    /// value really is in an initialized state. Calling this when the content is not yet fully
    /// initialized causes immediate undefined behavior.
    pub unsafe fn assume_init(self) -> TrackedEntityHandle<T> {
        TrackedEntityHandle {
            inner: self.inner.cast(),
        }
    }
}

unsafe impl<T> intrusive_collections::PointerOps for TrackedEntityHandle<T> {
    type Pointer = TrackedEntityHandle<T>;
    type Value = EntityObj<T>;

    unsafe fn from_raw(&self, value: *const Self::Value) -> Self::Pointer {
        assert!(!value.is_null());
        let offset = core::mem::offset_of!(TrackedEntityObj<T>, entity);
        let ptr = value.cast_mut().byte_sub(offset).cast::<TrackedEntityObj<T>>();
        debug_assert!(ptr.is_aligned());
        TrackedEntityHandle::new(NonNull::new_unchecked(ptr))
    }

    fn into_raw(&self, ptr: Self::Pointer) -> *const Self::Value {
        let ptr = ptr.into_raw().as_ptr().cast_const();
        let offset = core::mem::offset_of!(EntityObj<T>, cell);
        unsafe { ptr.byte_sub(offset).cast() }
    }
}

/// An adapter for storing any `Entity` impl in a [intrusive_collections::LinkedList]
#[derive(Default, Copy, Clone)]
pub struct EntityAdapter<T>(core::marker::PhantomData<T>);
impl<T> EntityAdapter<T> {
    pub const fn new() -> Self {
        Self(core::marker::PhantomData)
    }
}

unsafe impl<T> intrusive_collections::Adapter for EntityAdapter<T> {
    type LinkOps = intrusive_collections::linked_list::LinkOps;
    type PointerOps = intrusive_collections::DefaultPointerOps<TrackedEntityHandle<T>>;

    unsafe fn get_value(
        &self,
        link: <Self::LinkOps as intrusive_collections::LinkOps>::LinkPtr,
    ) -> *const <Self::PointerOps as intrusive_collections::PointerOps>::Value {
        let offset = core::mem::offset_of!(TrackedEntityObj<T>, link);
        let ptr = link.as_ptr().cast_const().byte_sub(offset);
        let offset = core::mem::offset_of!(TrackedEntityObj<T>, entity);
        ptr.byte_add(offset)
    }

    unsafe fn get_link(
        &self,
        value: *const <Self::PointerOps as intrusive_collections::PointerOps>::Value,
    ) -> <Self::LinkOps as intrusive_collections::LinkOps>::LinkPtr {
        let offset = core::mem::offset_of!(TrackedEntityObj<T>, entity);
        let ptr = value.byte_sub(offset);
        let offset = core::mem::offset_of!(TrackedEntityObj<T>, link);
        let ptr = ptr.byte_add(offset);
        NonNull::new_unchecked(ptr.cast_mut())
    }

    fn link_ops(&self) -> &Self::LinkOps {
        &intrusive_collections::linked_list::LinkOps
    }

    fn link_ops_mut(&mut self) -> &mut Self::LinkOps {
        &mut intrusive_collections::linked_list::LinkOps
    }

    fn pointer_ops(&self) -> &Self::PointerOps {
        const OPS: intrusive_collections::DefaultPointerOps<TrackedEntityHandle<T>>;

        &OPS
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
    #[must_use]
    #[inline]
    pub fn clone(orig: &Self) -> Self {
        Self {
            value: orig.value,
            borrow: orig.borrow.clone(),
        }
    }

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

impl<T: ?Sized + fmt::Display> fmt::Display for EntityRef<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        (**self).fmt(f)
    }
}

/// A guard that provides exclusive access to an IR entity
pub struct EntityMut<'a, T> {
    value: NonNull<T>,
    borrow: BorrowRefMut<'b>,
    _marker: core::marker::PhantomData<&'b mut T>,
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

impl<T: ?Sized + fmt::Display> fmt::Display for EntityMut<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        (**self).fmt(f)
    }
}

/// An [EntityObj] is a wrapper around IR objects that are allocated in a [Context].
///
/// It ensures that any [EntityHandle] which references the underlying entity, adheres to Rust's
/// aliasing rules.
pub struct EntityObj<T> {
    borrow: Cell<BorrowFlag>,
    #[cfg(debug_assertions)]
    borrowed_at: Cell<Option<&'static core::panic::Location<'static>>>,
    cell: UnsafeCell<T>,
}

/// A [TrackedEntityObj] is a wrapper around IR entities that are linked in to an
/// [intrusive_collections::LinkedList] for tracking of that entity. This permits the linked list
/// to be visited/mutated without borrowing the entities themselves, and thus risk violation of
/// the aliasing rules.
pub struct TrackedEntityObj<T> {
    link: intrusive_collections::linked_list::LinkedListLink,
    entity: EntityObj<T>,
}
impl<T> TrackedEntityObj<T> {
    pub fn new(value: T) -> Self {
        Self {
            link: Default::default(),
            entity: EntityObj::new(value),
        }
    }
}

impl<T> EntityObj<T> {
    pub fn new(value: T) -> Self {
        Self {
            borrow: Cell::new(BorrowFlag::UNUSED),
            #[cfg(debug_assertions)]
            borrowed_at: Cell::new(None),
            cell: UnsafeCell::new(value),
        }
    }

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
                    if b.borrow.get() == 1 {
                        self.borrowed_at.set(Some(core::panic::Location::caller()));
                    }
                }

                // SAFETY: `BorrowRef` ensures that there is only immutable access to the value
                // while borrowed.
                let value = unsafe { NonNull::new_unchecked(self.value.get()) };
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
                let value = unsafe { NonNull::new_unchecked(self.value.get()) };
                Ok(EntityMut {
                    value,
                    borrow: b,
                    _marker: PhantomData,
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

    #[inline]
    pub fn as_ptr(&self) -> *mut T {
        self.cell.get()
    }

    #[inline]
    pub fn get_mut(&mut self) -> &mut T {
        self.cell.get_mut()
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
