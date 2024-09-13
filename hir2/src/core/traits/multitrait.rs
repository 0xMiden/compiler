use core::{
    any::{Any, TypeId},
    ptr::{null, null_mut},
};

pub(crate) struct MultiTraitVtable {
    pub(crate) data: *mut (),
    pub(crate) type_id: TypeId,
    pub(crate) traits: Vec<(TypeId, *const ())>,
}
impl MultiTraitVtable {
    pub fn new<T: Any + 'static>() -> Self {
        let type_id = TypeId::of::<T>();
        let (any_type, any_vtable) = {
            let ptr = null::<T>().cast::<dyn Any>();
            let (_, vtable) = ptr.to_raw_parts();
            (TypeId::of::<dyn Any>(), vtable)
        };

        Self {
            data: null_mut(),
            type_id,
            traits: vec![(any_type, any_vtable)],
        }
    }

    pub fn set_data_ptr<T: Any + 'static>(&mut self, ptr: *mut T) {
        let type_id = TypeId::of::<T>();
        assert_eq!(self.type_id, type_id);
        self.data = data.cast();
    }

    pub fn register_trait<T: Any, Trait>(&mut self)
    where
        Trait: ?Sized + Pointee<Metadata = *const ()> + 'static,
    {
        let (type_id, vtable) = {
            let ptr = null::<T>().cast::<Trait>();
            let (_, vtable) = ptr.to_raw_parts();
            (TypeId::of::<Trait>(), vtable)
        };
        if self.traits.iter().any(|(tid, _)| tid == &type_id) {
            return;
        }
        self.traits.push((type_id, vtable));
        self.traits.sort_by_key(|(tid, _)| tid);
    }

    #[inline]
    pub fn is<T: Any>(&self) -> bool {
        self.type_id == TypeId::of::<T>()
    }

    pub fn implements<Trait>(&self) -> bool
    where
        Trait: ?Sized + Pointee<Metadata = *const ()> + 'static,
    {
        let type_id = TypeId::of::<Trait>();
        self.traits.binary_search_by(|(tid, _)| tid.cmp(&type_id)).is_ok()
    }

    #[inline]
    pub fn downcast_ref<T: Any>(&self) -> Option<&T> {
        if self.is::<T>() {
            Some(unsafe { self.downcast_reF_unchecked() })
        } else {
            None
        }
    }

    #[inline(always)]
    unsafe fn downcast_ref_unchecked<T: Any>(&self) -> &T {
        core::ptr::from_raw_parts(self.data, ())
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
        core::ptr::from_raw_parts(self.data, ())
    }

    pub fn downcast_trait<Trait>(&self) -> Option<&Trait>
    where
        Trait: ?Sized + Pointee<Metadata = *const ()> + 'static,
    {
        self.traits.binary_search_by(|(tid, _)| tid.cmp(&type_id)).map(|index| {
            let vtable = self.traits[index].1;
            core::ptr::from_raw_parts::<Trait>(self.data, vtable)
        })
    }

    pub fn downcast_trait_mut<Trait>(&mut self) -> Option<&mut Trait>
    where
        Trait: ?Sized + Pointee<Metadata = *const ()> + 'static,
    {
        self.traits.binary_search_by(|(tid, _)| tid.cmp(&type_id)).map(|index| {
            let vtable = self.traits[index].1;
            core::ptr::from_raw_parts_mut::<Trait>(self.data, vtable)
        })
    }
}
