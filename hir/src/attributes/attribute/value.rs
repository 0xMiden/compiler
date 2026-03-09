use core::clone::CloneToUninit;

use crate::any::AsAny;

pub trait AttributeValue: AsAny + CloneToUninit {}

impl<T: AsAny + CloneToUninit> AttributeValue for T {}

impl dyn AttributeValue {
    /// Returns true if this attribute is an instance of type `T`
    pub fn is<T: AsAny>(&self) -> bool {
        self.as_any().is::<T>()
    }

    /// Attempts to downcast a `&dyn Attribute` to `&T`, if the value is an instance of type `T`.
    pub fn downcast_ref<T: AsAny>(&self) -> Option<&T> {
        self.as_any().downcast_ref::<T>()
    }

    /// Attempts to downcast this attribute value reference to `&mut T`, if the value is an instance
    /// of type `T`.
    ///
    /// Returns `None` if this value is not of type `T`.
    pub fn downcast_mut<T: AsAny>(&mut self) -> Option<&mut T> {
        self.as_any_mut().downcast_mut::<T>()
    }
}
