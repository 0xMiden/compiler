use alloc::{boxed::Box, rc::Rc, vec::Vec};
use core::{
    any::TypeId,
    fmt,
    ptr::{DynMetadata, Pointee},
};

use super::OpRegistration;
use crate::{
    Context, interner,
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
    /// The implementation of `Canonicalizable::get_canonicalization_patterns` for this type
    get_canonicalization_patterns: fn(&mut RewritePatternSet, Rc<Context>),
}

impl OperationName {
    pub fn new<T>(dialect: interner::Symbol, mut extra_traits: Vec<TraitInfo>) -> Self
    where
        T: OpRegistration,
    {
        let type_id = TypeId::of::<T>();
        let mut traits = Vec::from(<T as OpRegistration>::traits());
        traits.append(&mut extra_traits);
        traits.sort_by_key(|ti| *ti.type_id());
        let get_canonicalization_patterns = <T as Canonicalizable>::get_canonicalization_patterns;
        let info = Rc::new(OperationInfo::new(
            dialect,
            <T as OpRegistration>::name(),
            type_id,
            traits,
            get_canonicalization_patterns,
        ));
        Self(info)
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

    /// Populates `rewrites` with the set of canonicalization patterns registered for this operation
    pub fn populate_canonicalization_patterns(
        &self,
        rewrites: &mut RewritePatternSet,
        context: Rc<Context>,
    ) {
        (self.0.get_canonicalization_patterns)(rewrites, context)
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

impl OperationInfo {
    pub fn new(
        dialect: interner::Symbol,
        name: interner::Symbol,
        type_id: TypeId,
        traits: Vec<TraitInfo>,
        get_canonicalization_patterns: fn(&mut RewritePatternSet, Rc<Context>),
    ) -> Self {
        Self {
            dialect,
            name,
            type_id,
            traits: traits.into_boxed_slice(),
            get_canonicalization_patterns,
        }
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
