use alloc::collections::VecDeque;
use core::{
    any::TypeId,
    cell::{Ref, RefCell},
    ptr::NonNull,
};

use smallvec::SmallVec;

use super::*;
use crate::{
    adt::SmallSet,
    dataflow::{solver::QueuedAnalysis, AnalysisQueue, LatticeAnchor, LatticeAnchorRef},
    EntityRef,
};

pub type Revision = u32;

pub struct AnalysisStateInfo {
    /// The immortal characteristics of the underlying analysis state:
    ///
    /// * The type id of the concrete `AnalysisState` implementation
    /// * `dyn AnalysisState` vtable
    /// * Offset from start of `AnalysisStateInfo` struct to the actual state data when allocated
    ///   in a `RawAnalysisStateInfo` struct.
    ///
    /// This is shared across all instances of the same analysis state type
    descriptor: NonNull<AnalysisStateDescriptor>,
    /// The anchor to which the analysis state is attached
    anchor: LatticeAnchorRef,
    /// A monotonically-increasing integer representing the number of revisions to this state
    revision: Revision,
    /// The set of dependent program points and associated analyses which are to be re-analyzed
    /// whenever the state changes.
    ///
    /// This is dynamically borrow-checked so that subscriptions can be recorded without requiring
    /// a mutable borrow of the analysis state itself.
    pub(super) subscriptions: RefCell<SmallSet<AnalysisStateSubscription, 4>>,
}
impl AnalysisStateInfo {
    #[inline]
    pub(super) fn new(
        descriptor: NonNull<AnalysisStateDescriptor>,
        anchor: LatticeAnchorRef,
    ) -> Self {
        Self {
            descriptor,
            anchor,
            revision: 0,
            subscriptions: Default::default(),
        }
    }

    #[inline]
    pub fn anchor(&self) -> &dyn LatticeAnchor {
        self.anchor.as_ref()
    }

    #[inline(always)]
    pub const fn anchor_ref(&self) -> LatticeAnchorRef {
        self.anchor
    }

    #[inline]
    pub const fn revision(&self) -> Revision {
        self.revision
    }

    pub(in crate::dataflow) fn increment_revision(&mut self) {
        self.revision += 1;
    }

    #[inline]
    pub fn debug_name(&self) -> &'static str {
        self.descriptor().debug_name()
    }

    #[inline(always)]
    pub(super) fn descriptor(&self) -> &AnalysisStateDescriptor {
        unsafe { self.descriptor.as_ref() }
    }

    #[allow(unused)]
    #[inline]
    pub(super) fn state(&self) -> NonNull<dyn AnalysisState> {
        let descriptor = self.descriptor();
        unsafe {
            let ptr = self as *const Self;
            let ptr =
                NonNull::new_unchecked(ptr.byte_add(descriptor.offset()).cast::<()>().cast_mut());
            NonNull::<dyn AnalysisState>::from_raw_parts(ptr, descriptor.metadata())
        }
    }

    pub(in crate::dataflow) fn borrow_state<T: BuildableAnalysisState>(&self) -> EntityRef<'_, T> {
        let descriptor = self.descriptor();
        assert_eq!(descriptor.type_id(), &TypeId::of::<T>());
        unsafe {
            let ptr = self as *const Self;
            let ptr =
                NonNull::new_unchecked(ptr.byte_add(descriptor.offset()).cast::<()>().cast_mut());
            let raw_entity = ptr.cast::<crate::ir::entity::RawEntity<T>>();
            raw_entity.as_ref().borrow()
        }
    }

    #[inline]
    pub fn subscriptions(&self) -> Ref<'_, [AnalysisStateSubscription]> {
        Ref::map(self.subscriptions.borrow(), |subs| subs.as_slice())
    }

    pub fn on_update_subscribers_count(&self) -> usize {
        self.subscriptions
            .borrow()
            .iter()
            .filter(|sub| matches!(sub, AnalysisStateSubscription::OnUpdate { .. }))
            .count()
    }

    pub fn on_update_subscribers(&self) -> SmallVec<[NonNull<dyn DataFlowAnalysis>; 4]> {
        self.subscriptions
            .borrow()
            .iter()
            .filter_map(|sub| match sub {
                AnalysisStateSubscription::OnUpdate { analysis } => Some(*analysis),
                _ => None,
            })
            .collect::<SmallVec<[_; 4]>>()
    }

    /// Add a subscription for `analysis` at `point` to this state
    pub fn subscribe(&self, subscriber: AnalysisStateSubscription) {
        self.subscriptions.borrow_mut().insert(subscriber);
    }
}

#[derive(PartialEq, Eq, Copy, Clone)]
pub enum AnalysisStateSubscription {
    /// This subscribes `analysis` to state changes handled by the `on_update` callback of the
    /// analysis state.
    OnUpdate {
        /// The analysis that subscribed to changes
        analysis: NonNull<dyn DataFlowAnalysis>,
    },
    /// This subscribes `analysis` to state changes by re-running it on `point`
    ///
    /// Point might be the same as the analysis state anchor, but doesn't have to be.
    AtPoint {
        /// The analysis to run
        analysis: NonNull<dyn DataFlowAnalysis>,
        /// The point at which to run it
        point: ProgramPoint,
    },
    /// This subscribes `analysis` to state changes by re-running it on all uses of the anchor value.
    ///
    /// NOTE: This subscription type is only valid for `ValueRef` anchors for the moment. In the
    /// future, we might be able to expand this to other forms of usable entities, e.g. symbols.
    Uses {
        /// The analysis to run
        analysis: NonNull<dyn DataFlowAnalysis>,
    },
}

impl AnalysisStateSubscription {
    pub fn handle_state_change<T, A>(
        &self,
        anchor: &dyn LatticeAnchor,
        worklist: &mut VecDeque<QueuedAnalysis, A>,
    ) where
        T: AnalysisState + 'static,
        A: alloc::alloc::Allocator,
    {
        log::trace!(
            "handling analysis state change to anchor '{anchor}' for {}",
            core::any::type_name::<T>()
        );
        match *self {
            // Delegated to [AnalysisStateSubscriptionBehavior::on_update]
            Self::OnUpdate { analysis: _ } => (),
            // Re-run `analysis` at `point`
            Self::AtPoint { analysis, point } => {
                log::trace!("enqueuing {} at {point}", unsafe { analysis.as_ref().debug_name() });
                worklist.push_back(QueuedAnalysis { point, analysis });
            }
            // Re-run `analysis` for all uses of the current value
            Self::Uses { analysis } => {
                let value = anchor
                    .as_value()
                    .unwrap_or_else(|| panic!("expected value anchor, got: {:?}", anchor));
                for user in value.borrow().iter_uses() {
                    let user = user.owner;
                    let point = ProgramPoint::after(user);
                    worklist.push_back(QueuedAnalysis { point, analysis });
                }
            }
        }
    }
}

pub trait AnalysisStateSubscriptionBehavior: AnalysisState {
    /// Called when an [AnalysisState] is being queried by an analysis via [DataFlowSolver::require]
    ///
    /// This invokes state-specific logic for how to subscribe the dependent analysis to changes
    /// of that state. By default, one of two options behaviors is applied:
    ///
    /// * For program point anchors, the analysis is re-run at `dependent` on state changes
    /// * For value anchors, the analysis is re-run at `dependent` _and_ at all uses of the value
    ///
    /// NOTE: Subscriptions are established when an analysis is queried, if you wish to execute
    /// custom behavior when updates are being propagated, see `on_update`.
    fn on_require_analysis(
        &self,
        info: &mut AnalysisStateInfo,
        current_analysis: NonNull<dyn DataFlowAnalysis>,
        dependent: ProgramPoint,
    ) {
        on_require_analysis_fallback(info, current_analysis, dependent);
    }

    /// Called when an analysis subscribes to any changes to the current [AnalysisState].
    fn on_subscribe(&self, subscriber: NonNull<dyn DataFlowAnalysis>, info: &AnalysisStateInfo) {
        log::trace!(
            "subscribing {} to state updates for analysis state {} at {}",
            unsafe { subscriber.as_ref().debug_name() },
            info.debug_name(),
            info.anchor()
        );
        info.subscribe(AnalysisStateSubscription::OnUpdate {
            analysis: subscriber,
        });
    }

    /// Called when changes to an [AnalysisState] are being propagated. This callback has visibility
    /// into the modified state, and can use that information to modify other analysis states that
    /// may be directly/indirectly affected by the changes.
    #[allow(unused_variables)]
    fn on_update(&self, info: &mut AnalysisStateInfo, worklist: &mut AnalysisQueue) {}
}

impl<T: AnalysisState> AnalysisStateSubscriptionBehavior for T {
    default fn on_require_analysis(
        &self,
        info: &mut AnalysisStateInfo,
        current_analysis: NonNull<dyn DataFlowAnalysis>,
        dependent: ProgramPoint,
    ) {
        on_require_analysis_fallback(info, current_analysis, dependent);
    }

    /// Called when an analysis subscribes to any changes to the current [AnalysisState].
    default fn on_subscribe(
        &self,
        subscriber: NonNull<dyn DataFlowAnalysis>,
        info: &AnalysisStateInfo,
    ) {
        log::trace!(
            "subscribing {} to state updates for analysis state {} at {}",
            unsafe { subscriber.as_ref().debug_name() },
            info.debug_name(),
            info.anchor()
        );
        info.subscribe(AnalysisStateSubscription::OnUpdate {
            analysis: subscriber,
        });
    }

    /// Called when changes to an [AnalysisState] are being propagated. This callback has visibility
    /// into the modified state, and can use that information to modify other analysis states that
    /// may be directly/indirectly affected by the changes.
    default fn on_update(&self, info: &mut AnalysisStateInfo, worklist: &mut AnalysisQueue) {
        log::trace!(
            "notifying {} subscribers of update to sparse lattice at {}",
            info.on_update_subscribers_count(),
            info.anchor()
        );
        if let Some(value) = info.anchor().as_value() {
            for user in value.borrow().uses() {
                let user = user.owner;
                for subscriber in info.on_update_subscribers() {
                    worklist.push_back(QueuedAnalysis {
                        point: ProgramPoint::after(user),
                        analysis: subscriber,
                    });
                }
            }
        } else if let Some(point) = info.anchor().as_program_point() {
            for subscriber in info.on_update_subscribers() {
                worklist.push_back(QueuedAnalysis {
                    point,
                    analysis: subscriber,
                });
            }
        }
    }
}

pub fn on_require_analysis_fallback(
    info: &mut AnalysisStateInfo,
    current_analysis: NonNull<dyn DataFlowAnalysis>,
    dependent: ProgramPoint,
) {
    log::trace!(
        "applying default subscriptions for {} at {} for {dependent} for analysis state {}",
        unsafe { current_analysis.as_ref().debug_name() },
        info.anchor(),
        info.debug_name()
    );
    if info.anchor().is_value() {
        info.subscribe(AnalysisStateSubscription::AtPoint {
            analysis: current_analysis,
            point: dependent,
        });
        info.subscribe(AnalysisStateSubscription::Uses {
            analysis: current_analysis,
        });
    } else {
        info.subscribe(AnalysisStateSubscription::AtPoint {
            analysis: current_analysis,
            point: dependent,
        });
    }
}
