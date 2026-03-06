use super::*;

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

unsafe impl<T, Link> intrusive_collections::PointerOps
    for DefaultPointerOps<RawEntityRef<T, Link>>
{
    type Pointer = RawEntityRef<T, Link>;
    type Value = RawEntityMetadata<T, Link>;

    #[inline]
    unsafe fn from_raw(&self, value: *const Self::Value) -> Self::Pointer {
        debug_assert!(!value.is_null() && value.is_aligned());
        unsafe { RawEntityRef::from_ptr(value.cast_mut()) }
    }

    #[inline]
    fn into_raw(&self, ptr: Self::Pointer) -> *const Self::Value {
        RawEntityRef::into_inner(ptr).as_ptr().cast_const()
    }
}

/// An adapter for storing any `Entity` impl in a [intrusive_collections::LinkedList]
pub struct EntityAdapter<T, Link, LinkOps> {
    pub(super) link_ops: LinkOps,
    pub(super) ptr_ops: DefaultPointerOps<RawEntityRef<T, Link>>,
    marker: core::marker::PhantomData<T>,
}
impl<T, Link, LinkOps: Copy> Copy for EntityAdapter<T, Link, LinkOps> {}
impl<T, Link, LinkOps: Clone> Clone for EntityAdapter<T, Link, LinkOps> {
    fn clone(&self) -> Self {
        Self {
            link_ops: self.link_ops.clone(),
            ptr_ops: self.ptr_ops,
            marker: core::marker::PhantomData,
        }
    }
}
impl<T, Link, LinkOps: Default> Default for EntityAdapter<T, Link, LinkOps> {
    fn default() -> Self {
        Self::new()
    }
}
impl<T, Link, LinkOps: Default> EntityAdapter<T, Link, LinkOps> {
    pub fn new() -> Self {
        Self {
            link_ops: LinkOps::default(),
            ptr_ops: DefaultPointerOps::new(),
            marker: core::marker::PhantomData,
        }
    }
}
