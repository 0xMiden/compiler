use alloc::rc::Rc;
use core::{cell::RefCell, ptr::NonNull};

use super::*;
use crate::{
    dataflow::{solver::AnalysisQueue, ChangeResult, DenseLattice, SparseLattice},
    ir::entity::BorrowRefMut,
};

pub struct AnalysisStateGuard<'a, T: AnalysisState + 'static> {
    worklist: Rc<RefCell<AnalysisQueue>>,
    info: NonNull<AnalysisStateInfo>,
    state: NonNull<T>,
    _borrow: BorrowRefMut<'a>,
    changed: bool,
}
impl<'a, T: AnalysisState + 'static> AnalysisStateGuard<'a, T> {
    pub(in crate::dataflow) unsafe fn new(
        info: NonNull<AnalysisStateInfo>,
        worklist: Rc<RefCell<AnalysisQueue>>,
    ) -> Self {
        let handle = RawAnalysisStateInfoHandle::new(info);
        let (state, _borrow) = handle.state_mut();
        Self {
            worklist,
            info,
            state,
            _borrow,
            changed: false,
        }
    }

    /// Consume the guard and convert the underlying mutable borrow of the state into an immutable
    /// borrow, after propagating any changes made to the state while mutating it. When this
    /// function returns, the state can be safely aliased by immutable references, while the caller
    /// retains the ability to interact with the state via the returned [crate::EntityRef].
    pub fn into_entity_ref(mut guard: Self) -> crate::EntityRef<'a, T> {
        guard.notify_if_changed();

        let guard = core::mem::ManuallyDrop::new(guard);
        let state = guard.state;
        let borrow_ref_mut = unsafe { core::ptr::read(&guard._borrow) };
        crate::EntityRef::from_raw_parts(state, borrow_ref_mut.into_borrow_ref())
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
    pub fn change<F>(&mut self, mut callback: F) -> ChangeResult
    where
        F: FnMut(&mut T) -> ChangeResult,
    {
        let result = callback(unsafe { self.state.as_mut() });
        self.changed |= matches!(result, ChangeResult::Changed);
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
    pub fn subscribe<A>(guard: &mut Self, analysis: &A)
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
    pub fn subscribe_nonnull(guard: &mut Self, analysis: NonNull<dyn DataFlowAnalysis>) {
        let info = unsafe { guard.info.as_mut() };
        let state = unsafe { guard.state.as_ref() };
        <T as AnalysisStateSubscriptionBehavior>::on_subscribe(state, analysis, info);
    }

    fn notify_if_changed(&mut self) {
        if self.changed {
            // propagting update to {state.debug_name()} of {state.anchor} value {state}
            let mut info = self.info;
            let info = unsafe { info.as_mut() };
            info.increment_revision();
            let mut worklist = self.worklist.borrow_mut();
            let anchor = info.anchor();
            let state = unsafe { self.state.as_ref() };
            // Handle the change for each subscriber to this analysis state
            for subscription in info.subscriptions() {
                subscription.handle_state_change(anchor, &mut *worklist);
            }
            // Invoke any custom on-update logic for this analysis state type
            <T as AnalysisStateSubscriptionBehavior>::on_update(state, info, &mut worklist);
        }
    }
}
impl<'a, T: AnalysisState> AsRef<T> for AnalysisStateGuard<'a, T> {
    #[inline(always)]
    fn as_ref(&self) -> &T {
        unsafe { self.state.as_ref() }
    }
}
impl<'a, T: AnalysisState> AsMut<T> for AnalysisStateGuard<'a, T> {
    #[inline]
    fn as_mut(&mut self) -> &mut T {
        // This is overly conservative, but we assume that a mutable borrow of the underlying state
        // changes that state, and thus must be notified. The problem is that just because you take
        // a mutable reference and seemingly change the state, doesn't mean that the state actually
        // changed. As a result, users of an AnalysisStateGuard are encouraged to either only take
        // a mutable reference after checking that the state actually changed, or use the
        // AnalysisStateGuard::change method.
        self.changed = true;

        unsafe { self.state.as_mut() }
    }
}
impl<'a, T: AnalysisState> core::ops::Deref for AnalysisStateGuard<'a, T> {
    type Target = T;

    #[inline(always)]
    fn deref(&self) -> &Self::Target {
        self.as_ref()
    }
}
impl<'a, T: AnalysisState> core::ops::DerefMut for AnalysisStateGuard<'a, T> {
    #[inline(always)]
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.as_mut()
    }
}
impl<T: AnalysisState + 'static> Drop for AnalysisStateGuard<'_, T> {
    fn drop(&mut self) {
        self.notify_if_changed();
    }
}
impl<'a, T: AnalysisState> AnalysisState for AnalysisStateGuard<'a, T> {
    fn as_any(&self) -> &dyn Any {
        self.as_ref().as_any()
    }

    fn anchor(&self) -> &dyn LatticeAnchor {
        self.as_ref().anchor()
    }
}
impl<'a, T: DenseLattice> DenseLattice for AnalysisStateGuard<'a, T> {
    type Lattice = <T as DenseLattice>::Lattice;

    #[inline]
    fn lattice(&self) -> &Self::Lattice {
        unsafe { self.state.as_ref() }.lattice()
    }

    fn join(&mut self, rhs: &Self::Lattice) -> ChangeResult {
        let result = unsafe { self.state.as_mut().join(rhs) };
        self.changed |= matches!(result, ChangeResult::Changed);
        result
    }

    fn meet(&mut self, rhs: &Self::Lattice) -> ChangeResult {
        let result = unsafe { self.state.as_mut().meet(rhs) };
        self.changed |= matches!(result, ChangeResult::Changed);
        result
    }
}
impl<'a, T: SparseLattice> SparseLattice for AnalysisStateGuard<'a, T> {
    type Lattice = <T as SparseLattice>::Lattice;

    #[inline]
    fn lattice(&self) -> &Self::Lattice {
        unsafe { self.state.as_ref() }.lattice()
    }

    fn join(&mut self, rhs: &Self::Lattice) -> ChangeResult {
        let result = unsafe { self.state.as_mut().join(rhs) };
        self.changed |= matches!(result, ChangeResult::Changed);
        result
    }

    fn meet(&mut self, rhs: &Self::Lattice) -> ChangeResult {
        let result = unsafe { self.state.as_mut().join(rhs) };
        self.changed |= matches!(result, ChangeResult::Changed);
        result
    }
}
