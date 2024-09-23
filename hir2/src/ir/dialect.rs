use core::ops::Deref;

/// A [Dialect] represents a collection of IR entities that are used in conjunction with one
/// another. Multiple dialects can co-exist _or_ be mutually exclusive. Converting between dialects
/// is the job of the conversion infrastructure, using a process called _legalization_.
pub trait Dialect {
    const INIT: Self;

    fn name(&self) -> DialectName;
}

/// A strongly-typed symbol representing the name of a [Dialect].
///
/// Dialect names should be in lowercase ASCII format, though this is not enforced.
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DialectName(::midenc_hir_symbol::Symbol);
impl DialectName {
    pub fn new<S>(name: S) -> Self
    where
        S: Into<::midenc_hir_symbol::Symbol>,
    {
        Self(name.into())
    }

    pub const fn from_symbol(name: ::midenc_hir_symbol::Symbol) -> Self {
        Self(name)
    }
}
impl core::fmt::Debug for DialectName {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.0.as_str())
    }
}
impl core::fmt::Display for DialectName {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.0.as_str())
    }
}
impl From<::midenc_hir_symbol::Symbol> for DialectName {
    #[inline(always)]
    fn from(value: ::midenc_hir_symbol::Symbol) -> Self {
        Self(value)
    }
}
impl From<DialectName> for ::midenc_hir_symbol::Symbol {
    #[inline(always)]
    fn from(value: DialectName) -> Self {
        value.0
    }
}
impl Deref for DialectName {
    type Target = ::midenc_hir_symbol::Symbol;

    #[inline(always)]
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl AsRef<::midenc_hir_symbol::Symbol> for DialectName {
    #[inline(always)]
    fn as_ref(&self) -> &::midenc_hir_symbol::Symbol {
        &self.0
    }
}
