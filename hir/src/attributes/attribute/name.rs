use alloc::{boxed::Box, rc::Rc, vec::Vec};
use core::{
    any::TypeId,
    fmt,
    ptr::{DynMetadata, Pointee},
};

use super::{Attribute, AttributeRef, AttributeRegistration};
use crate::{
    Context, UnsafeIntrusiveEntityRef, attributes::AttrParser, interner, parse, traits::TraitInfo,
};

/// The attribute name, or mnemonic, that uniquely identifies an attribute.
///
/// The attribute name consists of its dialect namespace, and the attribute name itself.
///
/// No two attribute names can share the same fully-qualified attribute name.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AttributeName(Rc<AttributeInfo>);

struct AttributeInfo {
    /// The dialect of this attribute
    dialect: interner::Symbol,
    /// The name for this attribute
    name: interner::Symbol,
    /// The type id of the concrete type that implements this attribute
    type_id: TypeId,
    /// Details of the traits implemented by this attribute, used to answer questions about what
    /// traits are implemented, as well as reconstruct `&dyn Trait` references given a pointer to
    /// the data of a specific attribute instance.
    traits: Box<[TraitInfo]>,
    /// The custom assembly parser for this attribute
    parse_assembly: Option<ParseAssemblyFn>,
    /// Allocates a new uninitialized instance of this attribute
    alloc_uninit: AllocUninit,
    create_default: fn(&Rc<Context>) -> AttributeRef,
}

unsafe fn alloc_uninit<T: AttributeRegistration>(context: Rc<Context>) -> *mut dyn Attribute {
    let uninit = context.alloc_uninit_tracked::<T>();
    let assumed_init = unsafe { UnsafeIntrusiveEntityRef::assume_init(uninit) };
    UnsafeIntrusiveEntityRef::into_raw(assumed_init.as_attribute_ref()).cast_mut()
}

fn create_default<T: AttributeRegistration>(context: &Rc<Context>) -> AttributeRef {
    <T as AttributeRegistration>::create_default(context).as_attribute_ref()
}

type AllocUninit = unsafe fn(context: Rc<Context>) -> *mut dyn Attribute;

pub type ParseAssemblyFn = fn(&mut dyn parse::Parser<'_>) -> parse::ParseResult<AttributeRef>;

trait MaybeAttrParser {
    fn parse_assembly() -> Option<ParseAssemblyFn>;
}

impl<T> MaybeAttrParser for T {
    #[inline(always)]
    default fn parse_assembly() -> Option<ParseAssemblyFn> {
        None
    }
}

impl<T: AttributeRegistration + AttrParser> MaybeAttrParser for T {
    #[inline(always)]
    fn parse_assembly() -> Option<ParseAssemblyFn> {
        Some(<T as AttrParser>::parse)
    }
}

impl AttributeName {
    pub fn new<T>(dialect: interner::Symbol, mut extra_traits: Vec<TraitInfo>) -> Self
    where
        T: AttributeRegistration,
    {
        let type_id = TypeId::of::<T>();
        let mut traits = Vec::from(<T as AttributeRegistration>::traits());
        traits.append(&mut extra_traits);
        traits.sort_by_key(|ti| *ti.type_id());
        let parse_assembly = <T as MaybeAttrParser>::parse_assembly();
        Self(Rc::new(AttributeInfo {
            dialect,
            name: <T as AttributeRegistration>::name(),
            type_id,
            traits: traits.into_boxed_slice(),
            parse_assembly,
            alloc_uninit: alloc_uninit::<T>,
            create_default: create_default::<T>,
        }))
    }

    /// Returns the dialect name of this attribute
    pub fn dialect(&self) -> interner::Symbol {
        self.0.dialect
    }

    /// Returns the name/opcode of this attribute
    pub fn name(&self) -> interner::Symbol {
        self.0.name
    }

    /// Returns the [TypeId] of the concrete attribute type
    #[inline]
    pub fn id(&self) -> &TypeId {
        &self.0.type_id
    }

    /// Returns the custom assembly parser function for this attribute, if it has one.
    #[inline(always)]
    pub fn parse_assembly_fn(&self) -> Option<ParseAssemblyFn> {
        self.0.parse_assembly
    }

    #[inline(always)]
    pub fn create_default(&self, context: &Rc<Context>) -> AttributeRef {
        (self.0.create_default)(context)
    }

    /// Returns a freshly allocated clone of `attr`, produced by allocating a new uninitialized
    /// instance of the underlying attribute type, and then cloning `attr` into it via the
    /// `CloneToUninit` implementation for the underlying attribute type.
    ///
    /// This function will panic if `attr` is not of the exact same concrete type as the one that
    /// this attribute was derived from.
    pub fn dyn_clone(&self, attr: &dyn Attribute) -> AttributeRef {
        let context = attr.context_rc();
        assert_eq!(attr.name().id(), self.id());
        // SAFETY: This is guaranteed to be safe due to the following:
        //
        // 1. The allocation function is guaranteed to be sufficient size and alignment to hold a
        //    value of the underlying type of this attribute.
        // 2. We've asserted that the input attribute value type is the same as the underlying
        //    type of this attribute.
        unsafe {
            let uninit = (self.0.alloc_uninit)(context);
            attr.clone_to_uninit(uninit.cast());
            AttributeRef::from_raw(uninit)
        }
    }

    /// Returns true if `T` is the concrete type that implements this attribute
    #[inline]
    pub fn is<T: 'static>(&self) -> bool {
        TypeId::of::<T>() == self.0.type_id
    }

    /// Returns true if this attribute implements `Trait`
    pub fn implements<Trait>(&self) -> bool
    where
        Trait: ?Sized + Pointee<Metadata = DynMetadata<Trait>> + 'static,
    {
        let type_id = TypeId::of::<Trait>();
        self.implements_trait_id(&type_id)
    }

    /// Returns true if this attribute implements `trait`, where `trait` is the `TypeId` of a
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

impl fmt::Debug for AttributeName {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

impl fmt::Display for AttributeName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}", &self.dialect(), &self.name())
    }
}

impl Eq for AttributeInfo {}

impl PartialEq for AttributeInfo {
    fn eq(&self, other: &Self) -> bool {
        self.dialect == other.dialect && self.name == other.name && self.type_id == other.type_id
    }
}

impl PartialOrd for AttributeInfo {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for AttributeInfo {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        self.dialect
            .cmp(&other.dialect)
            .then_with(|| self.name.cmp(&other.name))
            .then_with(|| self.type_id.cmp(&other.type_id))
    }
}

impl core::hash::Hash for AttributeInfo {
    fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
        self.dialect.hash(state);
        self.name.hash(state);
        self.type_id.hash(state);
    }
}
