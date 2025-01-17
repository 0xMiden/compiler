use alloc::collections::VecDeque;
use core::{any::TypeId, hash::Hash, ptr::NonNull};

use super::*;
use crate::{
    adt::SmallSet,
    dataflow::{solver::QueuedAnalysis, AnalysisQueue, LatticeAnchor, LatticeAnchorRef},
    EntityRef,
};

pub type Revision = u32;

pub struct AnalysisStateInfo {
    /// The unique hash of this analysis state's identity (type id + anchor)
    key: AnalysisStateKey,
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
    pub(super) subscriptions: SmallSet<AnalysisStateSubscription, 4>,
}
impl AnalysisStateInfo {
    #[inline]
    pub(super) fn new(
        key: AnalysisStateKey,
        descriptor: NonNull<AnalysisStateDescriptor>,
        anchor: LatticeAnchorRef,
    ) -> Self {
        Self {
            key,
            descriptor,
            anchor,
            revision: 0,
            subscriptions: Default::default(),
        }
    }

    pub fn compute_key_for<T, A>(anchor: &A) -> AnalysisStateKey
    where
        T: BuildableAnalysisState,
        A: LatticeAnchor + Hash,
    {
        use core::hash::{Hash, Hasher};

        let type_id = TypeId::of::<T>();
        let mut hasher = rustc_hash::FxHasher::default();
        type_id.hash(&mut hasher);
        anchor.hash(&mut hasher);
        AnalysisStateKey(hasher.finish())
    }

    pub fn key(&self) -> AnalysisStateKey {
        self.key
    }

    #[inline]
    pub fn anchor(&self) -> &dyn LatticeAnchor {
        &self.anchor
    }

    #[inline]
    pub const fn revision(&self) -> Revision {
        self.revision
    }

    pub(in crate::dataflow) fn increment_revision(&mut self) {
        self.revision += 1;
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
    pub fn subscriptions(&self) -> &[AnalysisStateSubscription] {
        self.subscriptions.as_slice()
    }

    /// Add a subscription for `analysis` at `point` to this state
    pub fn subscribe(&mut self, subscriber: AnalysisStateSubscription) {
        self.subscriptions.insert(subscriber);
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
    pub fn handle_state_change<A>(
        &self,
        anchor: &dyn LatticeAnchor,
        worklist: &mut VecDeque<QueuedAnalysis, A>,
    ) where
        A: alloc::alloc::Allocator,
    {
        match *self {
            Self::OnUpdate { analysis: _ } => {
                todo!()
            }
            Self::AtPoint { analysis, point } => {
                worklist.push_back(QueuedAnalysis { point, analysis });
            }
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

pub trait AnalysisStateSubscriptionBehavior {
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
    );

    fn on_subscribe(&self, subscriber: NonNull<dyn DataFlowAnalysis>, info: &mut AnalysisStateInfo);

    /// Called when changes to an [AnalysisState] are being propagated. This callback has visibility
    /// into the modified state, and can use that information to modify other analysis states that
    /// may be directly/indirectly affected by the changes.
    fn on_update(&self, info: &mut AnalysisStateInfo, worklist: &mut AnalysisQueue);
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

    default fn on_subscribe(
        &self,
        subscriber: NonNull<dyn DataFlowAnalysis>,
        info: &mut AnalysisStateInfo,
    ) {
        info.subscribe(AnalysisStateSubscription::OnUpdate {
            analysis: subscriber,
        });
    }

    default fn on_update(&self, _info: &mut AnalysisStateInfo, _worklist: &mut AnalysisQueue) {}
}

pub fn on_require_analysis_fallback(
    info: &mut AnalysisStateInfo,
    current_analysis: NonNull<dyn DataFlowAnalysis>,
    dependent: ProgramPoint,
) {
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
