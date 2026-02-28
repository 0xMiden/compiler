mod info;

use alloc::rc::Rc;
use core::ptr::{DynMetadata, Pointee};

pub use self::info::DialectInfo;
use crate::{
    AttributeRef, Builder, OperationName, OperationRef, SourceSpan, Type, any::AsAny,
    attributes::AttributeName, interner,
};

pub type DialectRegistrationHook = fn(&mut DialectInfo);

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

    /// Get the set of registered attributes associated with this dialect
    fn registered_attrs(&self) -> &[AttributeName] {
        self.info().attributes()
    }

    /// A hook to materialize a single constant operation from a given attribute value.
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
        attr: AttributeRef,
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

    /// Get the [AttributeName] of the attribute type `T`, if registered with this dialect.
    pub fn registered_attribute_name<T>(&self) -> Option<AttributeName>
    where
        T: crate::AttributeRegistration,
    {
        let name = <T as crate::AttributeRegistration>::name();
        self.registered_attrs().iter().find(|attr| attr.name() == name).cloned()
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

    /// Get the [AttributeName] of the operation type `T`.
    ///
    /// Panics if the attribute is not registered with this dialect.
    pub fn expect_registered_attribute_name<T>(&self) -> AttributeName
    where
        T: crate::AttributeRegistration,
    {
        self.registered_attribute_name::<T>().unwrap_or_else(|| {
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

    /// This is called when registering a dialect, to register attributes of the dialect.
    ///
    /// This is called _before_ [DialectRegistration::init].
    #[allow(unused_variables)]
    fn register_attributes(info: &mut DialectInfo) {}
}

inventory::collect!(DialectRegistrationInfo);
inventory::collect!(DialectRegistrationHookInfo);
inventory::collect!(DialectOpRegistrationInfo);
inventory::collect!(DialectAttributeRegistrationInfo);

#[doc(hidden)]
#[repr(transparent)]
pub struct DialectRegistrationInfo(DialectRegistrationEntry);

#[doc(hidden)]
#[repr(transparent)]
pub struct DialectRegistrationHookInfo(DialectRegistrationHookEntry);

#[doc(hidden)]
#[repr(transparent)]
pub struct DialectOpRegistrationInfo(DialectOpRegistrationEntry);

#[doc(hidden)]
#[repr(transparent)]
pub struct DialectAttributeRegistrationInfo(DialectAttributeRegistrationEntry);

#[repr(C)]
struct DialectRegistrationEntry {
    namespace: &'static str,
    type_name: &'static str,
    type_id: core::any::TypeId,
    builder: fn() -> Rc<dyn Dialect>,
}

impl DialectRegistrationInfo {
    pub const fn new<T: DialectRegistration>() -> Self {
        let namespace = <T as DialectRegistration>::NAMESPACE;
        Self(DialectRegistrationEntry {
            namespace,
            type_name: core::any::type_name::<T>(),
            type_id: core::any::TypeId::of::<T>(),
            builder: dialect_init::<T>,
        })
    }

    pub(crate) const fn namespace(&self) -> &'static str {
        self.0.namespace
    }

    pub(crate) const fn type_name(&self) -> &'static str {
        self.0.type_name
    }

    #[allow(unused)]
    pub(crate) const fn type_id(&self) -> &core::any::TypeId {
        &self.0.type_id
    }

    pub(crate) fn create(&self) -> Rc<dyn Dialect> {
        (self.0.builder)()
    }
}

#[repr(C)]
struct DialectRegistrationHookEntry {
    namespace: &'static str,
    type_name: &'static str,
    type_id: core::any::TypeId,
    hook: DialectRegistrationHook,
}

impl DialectRegistrationHookInfo {
    pub const fn new<T: DialectRegistration>(hook: DialectRegistrationHook) -> Self {
        let namespace = <T as DialectRegistration>::NAMESPACE;
        Self(DialectRegistrationHookEntry {
            namespace,
            type_name: core::any::type_name::<T>(),
            type_id: core::any::TypeId::of::<T>(),
            hook,
        })
    }
}

#[repr(C)]
struct DialectOpRegistrationEntry {
    dialect: &'static str,
    dialect_type: core::any::TypeId,
    get_opcode: fn() -> interner::Symbol,
    init: fn(interner::Symbol, alloc::vec::Vec<super::traits::TraitInfo>) -> OperationName,
}

impl DialectOpRegistrationInfo {
    pub const fn new<T: super::OpRegistration>() -> Self {
        let dialect = <<T as super::OpRegistration>::Dialect as DialectRegistration>::NAMESPACE;
        let dialect_type = core::any::TypeId::of::<<T as super::OpRegistration>::Dialect>();
        Self(DialectOpRegistrationEntry {
            dialect,
            dialect_type,
            get_opcode: <T as super::OpRegistration>::name,
            init: OperationName::new::<T>,
        })
    }
}

#[repr(C)]
struct DialectAttributeRegistrationEntry {
    dialect: &'static str,
    dialect_type: core::any::TypeId,
    get_name: fn() -> interner::Symbol,
    init: fn(interner::Symbol, alloc::vec::Vec<super::traits::TraitInfo>) -> AttributeName,
}

impl DialectAttributeRegistrationInfo {
    pub const fn new<T: crate::AttributeRegistration>() -> Self {
        let dialect =
            <<T as crate::AttributeRegistration>::Dialect as DialectRegistration>::NAMESPACE;
        let dialect_type = core::any::TypeId::of::<<T as crate::AttributeRegistration>::Dialect>();
        Self(DialectAttributeRegistrationEntry {
            dialect,
            dialect_type,
            get_name: <T as crate::AttributeRegistration>::name,
            init: AttributeName::new::<T>,
        })
    }
}

pub(super) fn dialect_init<T: DialectRegistration>() -> Rc<dyn Dialect> {
    let mut info = DialectInfo::new::<T>();

    let dialect_name = <T as DialectRegistration>::NAMESPACE;
    let dialect_type = core::any::TypeId::of::<T>();

    for dialect_hook in inventory::iter::<DialectRegistrationHookInfo>() {
        if dialect_hook.0.type_id == dialect_type && dialect_hook.0.namespace == dialect_name {
            (dialect_hook.0.hook)(&mut info);
        }
    }

    for op in inventory::iter::<DialectOpRegistrationInfo>() {
        if op.0.dialect_type == dialect_type && op.0.dialect == dialect_name {
            let opcode = (op.0.get_opcode)();
            info.get_or_register_with(opcode, op.0.init);
        }
    }

    for attr in inventory::iter::<DialectAttributeRegistrationInfo>() {
        if attr.0.dialect_type == dialect_type && attr.0.dialect == dialect_name {
            let name = (attr.0.get_name)();
            info.get_or_register_attribute_with(name, attr.0.init);
        }
    }

    Rc::new(<T as DialectRegistration>::init(info)) as Rc<dyn Dialect>
}
