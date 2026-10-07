//! A wrapper that aligns its contents to a VM word boundary.

use core::ops::{Deref, DerefMut};

/// A wrapper type which ensures that the wrapped value is aligned to a VM word boundary.
///
/// A word is 4 felts of 4 bytes each, 16 bytes, on the Miden target; the wrapper aligns to
/// 32 bytes, a multiple of that.
#[repr(C, align(32))]
pub struct WordAligned<T>(T);
impl<T> WordAligned<T> {
    #[inline(always)]
    /// Wraps the provided value.
    pub const fn new(t: T) -> Self {
        Self(t)
    }

    #[inline(always)]
    /// Returns the wrapped value.
    pub fn into_inner(self) -> T {
        self.0
    }
}
impl<T> From<T> for WordAligned<T> {
    #[inline(always)]
    fn from(t: T) -> Self {
        Self(t)
    }
}
impl<T> AsRef<T> for WordAligned<T> {
    #[inline(always)]
    fn as_ref(&self) -> &T {
        &self.0
    }
}
impl<T> AsMut<T> for WordAligned<T> {
    #[inline(always)]
    fn as_mut(&mut self) -> &mut T {
        &mut self.0
    }
}
impl<T> Deref for WordAligned<T> {
    type Target = T;

    #[inline(always)]
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl<T> DerefMut for WordAligned<T> {
    #[inline(always)]
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}
