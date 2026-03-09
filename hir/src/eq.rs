use core::any::Any;

use crate::{EntityMut, EntityRef};

/// A type-erased version of [PartialEq]
pub trait DynPartialEq: Any + 'static {
    fn dyn_eq(&self, rhs: &dyn PartialEqable) -> bool;
}

impl<T> DynPartialEq for T
where
    T: Any + PartialEq + 'static,
{
    #[inline]
    default fn dyn_eq(&self, rhs: &dyn PartialEqable) -> bool {
        rhs.eqable().downcast_ref::<T>().map(|rhs| self.eq(rhs)).unwrap_or(false)
    }
}

/// A trait implemented by all types that are valid operands for [DynPartialEq].
///
/// It can be used to override the concrete type that is used as the eqable value, and obtain
/// debugging information about that type (i.e. it's type name).
pub trait PartialEqable {
    fn equable_type_name(&self) -> &'static str;
    fn eqable(&self) -> &dyn core::any::Any;
}

impl<T: ?Sized + PartialEq + crate::any::AsAny + 'static> PartialEqable for T {
    #[inline]
    default fn equable_type_name(&self) -> &'static str {
        <T as crate::any::AsAny>::type_name(self)
    }

    #[inline]
    default fn eqable(&self) -> &dyn core::any::Any {
        <T as crate::any::AsAny>::as_any(self)
    }
}

impl<'a, T: ?Sized + PartialEq + crate::any::AsAny + 'static> PartialEqable for EntityRef<'a, T> {
    #[inline]
    fn equable_type_name(&self) -> &'static str {
        <T as crate::any::AsAny>::type_name(self)
    }

    #[inline]
    fn eqable(&self) -> &dyn core::any::Any {
        <T as crate::any::AsAny>::as_any(self)
    }
}

impl<'a, T: ?Sized + PartialEq + crate::any::AsAny + 'static> PartialEqable for EntityMut<'a, T> {
    #[inline]
    fn equable_type_name(&self) -> &'static str {
        <T as crate::any::AsAny>::type_name(self)
    }

    #[inline]
    fn eqable(&self) -> &dyn core::any::Any {
        <T as crate::any::AsAny>::as_any(self)
    }
}
