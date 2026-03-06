use core::ops::{Deref, DerefMut};

pub use miden_field::{Felt, Word};

pub use self::{
    crypto::Digest,
    felt::{assert, assert_eq, assertz},
};

pub mod advice;
pub mod crypto;
pub mod debug;
pub mod felt;

/// A wrapper type which ensures that the wrapped value is aligned to 32 bytes.
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
