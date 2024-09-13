use core::ptr::NonNull;

#[derive(Copy, Clone)]
#[repr(transparent)]
pub struct UnsafeRef<T: ?Sized>(NonNull<T>);

impl<T: ?Sized> UnsafeRef<T> {
    /// Construct a new [UnsafeRef] from a non-null pointer to `T`
    pub fn new(ptr: NonNull<T>) -> Self {
        Self(ptr)
    }

    /// Get the underlying raw pointer for this [UnsafeRef]
    #[inline(always)]
    pub const fn into_raw(self) -> NonNull<T> {
        self.0
    }

    /// Construct an [UnsafeRef] from a [Box]
    pub fn from_box(ptr: Box<T>) -> Self {
        Self(unsafe { NonNull::new_unchecked(Box::into_raw(ptr)) })
    }

    /// Convert this [UnsafeRef] back into the [Box] it was derived from.
    ///
    /// # Safety
    ///
    /// The following must be upheld by the caller:
    ///
    /// * This [UnsafeRef] _MUST_ have been created via [UnsafeRef::from_box]
    /// * There _MUST NOT_ be any other [UnsafeRef] pointing to the same allocation
    /// * `T` must be the same type as the original [Box] was allocated with
    pub unsafe fn into_box(self) -> Box<T> {
        Box::from_raw(self.0.as_ptr())
    }
}

impl<T: ?Sized> core::ops::Deref for UnsafeRef<T> {
    type Target = T;

    #[inline]
    fn deref(&self) -> &Self::Target {
        unsafe { self.0.as_ref() }
    }
}

impl<T: ?Sized> AsRef<T> for UnsafeRef<T> {
    fn as_ref(&self) -> &T {
        unsafe { self.0.as_ref() }
    }
}

impl<T, U> AsRef<U> for UnsafeRef<T>
where
    T: core::marker::Unsize<U> + ?Sized,
    U: ?Sized,
{
    fn as_ref(&self) -> &U {
        unsafe { self.0.as_ref() as &U }
    }
}

impl<T: ?Sized> core::borrow::Borrow for UnsafeRef<T> {
    fn borrow(&self) -> &T {
        unsafe { self.0.as_ref() }
    }
}

impl<T, U> core::ops::CoerceUnsized<UnsafeRef<U>> for UnsafeRef<T>
where
    T: core::marker::Unsize<U> + ?Sized,
    U: ?Sized,
{
}

impl<T, U> core::ops::DispatchFromDyn<UnsafeRef<U>> for UnsafeRef<T>
where
    T: core::marker::Unsize<U> + ?Sized,
    U: ?Sized,
{
}

unsafe impl<T: ?Sized + Send + Sync> Send for UnsafeRef<T> {}

unsafe impl<T: ?Sized + Sync> Sync for UnsafeRef<T> {}

unsafe impl<T: ?Sized> intrusive_collections::PointerOps
    for intrusive_collections::DefaultPointerOps<UnsafeRef<T>>
{
    type Pointer = UnsafeRef<T>;
    type Value = T;

    unsafe fn from_raw(&self, value: *const Self::Value) -> Self::Pointer {
        let value = NonNull::new(value.cast_mut()).expect("expected non-null node pointer");
        UnsafeRef::new(value)
    }

    fn into_raw(&self, ptr: Self::Pointer) -> *const Self::Value {
        ptr.into_raw().as_ptr().cast_const()
    }
}
