use alloc::vec::Vec;
use core::{
    any::TypeId,
    marker::Unsize,
    ptr::{DynMetadata, Pointee},
};

use super::{Dialect, DialectRegistration};
use crate::{FxHashMap, OpRegistration, OperationName, interner, traits::TraitInfo};

pub struct DialectInfo {
    /// The namespace of this dialect
    name: interner::Symbol,
    /// The concrete type id of the dialect implementation
    type_id: TypeId,
    /// The set of operations registered to this dialect
    registered_ops: Vec<OperationName>,
    /// The set of dialect interfaces (traits) implemented by this dialect
    registered_interfaces: Vec<TraitInfo>,
    /// The set of trait implementations for operations of this dialect which for one reason or
    /// another could not be attached to the operation definition itself. These traits are instead
    /// late-bound at dialect registration time. This field is only used during dialect registration.
    late_bound_traits: FxHashMap<interner::Symbol, Vec<TraitInfo>>,
}

impl DialectInfo {
    pub(crate) fn new<T>() -> Self
    where
        T: DialectRegistration,
    {
        let type_id = TypeId::of::<T>();
        Self {
            name: <T as DialectRegistration>::NAMESPACE.into(),
            type_id,
            registered_ops: Default::default(),
            registered_interfaces: Default::default(),
            late_bound_traits: Default::default(),
        }
    }

    pub const fn name(&self) -> interner::Symbol {
        self.name
    }

    pub const fn dialect_type_id(&self) -> &TypeId {
        &self.type_id
    }

    pub fn operations(&self) -> &[OperationName] {
        &self.registered_ops
    }

    pub fn register_operation<T>(&mut self) -> OperationName
    where
        T: OpRegistration,
    {
        let opcode = <T as OpRegistration>::name();
        match self.registered_ops.binary_search_by_key(&opcode, |op| op.name()) {
            Ok(index) => self.registered_ops[index].clone(),
            Err(index) => {
                let extra_traits = self.late_bound_traits.remove(&opcode).unwrap_or_default();
                let name = OperationName::new::<T>(self.name, extra_traits);
                self.registered_ops.insert(index, name.clone());
                name
            }
        }
    }

    pub fn register_operation_trait<T, Trait>(&mut self)
    where
        T: OpRegistration + Unsize<Trait> + 'static,
        Trait: ?Sized + Pointee<Metadata = DynMetadata<Trait>> + 'static,
    {
        let opcode = <T as OpRegistration>::name();
        let traits = self.late_bound_traits.entry(opcode).or_default();
        let trait_type_id = TypeId::of::<Trait>();
        if let Err(index) = traits.binary_search_by(|ti| ti.type_id().cmp(&trait_type_id)) {
            traits.insert(index, TraitInfo::new::<T, Trait>());
        }
    }

    pub fn register_interface<T, Trait>(&mut self)
    where
        T: Dialect + Unsize<Trait> + 'static,
        Trait: ?Sized + Pointee<Metadata = DynMetadata<Trait>> + 'static,
    {
        let type_id = TypeId::of::<T>();
        assert_eq!(
            type_id, self.type_id,
            "cannot register implementation of Trait for T, for another type"
        );

        let trait_type_id = TypeId::of::<Trait>();
        if let Err(index) = self
            .registered_interfaces
            .binary_search_by(|ti| ti.type_id().cmp(&trait_type_id))
        {
            self.registered_interfaces.insert(index, TraitInfo::new::<T, Trait>());
        }
    }

    /// Returns true if this operation implements `Trait`
    pub fn implements<Trait>(&self) -> bool
    where
        Trait: ?Sized + Pointee<Metadata = DynMetadata<Trait>> + 'static,
    {
        let type_id = TypeId::of::<Trait>();
        self.registered_interfaces
            .binary_search_by(|ti| ti.type_id().cmp(&type_id))
            .is_ok()
    }

    pub(super) fn upcast<'a, Trait>(&self, ptr: *const ()) -> Option<&'a Trait>
    where
        Trait: ?Sized + Pointee<Metadata = DynMetadata<Trait>> + 'static,
    {
        let type_id = TypeId::of::<Trait>();
        let metadata = self
            .registered_interfaces
            .binary_search_by(|ti| ti.type_id().cmp(&type_id))
            .ok()
            .map(|index| unsafe {
                self.registered_interfaces[index].metadata_unchecked::<Trait>()
            })?;
        Some(unsafe { &*core::ptr::from_raw_parts(ptr, metadata) })
    }
}

impl Eq for DialectInfo {}
impl PartialEq for DialectInfo {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
    }
}
impl Ord for DialectInfo {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        self.name.cmp(&other.name)
    }
}
impl PartialOrd for DialectInfo {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl core::hash::Hash for DialectInfo {
    fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
        self.name.hash(state);
    }
}

impl core::fmt::Debug for DialectInfo {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.name.as_str())
    }
}
impl core::fmt::Display for DialectInfo {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.name.as_str())
    }
}
