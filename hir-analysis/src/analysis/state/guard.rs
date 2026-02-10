use alloc::rc::Rc;
use core::{cell::RefCell, ptr::NonNull};

use midenc_hir::{
    EntityRef,
    entity::{BorrowRef, BorrowRefMut},
};

use super::*;
use crate::{ChangeResult, DenseLattice, SparseLattice, solver::AnalysisQueue};

/// An immmutable handle/guard for some analysis state T
pub struct AnalysisStateGuard<'a, T: AnalysisState + 'static> {
    #[allow(unused)]
    info: NonNull<AnalysisStateInfo>,
    state: NonNull<T>,
    _borrow: BorrowRef<'a>,
}
impl<'a, T: AnalysisState + 'static> AnalysisStateGuard<'a, T> {
    pub(crate) unsafe fn new(info: NonNull<AnalysisStateInfo>) -> Self {
        unsafe {
            let handle = RawAnalysisStateInfoHandle::new(info);
            let (state, _borrow) = handle.state_ref();
            Self {
                info,
                state,
                _borrow,
            }
        }
    }

    pub fn into_entity_ref(guard: Self) -> EntityRef<'a, T> {
        let guard = core::mem::ManuallyDrop::new(guard);
        let state = guard.state;
        let borrow_ref = unsafe { core::ptr::read(&guard._borrow) };
        EntityRef::from_raw_parts(state, borrow_ref)
    }

    /// Subscribe `analysis` to any changes of the lattice anchor.
    ///
    /// This is handled by invoking the [AnalysisStateSubscriptionBehavior::on_subscribe] callback,
    /// leaving the handling for ad-hoc subscriptions of this kind to each [AnalysisState]
    /// implementation.
    ///
    /// # Safety
    ///
    /// The caller must ensure that `analysis` is owned by the [DataFlowSolver], as that is the
    /// only situation in which it is safe for us to take the address of the analysis for later use.
    pub fn subscribe<A>(guard: &Self, analysis: &A)
    where
        A: DataFlowAnalysis + 'static,
    {
        let analysis = analysis as *const dyn DataFlowAnalysis;
        let analysis = unsafe { NonNull::new_unchecked(analysis.cast_mut()) };
        Self::subscribe_nonnull(guard, analysis);
    }

    /// Subscribe `analysis` to any changes of the lattice anchor.
    ///
    /// This is handled by invoking the [AnalysisStateSubscriptionBehavior::on_subscribe] callback,
    /// leaving the handling for ad-hoc subscriptions of this kind to each [AnalysisState]
    /// implementation.
    ///
    /// # Safety
    ///
    /// The caller must ensure that `analysis` is owned by the [DataFlowSolver], as that is the
    /// only situation in which it is safe for us to take the address of the analysis for later use.
    fn subscribe_nonnull(guard: &Self, analysis: NonNull<dyn DataFlowAnalysis>) {
        let info = unsafe { guard.info.as_ref() };
        let state = unsafe { guard.state.as_ref() };
        <T as AnalysisStateSubscriptionBehavior>::on_subscribe(state, analysis, info);
    }
}
impl<T: AnalysisState> AsRef<T> for AnalysisStateGuard<'_, T> {
    #[inline(always)]
    fn as_ref(&self) -> &T {
        unsafe { self.state.as_ref() }
    }
}
impl<T: AnalysisState> core::ops::Deref for AnalysisStateGuard<'_, T> {
    type Target = T;

    #[inline(always)]
    fn deref(&self) -> &Self::Target {
        self.as_ref()
    }
}
impl<T: AnalysisState + core::fmt::Debug> core::fmt::Debug for AnalysisStateGuard<'_, T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        core::fmt::Debug::fmt(self.as_ref(), f)
    }
}
impl<T: AnalysisState + core::fmt::Display> core::fmt::Display for AnalysisStateGuard<'_, T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        core::fmt::Display::fmt(self.as_ref(), f)
    }
}
impl<T: AnalysisState> AnalysisState for AnalysisStateGuard<'_, T> {
    fn as_any(&self) -> &dyn Any {
        self.as_ref().as_any()
    }

    fn anchor(&self) -> &dyn LatticeAnchor {
        self.as_ref().anchor()
    }
}

/// A mutable handle/guard for some analysis state T
pub struct AnalysisStateGuardMut<'a, T: AnalysisState + 'static> {
    worklist: Rc<RefCell<AnalysisQueue>>,
    info: NonNull<AnalysisStateInfo>,
    state: NonNull<T>,
    _borrow: BorrowRefMut<'a>,
    changed: ChangeResult,
}
impl<'a, T: AnalysisState + 'static> AnalysisStateGuardMut<'a, T> {
    pub(crate) unsafe fn new(
        info: NonNull<AnalysisStateInfo>,
        worklist: Rc<RefCell<AnalysisQueue>>,
    ) -> Self {
        unsafe {
            let handle = RawAnalysisStateInfoHandle::new(info);
            let (state, _borrow) = handle.state_mut();
            Self {
                worklist,
                info,
                state,
                _borrow,
                changed: ChangeResult::Unchanged,
            }
        }
    }

    pub fn freeze(mut guard: Self) -> AnalysisStateGuard<'a, T> {
        guard.notify_if_changed();

        let guard = core::mem::ManuallyDrop::new(guard);
        let info = guard.info;
        let state = guard.state;
        let _worklist = unsafe { core::ptr::read(&guard.worklist) };
        let borrow_ref_mut = unsafe { core::ptr::read(&guard._borrow) };
        AnalysisStateGuard {
            info,
            state,
            _borrow: borrow_ref_mut.into_borrow_ref(),
        }
    }

    /// Consume the guard and convert the underlying mutable borrow of the state into an immutable
    /// borrow, after propagating any changes made to the state while mutating it. When this
    /// function returns, the state can be safely aliased by immutable references, while the caller
    /// retains the ability to interact with the state via the returned [crate::EntityRef].
    pub fn into_entity_ref(mut guard: Self) -> EntityRef<'a, T> {
        guard.notify_if_changed();

        let guard = core::mem::ManuallyDrop::new(guard);
        let state = guard.state;
        let borrow_ref_mut = unsafe { core::ptr::read(&guard._borrow) };
        EntityRef::from_raw_parts(state, borrow_ref_mut.into_borrow_ref())
    }

    /// Apply a function to the underlying [AnalysisState] that may or may not change it.
    ///
    /// The callback is expected to return a [ChangeResult] reflecting whether or not changes were
    /// applied. If the callback fails to signal that a change was made, dependent analyses will
    /// not be re-enqueued, and thus analyses may make incorrect assumptions.
    ///
    /// This should be used when you need a mutable reference to the state, but may not actually
    /// end up mutating the state with it. The default [DerefMut] implementation will always
    /// assume the underlying state was changed if invoked - this function lets you bypass that,
    /// but with the requirement that you signal changes manually.
    pub fn change<F>(&mut self, callback: F) -> ChangeResult
    where
        F: FnOnce(&mut T) -> ChangeResult,
    {
        log::trace!(target: "analysis:state", "starting analysis state change of type {}", core::any::type_name::<T>());
        let result = callback(unsafe { self.state.as_mut() });
        log::trace!(target: "analysis:state", "analysis state changed: {}", result.changed());
        self.changed |= result;
        result
    }

    /// Subscribe `analysis` to any changes of the lattice anchor.
    ///
    /// This is handled by invoking the [AnalysisStateSubscriptionBehavior::on_subscribe] callback,
    /// leaving the handling for ad-hoc subscriptions of this kind to each [AnalysisState]
    /// implementation.
    ///
    /// # Safety
    ///
    /// The caller must ensure that `analysis` is owned by the [DataFlowSolver], as that is the
    /// only situation in which it is safe for us to take the address of the analysis for later use.
    pub fn subscribe<A>(guard: &Self, analysis: &A)
    where
        A: DataFlowAnalysis + 'static,
    {
        let analysis = analysis as *const dyn DataFlowAnalysis;
        let analysis = unsafe { NonNull::new_unchecked(analysis.cast_mut()) };
        Self::subscribe_nonnull(guard, analysis);
    }

    /// Require this analysis state at `dependent`
    ///
    /// This is meant to be used in cases where calling `DataFlowSolver::require` is not possible
    /// because you already hold an `AnalysisStateGuardMut` for the state, but you still need to
    /// ensure that we execute the `on_require_analysis` hook for `dependent`.
    pub fn require<A>(&mut self, analysis: &A, dependent: ProgramPoint)
    where
        A: DataFlowAnalysis + 'static,
    {
        let analysis = analysis as *const dyn DataFlowAnalysis;
        let analysis = unsafe { NonNull::new_unchecked(analysis.cast_mut()) };
        let info = unsafe { self.info.as_mut() };
        let state = unsafe { self.state.as_ref() };
        <T as AnalysisStateSubscriptionBehavior>::on_require_analysis(
            state, info, analysis, dependent,
        );
    }

    /// Subscribe `analysis` to any changes of the lattice anchor.
    ///
    /// This is handled by invoking the [AnalysisStateSubscriptionBehavior::on_subscribe] callback,
    /// leaving the handling for ad-hoc subscriptions of this kind to each [AnalysisState]
    /// implementation.
    ///
    /// # Safety
    ///
    /// The caller must ensure that `analysis` is owned by the [DataFlowSolver], as that is the
    /// only situation in which it is safe for us to take the address of the analysis for later use.
    fn subscribe_nonnull(guard: &Self, analysis: NonNull<dyn DataFlowAnalysis>) {
        let info = unsafe { guard.info.as_ref() };
        let state = unsafe { guard.state.as_ref() };
        <T as AnalysisStateSubscriptionBehavior>::on_subscribe(state, analysis, info);
    }

    fn notify_if_changed(&mut self) {
        if self.changed.changed() {
            // propagting update to {state.debug_name()} of {state.anchor} value {state}
            let mut info = self.info;
            let info = unsafe { info.as_mut() };
            info.increment_revision();
            let mut worklist = self.worklist.borrow_mut();
            let anchor = info.anchor();
            log::trace!(
                target: "analysis:state",
                "committing changes to analysis state {} at {anchor}",
                core::any::type_name::<T>()
            );
            let state = unsafe { self.state.as_ref() };
            // Handle the change for each subscriber to this analysis state
            log::trace!(
                target: "analysis:state",
                "there are {} subscriptions to notify of this change",
                info.subscriptions().len()
            );
            {
                let subscriptions = info.subscriptions();
                for subscription in subscriptions.iter() {
                    subscription.handle_state_change::<T, _>(anchor, &mut *worklist);
                }
            }
            // Invoke any custom on-update logic for this analysis state type
            log::trace!(target: "analysis:state", "invoking on_update callback to notify user-defined subscriptions");
            <T as AnalysisStateSubscriptionBehavior>::on_update(state, info, &mut worklist);
        }
    }
}
impl<T: AnalysisState> AsRef<T> for AnalysisStateGuardMut<'_, T> {
    #[inline(always)]
    fn as_ref(&self) -> &T {
        unsafe { self.state.as_ref() }
    }
}
impl<T: AnalysisState> AsMut<T> for AnalysisStateGuardMut<'_, T> {
    #[inline]
    fn as_mut(&mut self) -> &mut T {
        // This is overly conservative, but we assume that a mutable borrow of the underlying state
        // changes that state, and thus must be notified. The problem is that just because you take
        // a mutable reference and seemingly change the state, doesn't mean that the state actually
        // changed. As a result, users of an AnalysisStateGuard are encouraged to either only take
        // a mutable reference after checking that the state actually changed, or use the
        // AnalysisStateGuard::change method.
        self.changed = ChangeResult::Changed;

        unsafe { self.state.as_mut() }
    }
}
impl<T: AnalysisState> core::ops::Deref for AnalysisStateGuardMut<'_, T> {
    type Target = T;

    #[inline(always)]
    fn deref(&self) -> &Self::Target {
        self.as_ref()
    }
}
impl<T: AnalysisState> core::ops::DerefMut for AnalysisStateGuardMut<'_, T> {
    #[inline(always)]
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.as_mut()
    }
}
impl<T: AnalysisState + 'static> Drop for AnalysisStateGuardMut<'_, T> {
    fn drop(&mut self) {
        self.notify_if_changed();
    }
}
impl<T: AnalysisState + core::fmt::Debug> core::fmt::Debug for AnalysisStateGuardMut<'_, T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        core::fmt::Debug::fmt(self.as_ref(), f)
    }
}
impl<T: AnalysisState + core::fmt::Display> core::fmt::Display for AnalysisStateGuardMut<'_, T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        core::fmt::Display::fmt(self.as_ref(), f)
    }
}
impl<T: AnalysisState> AnalysisState for AnalysisStateGuardMut<'_, T> {
    fn as_any(&self) -> &dyn Any {
        self.as_ref().as_any()
    }

    fn anchor(&self) -> &dyn LatticeAnchor {
        self.as_ref().anchor()
    }
}
impl<T: DenseLattice> DenseLattice for AnalysisStateGuardMut<'_, T> {
    type Lattice = <T as DenseLattice>::Lattice;

    #[inline]
    fn lattice(&self) -> &Self::Lattice {
        unsafe { self.state.as_ref() }.lattice()
    }

    #[inline]
    fn lattice_mut(&mut self) -> &mut Self::Lattice {
        unsafe { self.state.as_mut() }.lattice_mut()
    }

    fn join(&mut self, rhs: &Self::Lattice) -> ChangeResult {
        let result = unsafe { self.state.as_mut().join(rhs) };
        self.changed |= result;
        result
    }

    fn meet(&mut self, rhs: &Self::Lattice) -> ChangeResult {
        let result = unsafe { self.state.as_mut().meet(rhs) };
        self.changed |= result;
        result
    }
}
impl<T: SparseLattice> SparseLattice for AnalysisStateGuardMut<'_, T> {
    type Lattice = <T as SparseLattice>::Lattice;

    #[inline]
    fn lattice(&self) -> &Self::Lattice {
        unsafe { self.state.as_ref() }.lattice()
    }

    fn join(&mut self, rhs: &Self::Lattice) -> ChangeResult {
        let result = unsafe { self.state.as_mut().join(rhs) };
        self.changed |= result;
        result
    }

    fn meet(&mut self, rhs: &Self::Lattice) -> ChangeResult {
        let result = unsafe { self.state.as_mut().join(rhs) };
        self.changed |= result;
        result
    }
}
