use alloc::{boxed::Box, rc::Rc, vec::Vec};
use core::{
    any::TypeId,
    fmt,
    ptr::{DynMetadata, Pointee},
};

use super::{OpRegistration, OperationRef};
use crate::{
    Attribute, AttributeRef, AttributeRegistration, Context, EntityMut, EntityRef, OperationState,
    UnsafeIntrusiveEntityRef, interner, parse,
    patterns::RewritePatternSet,
    traits::{Canonicalizable, TraitInfo},
};

/// The operation name, or mnemonic, that uniquely identifies an operation.
///
/// The operation name consists of its dialect name, and the opcode name within the dialect.
///
/// No two operation names can share the same fully-qualified operation name.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct OperationName(Rc<OperationInfo>);

struct OperationInfo {
    /// The dialect of this operation
    dialect: interner::Symbol,
    /// The opcode name for this operation
    name: interner::Symbol,
    /// The type id of the concrete type that implements this operation
    type_id: TypeId,
    /// Details of the traits implemented by this operation, used to answer questions about what
    /// traits are implemented, as well as reconstruct `&dyn Trait` references given a pointer to
    /// the data of a specific operation instance.
    traits: Box<[TraitInfo]>,
    /// Details of the attributes defined as part of this operation
    attrs: Box<[AttrInfo]>,
    /// The implementation of `Canonicalizable::get_canonicalization_patterns` for this type
    get_canonicalization_patterns: fn(&mut RewritePatternSet, Rc<Context>),
    /// A type-erased allocator function to obtain an uninitialized instance of this op
    alloc_default: AllocDefaultFn,
    /// The custom assembly parser for this operation
    parse_assembly: Option<ParseAssemblyFn>,
}

type AttrValueGetRaw = unsafe fn(*const (), &AttrInfo) -> AttributeRef;

type AttrValueGet = for<'a> unsafe fn(
    *const (),
    &AttrInfo,
    core::marker::PhantomData<fn(&'a ())>,
) -> EntityRef<'a, dyn Attribute>;

type AttrValueGetMut = for<'a> unsafe fn(
    *mut (),
    &AttrInfo,
    core::marker::PhantomData<fn(&'a mut ())>,
) -> EntityMut<'a, dyn Attribute>;

type TryFromAttr =
    unsafe fn(*mut (), &AttrInfo, AttributeRef) -> Result<(), crate::diagnostics::Report>;

/// This trait provides for type-erased writes to an attribute property of an [Operation].
///
/// Implementations must use the provided pointer, derive a pointer to the field holding the
/// [UnsafeIntrusiveEntityRef] of the current attribute value via the provided [AttrInfo], and
/// then check that the provided value is of the same type as expected by the field (or is
/// convertible to it), and either write the updated value, or return a diagnostic.
///
/// # SAFETY
///
/// Implementors of this trait can assume the following, and _only_ the following:
///
/// 1. `op` is a pointer to an [Op] implementation
/// 2. `info` contains trusted metadata about the field to write to. Namely, deriving a pointer to
///    the attribute field using the offset in that metadata is guaranteed to be correct.
/// 3. `value` is a valid reference to some `Attribute` implementation, i.e. it is not dangling.
///
/// Implementors of this trait _cannot_ assume the following:
///
/// 1. The current value of the field holds a valid [UnsafeIntrusiveEntityRef]. This trait is
///    used during initialization, where the initial value of all property fields is a dangling
///    pointer. Implementors _MUST NOT_ attempt to borrow or dereference the field value.
/// 2. That `value` is of the same type as expected by the field being written to. In general, it
///    should always be the correct type, but when parsing the IR, operations are constructed
///    generically, and so we rely on implementations of this trait to catch type errors at runtime.
///    This is also why this trait returns a diagnostic, rather than panicking.
unsafe trait TryFromAttribute {
    unsafe fn try_from_attribute_value(
        op: *mut (),
        info: &AttrInfo,
        value: AttributeRef,
    ) -> Result<(), crate::diagnostics::Report>;
}

unsafe impl<T: AttributeRegistration> TryFromAttribute for T {
    unsafe fn try_from_attribute_value(
        op: *mut (),
        info: &AttrInfo,
        value: AttributeRef,
    ) -> Result<(), crate::diagnostics::Report> {
        use alloc::format;
        let attr = value.borrow().as_attr().as_attr_ref();
        let typed_attr = value.try_downcast::<T>().ok();
        if let Some(attr) = typed_attr {
            let offset = info.offset as usize;
            unsafe {
                let ptr = op.byte_add(offset).cast::<UnsafeIntrusiveEntityRef<T>>();
                *ptr = attr;
            }
            Ok(())
        } else {
            use crate::any::AsAny;
            let attr = attr.borrow();
            let value = attr.as_trait::<dyn AsAny>().unwrap();
            Err(crate::diagnostics::Report::msg(format!(
                "could not convert attribute of type '{}' to '{}' for property '{}'",
                value.type_name(),
                info.type_name,
                &info.name,
            )))
        }
    }
}

#[derive(Debug)]
pub struct AttrInfo {
    pub name: interner::Symbol,
    pub type_id: TypeId,
    pub type_name: &'static str,
    pub hidden: bool,
    /// The byte offset from the start of the concrete type, to the field which holds this
    /// attribute.
    pub offset: u16,
    /// A function which returns a reference to the [Attribute]
    pub get: AttrValueGet,
    /// A function which returns a mutable reference to the [Attribute]
    pub get_mut: AttrValueGetMut,
    /// A function which returns an [AttributeRef] for this attribute
    pub get_raw: AttrValueGetRaw,
    /// A function which attempts to convert the given [AttributeRef] to the concrete type of the
    /// field, or returns an error if the value is not valid.
    pub try_from: TryFromAttr,
}

impl AttrInfo {
    #[doc(hidden)]
    pub unsafe fn new<T: AttributeRegistration>(
        name: interner::Symbol,
        offset: u16,
        hidden: bool,
    ) -> Self {
        let type_id = core::any::TypeId::of::<T>();
        let type_name = core::any::type_name::<T>();
        Self {
            name,
            type_id,
            type_name,
            hidden,
            offset,
            get: Self::get::<T>,
            get_mut: Self::get_mut::<T>,
            get_raw: Self::get_raw::<T>,
            try_from: Self::try_from::<T>,
        }
    }

    unsafe fn get<'a, T: Attribute>(
        op: *const (),
        info: &AttrInfo,
        _marker: core::marker::PhantomData<fn(&'a ())>,
    ) -> EntityRef<'a, dyn Attribute> {
        let offset = info.offset as usize;
        unsafe {
            let ptr = op.byte_add(offset).cast::<UnsafeIntrusiveEntityRef<T>>();
            EntityRef::map((&*ptr).borrow(), |attr| attr as &dyn Attribute)
        }
    }

    unsafe fn get_mut<'a, T: Attribute>(
        op: *mut (),
        info: &AttrInfo,
        _marker: core::marker::PhantomData<fn(&'a mut ())>,
    ) -> EntityMut<'a, dyn crate::Attribute> {
        let offset = info.offset as usize;
        unsafe {
            let ptr = op.byte_add(offset).cast::<UnsafeIntrusiveEntityRef<T>>();
            EntityMut::map((&mut *ptr).borrow_mut(), |attr| attr as &mut dyn Attribute)
        }
    }

    unsafe fn get_raw<T: Attribute>(op: *const (), info: &AttrInfo) -> AttributeRef {
        let offset = info.offset as usize;
        unsafe {
            let ptr = op.byte_add(offset).cast::<UnsafeIntrusiveEntityRef<T>>();
            (*ptr).as_attribute_ref()
        }
    }

    unsafe fn try_from<T>(
        op: *mut (),
        info: &AttrInfo,
        value: AttributeRef,
    ) -> Result<(), crate::diagnostics::Report>
    where
        T: TryFromAttribute,
    {
        unsafe { T::try_from_attribute_value(op, info, value) }
    }
}

type AllocDefaultFn = fn(Rc<Context>) -> OperationRef;

pub type ParseAssemblyFn =
    fn(&mut OperationState, &mut dyn parse::OpAsmParser<'_>) -> parse::ParseResult;

trait MaybeOpParser {
    fn parse_assembly() -> Option<ParseAssemblyFn>;
}

impl<T> MaybeOpParser for T {
    #[inline(always)]
    default fn parse_assembly() -> Option<ParseAssemblyFn> {
        None
    }
}

impl<T: OpRegistration + parse::OpParser> MaybeOpParser for T {
    #[inline(always)]
    fn parse_assembly() -> Option<ParseAssemblyFn> {
        Some(<T as parse::OpParser>::parse)
    }
}

impl OperationName {
    pub fn new<T>(dialect: interner::Symbol, mut extra_traits: Vec<TraitInfo>) -> Self
    where
        T: OpRegistration,
    {
        let type_id = TypeId::of::<T>();
        let attrs = <T as OpRegistration>::attrs();
        let mut traits = Vec::from(<T as OpRegistration>::traits());
        traits.append(&mut extra_traits);
        traits.sort_by_key(|ti| *ti.type_id());
        let get_canonicalization_patterns = <T as Canonicalizable>::get_canonicalization_patterns;
        let alloc_default = <T as OpRegistration>::alloc_uninit;
        let parse_assembly = <T as MaybeOpParser>::parse_assembly();
        Self(Rc::new(OperationInfo {
            dialect,
            name: <T as OpRegistration>::name(),
            type_id,
            attrs,
            traits: traits.into_boxed_slice(),
            get_canonicalization_patterns,
            alloc_default,
            parse_assembly,
        }))
    }

    /// Returns the dialect name of this operation
    pub fn dialect(&self) -> interner::Symbol {
        self.0.dialect
    }

    /// Returns the name/opcode of this operation
    pub fn name(&self) -> interner::Symbol {
        self.0.name
    }

    /// Returns the [TypeId] of the operation type
    #[inline]
    pub fn id(&self) -> &TypeId {
        &self.0.type_id
    }

    /// Allocates a new default-initialized instance of this operation
    #[inline]
    pub fn alloc_default(&self, context: Rc<Context>) -> OperationRef {
        (self.0.alloc_default)(context)
    }

    /// Returns the custom assembly parser function for this operation, if it has one.
    #[inline(always)]
    pub fn parse_assembly_fn(&self) -> Option<ParseAssemblyFn> {
        self.0.parse_assembly
    }

    /// Populates `rewrites` with the set of canonicalization patterns registered for this operation
    pub fn populate_canonicalization_patterns(
        &self,
        rewrites: &mut RewritePatternSet,
        context: Rc<Context>,
    ) {
        (self.0.get_canonicalization_patterns)(rewrites, context)
    }

    /// Get metadata about all properties of this operation
    #[inline]
    pub fn properties(&self) -> &[AttrInfo] {
        &self.0.attrs
    }

    /// Get the property (i.e. intrinsic attribute) named `name` of this operation, if present.
    pub fn get_property(&self, name: interner::Symbol) -> Option<&AttrInfo> {
        self.0.attrs.iter().find(|attr| attr.name == name)
    }

    /// Returns true if this operation has a property (i.e. intrinsic attribute) named `name`
    pub fn has_property(&self, name: interner::Symbol) -> bool {
        self.0.attrs.iter().any(|attr| attr.name == name)
    }

    /// Returns true if `T` is the concrete type that implements this operation
    #[inline]
    pub fn is<T: 'static>(&self) -> bool {
        TypeId::of::<T>() == self.0.type_id
    }

    /// Returns true if this operation implements `Trait`
    pub fn implements<Trait>(&self) -> bool
    where
        Trait: ?Sized + Pointee<Metadata = DynMetadata<Trait>> + 'static,
    {
        let type_id = TypeId::of::<Trait>();
        self.implements_trait_id(&type_id)
    }

    /// Returns true if this operation implements `trait`, where `trait` is the `TypeId` of a
    /// `dyn Trait` type.
    pub fn implements_trait_id(&self, trait_id: &TypeId) -> bool {
        self.0.traits.binary_search_by(|ti| ti.type_id().cmp(trait_id)).is_ok()
    }

    #[inline]
    pub(super) fn downcast_ref<T: 'static>(&self, ptr: *const ()) -> Option<&T> {
        if self.is::<T>() {
            Some(unsafe { self.downcast_ref_unchecked(ptr) })
        } else {
            None
        }
    }

    #[inline(always)]
    unsafe fn downcast_ref_unchecked<T: 'static>(&self, ptr: *const ()) -> &T {
        unsafe { &*core::ptr::from_raw_parts(ptr.cast::<T>(), ()) }
    }

    #[inline]
    pub(super) fn downcast_mut<T: 'static>(&mut self, ptr: *mut ()) -> Option<&mut T> {
        if self.is::<T>() {
            Some(unsafe { self.downcast_mut_unchecked(ptr) })
        } else {
            None
        }
    }

    #[inline(always)]
    unsafe fn downcast_mut_unchecked<T: 'static>(&mut self, ptr: *mut ()) -> &mut T {
        unsafe { &mut *core::ptr::from_raw_parts_mut(ptr.cast::<T>(), ()) }
    }

    pub(super) fn upcast<Trait>(&self, ptr: *const ()) -> Option<&Trait>
    where
        Trait: ?Sized + Pointee<Metadata = DynMetadata<Trait>> + 'static,
    {
        let metadata = self
            .get::<Trait>()
            .map(|trait_impl| unsafe { trait_impl.metadata_unchecked::<Trait>() })?;
        Some(unsafe { &*core::ptr::from_raw_parts(ptr, metadata) })
    }

    /// Rebuilds a raw trait object pointer for `ptr` using metadata registered for `Trait`.
    pub(super) fn upcast_raw<Trait>(&self, ptr: *const ()) -> Option<*const Trait>
    where
        Trait: ?Sized + Pointee<Metadata = DynMetadata<Trait>> + 'static,
    {
        let metadata = self
            .get::<Trait>()
            .map(|trait_impl| unsafe { trait_impl.metadata_unchecked::<Trait>() })?;
        Some(core::ptr::from_raw_parts(ptr, metadata))
    }

    pub(super) fn upcast_mut<Trait>(&mut self, ptr: *mut ()) -> Option<&mut Trait>
    where
        Trait: ?Sized + Pointee<Metadata = DynMetadata<Trait>> + 'static,
    {
        let metadata = self
            .get::<Trait>()
            .map(|trait_impl| unsafe { trait_impl.metadata_unchecked::<Trait>() })?;
        Some(unsafe { &mut *core::ptr::from_raw_parts_mut(ptr, metadata) })
    }

    #[inline]
    fn get<Trait: ?Sized + 'static>(&self) -> Option<&TraitInfo> {
        let type_id = TypeId::of::<Trait>();
        let traits = self.0.traits.as_ref();
        traits
            .binary_search_by(|ti| ti.type_id().cmp(&type_id))
            .ok()
            .map(|index| &traits[index])
    }
}
impl fmt::Debug for OperationName {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}
impl fmt::Display for OperationName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}", &self.dialect(), &self.name())
    }
}

impl Eq for OperationInfo {}
impl PartialEq for OperationInfo {
    fn eq(&self, other: &Self) -> bool {
        self.dialect == other.dialect && self.name == other.name && self.type_id == other.type_id
    }
}
impl PartialOrd for OperationInfo {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for OperationInfo {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        self.dialect
            .cmp(&other.dialect)
            .then_with(|| self.name.cmp(&other.name))
            .then_with(|| self.type_id.cmp(&other.type_id))
    }
}
impl core::hash::Hash for OperationInfo {
    fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
        self.dialect.hash(state);
        self.name.hash(state);
        self.type_id.hash(state);
    }
}
