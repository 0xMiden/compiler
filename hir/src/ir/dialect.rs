mod info;

use alloc::boxed::Box;
use core::ptr::{DynMetadata, Pointee};

pub use self::info::DialectInfo;
use crate::{
    any::AsAny, interner, AttributeValue, Builder, OperationName, OperationRef, SourceSpan, Type,
};

pub type DialectRegistrationHook = Box<dyn Fn(&mut DialectInfo, &super::Context)>;

/// A [Dialect] represents a collection of IR entities that are used in conjunction with one
/// another. Multiple dialects can co-exist _or_ be mutually exclusive. Converting between dialects
/// is the job of the conversion infrastructure, using a process called _legalization_.
pub trait Dialect {
    /// Get metadata about this dialect (it's operations, interfaces, etc.)
    fn info(&self) -> &DialectInfo;

    /// Get the name(space) of this dialect
    fn name(&self) -> interner::Symbol {
        self.info().name()
    }

    /// Get the set of registered operations associated with this dialect
    fn registered_ops(&self) -> &[OperationName] {
        self.info().operations()
    }

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

impl dyn Dialect {
    /// Get the [OperationName] of the operation type `T`, if registered with this dialect.
    pub fn registered_name<T>(&self) -> Option<OperationName>
    where
        T: crate::OpRegistration,
    {
        let opcode = <T as crate::OpRegistration>::name();
        self.registered_ops().iter().find(|op| op.name() == opcode).cloned()
    }

    /// Get the [OperationName] of the operation type `T`.
    ///
    /// Panics if the operation is not registered with this dialect.
    pub fn expect_registered_name<T>(&self) -> OperationName
    where
        T: crate::OpRegistration,
    {
        self.registered_name::<T>().unwrap_or_else(|| {
            panic!(
                "{} is not registered with dialect '{}'",
                core::any::type_name::<T>(),
                self.name()
            )
        })
    }

    /// Attempt to cast this operation reference to an implementation of `Trait`
    pub fn as_registered_interface<Trait>(&self) -> Option<&Trait>
    where
        Trait: ?Sized + Pointee<Metadata = DynMetadata<Trait>> + 'static,
    {
        let this = self as *const dyn Dialect;
        let (ptr, _) = this.to_raw_parts();
        let info = self.info();
        info.upcast(ptr)
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
    fn init(info: DialectInfo) -> Self;

    /// This is called when registering a dialect, to register operations of the dialect.
    ///
    /// This is called _before_ [DialectRegistration::init].
    fn register_operations(info: &mut DialectInfo);
}
