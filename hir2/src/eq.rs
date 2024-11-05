use core::any::Any;

/// A type-erased version of [PartialEq]
pub trait DynPartialEq: Any {
    fn dyn_eq(&self, rhs: &dyn DynPartialEq) -> bool;
}

impl<T> DynPartialEq for T
where
    T: PartialEq + 'static,
{
    #[inline]
    default fn dyn_eq(&self, rhs: &dyn DynPartialEq) -> bool {
        let rhs = rhs as &dyn Any;
        rhs.downcast_ref::<T>().map(|rhs| self.eq(rhs)).unwrap_or(false)
    }
}
