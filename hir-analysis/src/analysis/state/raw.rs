use core::{any::TypeId, ptr::NonNull};

use midenc_hir::{
    FxHashMap,
    entity::{BorrowRef, BorrowRefMut, EntityRef, RawEntity},
};

use super::*;
use crate::LatticeAnchorRef;

pub struct AnalysisStateDescriptor {
    type_name: &'static str,
    /// The unique id of the concrete analysis state type
    type_id: TypeId,
    /// The vtable pointer for the analysis state
    metadata: core::ptr::DynMetadata<dyn AnalysisState>,
    /// Offset from the start of [AnalysisStateInfo] to the start of the RawEntity<T> for this
    /// analsyis type
    offset: u32,
}
impl AnalysisStateDescriptor {
    pub fn new<T: BuildableAnalysisState>(alloc: &blink_alloc::Blink) -> NonNull<Self> {
        let dyn_ptr = NonNull::<T>::dangling() as NonNull<dyn AnalysisState>;
        let offset = (core::mem::offset_of!(RawAnalysisStateInfo<T>, state)
            - core::mem::offset_of!(RawAnalysisStateInfo<T>, info)) as u32;
        let desc = alloc.put(Self {
            type_name: core::any::type_name::<T>(),
            type_id: TypeId::of::<T>(),
            metadata: dyn_ptr.to_raw_parts().1,
            offset,
        });
        unsafe { NonNull::new_unchecked(desc) }
    }

    #[inline(always)]
    pub const fn debug_name(&self) -> &'static str {
        self.type_name
    }

    #[inline(always)]
    pub const fn type_id(&self) -> &TypeId {
        &self.type_id
    }

    #[inline(always)]
    pub const fn offset(&self) -> usize {
        self.offset as usize
    }

    #[inline(always)]
    pub const fn metadata(&self) -> core::ptr::DynMetadata<dyn AnalysisState> {
        self.metadata
    }
}

#[repr(C)]
pub struct RawAnalysisStateInfo<T> {
    info: AnalysisStateInfo,
    state: RawEntity<T>,
}
impl<T: BuildableAnalysisState> RawAnalysisStateInfo<T> {
    /// Allocate a new instance of the analysis state `T` attached to `anchor`, using `alloc`.
    ///
    /// Returns the [AnalysisStateKey] which uniquely identifies this state.
    pub fn alloc(
        alloc: &blink_alloc::Blink,
        descriptors: &mut FxHashMap<TypeId, NonNull<AnalysisStateDescriptor>>,
        key: AnalysisStateKey,
        anchor: LatticeAnchorRef,
    ) -> NonNull<Self> {
        debug_assert_eq!(key, AnalysisStateKey::new::<T>(anchor));

        let type_id = TypeId::of::<T>();
        let descriptor = *descriptors
            .entry(type_id)
            .or_insert_with(|| AnalysisStateDescriptor::new::<T>(alloc));

        let info = alloc.put(RawAnalysisStateInfo {
            info: AnalysisStateInfo::new(descriptor, anchor),
            state: RawEntity::new(<T as BuildableAnalysisState>::create(anchor)),
        });
        unsafe { NonNull::new_unchecked(info) }
    }

    #[inline]
    pub fn as_info_ptr(raw_info: NonNull<Self>) -> NonNull<AnalysisStateInfo> {
        unsafe {
            let raw = raw_info.as_ptr();
            let info = core::ptr::addr_of_mut!((*raw).info);
            NonNull::new_unchecked(info)
        }
    }
}

pub struct RawAnalysisStateInfoHandle<T: ?Sized> {
    state: NonNull<RawEntity<T>>,
    offset: u32,
}
impl<T: 'static> RawAnalysisStateInfoHandle<T> {
    pub unsafe fn new(info: NonNull<AnalysisStateInfo>) -> Self {
        unsafe {
            let offset = info.as_ref().descriptor().offset;
            let state = info.byte_add(offset as usize).cast::<RawEntity<T>>();
            Self { state, offset }
        }
    }

    #[track_caller]
    #[inline]
    #[allow(unused)]
    pub fn into_entity_ref<'a>(self) -> EntityRef<'a, T> {
        unsafe { self.state.as_ref().borrow() }
    }

    #[track_caller]
    #[inline]
    pub(super) unsafe fn state_mut<'a>(self) -> (NonNull<T>, BorrowRefMut<'a>) {
        unsafe { self.state.as_ref().borrow_mut_unsafe() }
    }

    #[track_caller]
    #[inline]
    pub(super) unsafe fn state_ref<'a>(self) -> (NonNull<T>, BorrowRef<'a>) {
        unsafe { self.state.as_ref().borrow_unsafe() }
    }

    pub fn with<F>(&mut self, callback: F)
    where
        F: FnOnce(&mut T, &mut AnalysisStateInfo),
    {
        let mut info =
            unsafe { self.state.byte_sub(self.offset as usize).cast::<AnalysisStateInfo>() };
        let mut state = self.state;
        let state = unsafe { state.as_mut() };
        let info = unsafe { info.as_mut() };
        callback(&mut state.borrow_mut(), info);
    }
}

impl<T, U> core::ops::CoerceUnsized<RawAnalysisStateInfoHandle<U>> for RawAnalysisStateInfoHandle<T>
where
    T: ?Sized + core::marker::Unsize<U>,
    U: ?Sized,
{
}
