use alloc::rc::Rc;
use core::{
    any::TypeId,
    fmt,
    ptr::{DynMetadata, Pointee},
};

use crate::{interner, traits::TraitInfo, DialectName, Op};

/// The operation name, or mnemonic, that uniquely identifies an operation.
///
/// The operation name consists of its dialect name, and the opcode name within the dialect.
///
/// No two operation names can share the same fully-qualified operation name.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct OperationName(Rc<OperationInfo>);

struct OperationInfo {
    /// The dialect of this operation
    dialect: DialectName,
    /// The opcode name for this operation
    name: interner::Symbol,
    /// The type id of the concrete type that implements this operation
    type_id: TypeId,
    /// Details of the traits implemented by this operation, used to answer questions about what
    /// traits are implemented, as well as reconstruct `&dyn Trait` references given a pointer to
    /// the data of a specific operation instance.
    traits: Box<[TraitInfo]>,
}

impl OperationName {
    pub fn new<O, S, T>(dialect: DialectName, name: S, traits: T) -> Self
    where
        O: crate::Op,
        S: Into<interner::Symbol>,
        T: IntoIterator<Item = TraitInfo>,
    {
        let type_id = TypeId::of::<O>();
        let mut traits = traits.into_iter().collect::<Vec<_>>();
        traits.sort_by_key(|ti| *ti.type_id());
        let traits = traits.into_boxed_slice();
        let info = Rc::new(OperationInfo::new(dialect, name.into(), type_id, traits));
        Self(info)
    }

    /// Returns the dialect name of this operation
    pub fn dialect(&self) -> DialectName {
        self.0.dialect
    }

    /// Returns the namespace to which this operation name belongs (i.e. dialect name)
    pub fn namespace(&self) -> interner::Symbol {
        self.0.dialect.as_symbol()
    }

    /// Returns the name/opcode of this operation
    pub fn name(&self) -> interner::Symbol {
        self.0.name
    }

    /// Returns true if `T` is the concrete type that implements this operation
    pub fn is<T: Op>(&self) -> bool {
        TypeId::of::<T>() == self.0.type_id
    }

    /// Returns true if this operation implements `Trait`
    pub fn implements<Trait>(&self) -> bool
    where
        Trait: ?Sized + Pointee<Metadata = DynMetadata<Trait>> + 'static,
    {
        let type_id = TypeId::of::<Trait>();
        self.0.traits.binary_search_by(|ti| ti.type_id().cmp(&type_id)).is_ok()
    }

    /// Returns true if this operation implements `trait`, where `trait` is the `TypeId` of a
    /// `dyn Trait` type.
    pub fn implements_trait_id(&self, trait_id: &TypeId) -> bool {
        self.0.traits.binary_search_by(|ti| ti.type_id().cmp(trait_id)).is_ok()
    }

    #[inline]
    pub(super) fn downcast_ref<T: Op>(&self, ptr: *const ()) -> Option<&T> {
        if self.is::<T>() {
            Some(unsafe { self.downcast_ref_unchecked(ptr) })
        } else {
            None
        }
    }

    #[inline(always)]
    unsafe fn downcast_ref_unchecked<T: Op>(&self, ptr: *const ()) -> &T {
        &*core::ptr::from_raw_parts(ptr.cast::<T>(), ())
    }

    #[inline]
    pub(super) fn downcast_mut<T: Op>(&mut self, ptr: *mut ()) -> Option<&mut T> {
        if self.is::<T>() {
            Some(unsafe { self.downcast_mut_unchecked(ptr) })
        } else {
            None
        }
    }

    #[inline(always)]
    unsafe fn downcast_mut_unchecked<T: Op>(&mut self, ptr: *mut ()) -> &mut T {
        &mut *core::ptr::from_raw_parts_mut(ptr.cast::<T>(), ())
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

    fn get<Trait: ?Sized + 'static>(&self) -> Option<&TraitInfo> {
        let type_id = TypeId::of::<Trait>();
        self.0
            .traits
            .binary_search_by(|ti| ti.type_id().cmp(&type_id))
            .ok()
            .map(|index| &self.0.traits[index])
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
        write!(f, "{}.{}", &self.namespace(), &self.name())
    }
}

impl OperationInfo {
    pub fn new(
        dialect: DialectName,
        name: interner::Symbol,
        type_id: TypeId,
        traits: Box<[TraitInfo]>,
    ) -> Self {
        Self {
            dialect,
            name,
            type_id,
            traits,
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
