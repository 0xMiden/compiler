use alloc::rc::Rc;
use core::{borrow::Borrow, ops::Deref};

use crate::{AsAny, AttributeValue, Builder, OperationName, OperationRef, SourceSpan, Type};

/// A [Dialect] represents a collection of IR entities that are used in conjunction with one
/// another. Multiple dialects can co-exist _or_ be mutually exclusive. Converting between dialects
/// is the job of the conversion infrastructure, using a process called _legalization_.
pub trait Dialect {
    /// Get the name(space) of this dialect
    fn name(&self) -> DialectName;
    /// Get the set of registered operations associated with this dialect
    fn registered_ops(&self) -> Rc<[OperationName]>;
    /// Get the registered [OperationName] for an op `opcode`, or register it with `register`.
    ///
    /// Registering an operation with the dialect allows various parts of the IR to introspect the
    /// set of operations which belong to a given dialect namespace.
    fn get_or_register_op(
        &self,
        opcode: ::midenc_hir_symbol::Symbol,
        register: fn(DialectName, ::midenc_hir_symbol::Symbol) -> OperationName,
    ) -> OperationName;

    /// A hook to materialize a single constant operation from a given attribute value and type.
    ///
    /// This method should use the provided builder to create the operation without changing the
    /// insertion point. The generated operation is expected to be constant-like, i.e. single result
    /// zero operands, no side effects, etc.
    ///
    /// Returns `None` if a constant cannot be materialized for the given attribute.
    #[allow(unused_variables)]
    #[inline]
    fn materialize_constant(
        &self,
        builder: &mut dyn Builder,
        attr: Box<dyn AttributeValue>,
        ty: &Type,
        span: SourceSpan,
    ) -> Option<OperationRef> {
        None
    }
}

/// A [DialectRegistration] must be implemented for any implementation of [Dialect], to allow the
/// dialect to be registered with a [crate::Context] and instantiated on demand when building ops
/// in the IR.
///
/// This is not part of the [Dialect] trait itself, as that trait must be object safe, and this
/// trait is _not_ object safe.
pub trait DialectRegistration: AsAny + Dialect {
    /// The namespace of the dialect to register
    ///
    /// A dialect namespace serves both as a way to namespace the operations of that dialect, as
    /// well as a way to uniquely name/identify the dialect itself. Thus, no two dialects can have
    /// the same namespace at the same time.
    const NAMESPACE: &'static str;

    /// Initialize an instance of this dialect to be stored (uniqued) in the current
    /// [crate::Context].
    ///
    /// A dialect will only ever be initialized once per context. A dialect must use interior
    /// mutability to satisfy the requirements of the [Dialect] trait, and to allow the context to
    /// store the returned instance in a reference-counted smart pointer.
    fn init() -> Self;
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

    pub const fn as_symbol(&self) -> ::midenc_hir_symbol::Symbol {
        self.0
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
impl Borrow<::midenc_hir_symbol::Symbol> for DialectName {
    #[inline(always)]
    fn borrow(&self) -> &::midenc_hir_symbol::Symbol {
        &self.0
    }
}
impl Borrow<str> for DialectName {
    #[inline(always)]
    fn borrow(&self) -> &str {
        self.0.as_str()
    }
}
