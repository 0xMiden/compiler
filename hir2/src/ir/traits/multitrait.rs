use core::{
    any::{Any, TypeId},
    marker::Unsize,
    ptr::{null, null_mut, DynMetadata, Pointee},
};

struct TraitImpl {
    /// The [TypeId] of the trait type, used as a unique key for [TraitImpl]s
    type_id: TypeId,
    /// Type-erased dyn metadata containing the trait vtable pointer for the concrete type
    ///
    /// This is transmuted to the correct trait type when reifying a `&dyn Trait` reference,
    /// which is safe as `DynMetadata` is always the same size for all types.
    metadata: DynMetadata<dyn Any>,
}
impl TraitImpl {
    fn new<T, Trait>() -> Self
    where
        T: Any + Unsize<Trait>,
        Trait: ?Sized + Pointee<Metadata = DynMetadata<Trait>> + 'static,
    {
        let type_id = TypeId::of::<Trait>();
        let ptr = null::<T>() as *const Trait;
        let (_, metadata) = ptr.to_raw_parts();
        Self {
            type_id,
            metadata: unsafe {
                core::mem::transmute::<DynMetadata<Trait>, DynMetadata<dyn Any>>(metadata)
            },
        }
    }

    unsafe fn metadata_unchecked<Trait>(&self) -> DynMetadata<Trait>
    where
        Trait: ?Sized + Pointee<Metadata = DynMetadata<Trait>> + 'static,
    {
        debug_assert!(self.type_id == TypeId::of::<Trait>());
        core::mem::transmute(self.metadata)
    }
}
impl Eq for TraitImpl {}
impl PartialEq for TraitImpl {
    fn eq(&self, other: &Self) -> bool {
        self.type_id == other.type_id
    }
}
impl PartialEq<TypeId> for TraitImpl {
    fn eq(&self, other: &TypeId) -> bool {
        self.type_id.eq(other)
    }
}
impl PartialOrd for TraitImpl {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.type_id.cmp(&other.type_id))
    }
}
impl PartialOrd<TypeId> for TraitImpl {
    fn partial_cmp(&self, other: &TypeId) -> Option<core::cmp::Ordering> {
        Some(self.type_id.cmp(other))
    }
}
impl Ord for TraitImpl {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        self.type_id.cmp(&other.type_id)
    }
}

pub(crate) struct MultiTraitVtable {
    data: *mut (),
    type_id: TypeId,
    traits: Vec<TraitImpl>,
}
impl MultiTraitVtable {
    pub fn new<T: Any + 'static>() -> Self {
        let type_id = TypeId::of::<T>();
        let any_impl = TraitImpl::new::<T, dyn Any>();

        Self {
            data: null_mut(),
            type_id,
            traits: vec![any_impl],
        }
    }

    #[allow(unused)]
    #[inline]
    pub const fn data_ptr(&self) -> *mut () {
        self.data
    }

    pub(crate) unsafe fn set_data_ptr<T: Any + 'static>(&mut self, ptr: *mut T) {
        assert!(!ptr.is_null());
        assert!(ptr.is_aligned());
        assert!(self.is::<T>());
        self.data = ptr.cast();
    }

    pub fn register_trait<T, Trait>(&mut self)
    where
        T: Any + Unsize<Trait> + 'static,
        Trait: ?Sized + Pointee<Metadata = DynMetadata<Trait>> + 'static,
    {
        let trait_impl = TraitImpl::new::<T, Trait>();
        match self.traits.binary_search(&trait_impl) {
            Ok(_) => (),
            Err(index) if index + 1 == self.traits.len() => self.traits.push(trait_impl),
            Err(index) => self.traits.insert(index, trait_impl),
        }
    }

    #[inline]
    pub fn is<T: Any>(&self) -> bool {
        self.type_id == TypeId::of::<T>()
    }

    pub fn implements<Trait>(&self) -> bool
    where
        Trait: ?Sized + Pointee<Metadata = DynMetadata<Trait>> + 'static,
    {
        let type_id = TypeId::of::<Trait>();
        self.traits.binary_search_by(|ti| ti.type_id.cmp(&type_id)).is_ok()
    }

    #[inline]
    pub fn downcast_ref<T: Any>(&self) -> Option<&T> {
        if self.is::<T>() {
            Some(unsafe { self.downcast_ref_unchecked() })
        } else {
            None
        }
    }

    #[inline(always)]
    unsafe fn downcast_ref_unchecked<T: Any>(&self) -> &T {
        &*core::ptr::from_raw_parts(self.data.cast::<T>(), ())
    }

    #[inline]
    pub fn downcast_mut<T: Any>(&mut self) -> Option<&mut T> {
        if self.is::<T>() {
            Some(unsafe { self.downcast_mut_unchecked() })
        } else {
            None
        }
    }

    #[inline(always)]
    unsafe fn downcast_mut_unchecked<T: Any>(&mut self) -> &mut T {
        &mut *core::ptr::from_raw_parts_mut(self.data.cast::<T>(), ())
    }

    pub fn downcast_trait<Trait>(&self) -> Option<&Trait>
    where
        Trait: ?Sized + Pointee<Metadata = DynMetadata<Trait>> + 'static,
    {
        let metadata = self
            .get::<Trait>()
            .map(|trait_impl| unsafe { trait_impl.metadata_unchecked::<Trait>() })?;
        Some(unsafe { &*core::ptr::from_raw_parts(self.data, metadata) })
    }

    pub fn downcast_trait_mut<Trait>(&mut self) -> Option<&mut Trait>
    where
        Trait: ?Sized + Pointee<Metadata = DynMetadata<Trait>> + 'static,
    {
        let metadata = self
            .get::<Trait>()
            .map(|trait_impl| unsafe { trait_impl.metadata_unchecked::<Trait>() })?;
        Some(unsafe { &mut *core::ptr::from_raw_parts_mut(self.data, metadata) })
    }

    fn get<Trait: ?Sized + 'static>(&self) -> Option<&TraitImpl> {
        let type_id = TypeId::of::<Trait>();
        self.traits
            .binary_search_by(|ti| ti.type_id.cmp(&type_id))
            .ok()
            .map(|index| &self.traits[index])
    }
}
