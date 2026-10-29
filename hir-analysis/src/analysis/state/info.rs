use alloc::{boxed::Box, collections::VecDeque};
use core::{
    any::TypeId,
    cell::{Ref, RefCell},
    hash::Hash,
    ptr::NonNull,
};

use midenc_hir::{EntityRef, FxHashSet, SmallVec, entity::RawEntity};

use super::*;
use crate::{AnalysisQueue, LatticeAnchor, LatticeAnchorRef, solver::QueuedAnalysis};

pub type Revision = u32;

/// First-insertion order with allocation-free membership checks for up to four subscribers.
/// Larger states keep a separate membership index to avoid quadratic subscription work.
struct OrderedSubscriptions<T> {
    items: SmallVec<[T; 4]>,
    membership: Option<Box<FxHashSet<T>>>,
}

impl<T> Default for OrderedSubscriptions<T> {
    fn default() -> Self {
        Self {
            items: SmallVec::new(),
            membership: None,
        }
    }
}

impl<T: Copy + Eq + Hash> OrderedSubscriptions<T> {
    fn insert(&mut self, item: T) -> bool {
        if let Some(membership) = self.membership.as_mut() {
            if !membership.insert(item) {
                return false;
            }
        } else {
            if self.items.contains(&item) {
                return false;
            }
            if self.items.len() == self.items.inline_size() {
                let mut membership: FxHashSet<T> = self.items.iter().copied().collect();
                membership.insert(item);
                self.membership = Some(Box::new(membership));
            }
        }
        self.items.push(item);
        true
    }

    fn as_slice(&self) -> &[T] {
        self.items.as_slice()
    }
}

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
    subscriptions: RefCell<OrderedSubscriptions<AnalysisStateSubscription>>,
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

    pub(crate) fn increment_revision(&mut self) {
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

    pub(crate) fn borrow_state<T: BuildableAnalysisState>(&self) -> EntityRef<'_, T> {
        let descriptor = self.descriptor();
        assert_eq!(descriptor.type_id(), &TypeId::of::<T>());
        unsafe {
            let ptr = self as *const Self;
            let ptr =
                NonNull::new_unchecked(ptr.byte_add(descriptor.offset()).cast::<()>().cast_mut());
            let raw_entity = ptr.cast::<RawEntity<T>>();
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
            .as_slice()
            .iter()
            .filter(|sub| matches!(sub, AnalysisStateSubscription::OnUpdate { .. }))
            .count()
    }

    pub fn on_update_subscribers(&self) -> SmallVec<[NonNull<dyn DataFlowAnalysis>; 4]> {
        self.subscriptions
            .borrow()
            .as_slice()
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

#[derive(PartialEq, Eq, Hash, Copy, Clone)]
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
            target: "analysis:subscription",
            "handling analysis state change to anchor '{anchor}' for {}",
            core::any::type_name::<T>()
        );
        match *self {
            // Delegated to [AnalysisStateSubscriptionBehavior::on_update]
            Self::OnUpdate { analysis: _ } => (),
            // Re-run `analysis` at `point`
            Self::AtPoint { analysis, point } => {
                log::trace!(target: "analysis:subscription", "enqueuing {} at {point}", unsafe { analysis.as_ref().debug_name() });
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
    /// Called when an [AnalysisState] is being queried by an analysis via [crate::DataFlowSolver::require]
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
            target: "analysis:subscription",
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
            target: "analysis:subscription",
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
            target: "analysis:subscription",
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
        target: "analysis:subscription",
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

#[cfg(test)]
mod tests {
    use alloc::rc::Rc;
    use core::{
        cell::Cell,
        hash::{Hash, Hasher},
    };

    use midenc_dialect_arith::ArithOpBuilder;
    use midenc_hir::{
        SourceSpan, Type, dialects::builtin::BuiltinOpBuilder, pass::AnalysisManager, testing::Test,
    };

    use super::*;
    use crate::{DataFlowSolver, Lattice};

    #[repr(transparent)]
    struct TestAnalysis<T>(T);

    impl<T: 'static> DataFlowAnalysis for TestAnalysis<T> {
        fn analysis_id(&self) -> TypeId {
            TypeId::of::<Self>()
        }

        fn initialize(
            &self,
            _: &midenc_hir::Operation,
            _: &mut DataFlowSolver,
            _: AnalysisManager,
        ) -> Result<(), midenc_hir::Report> {
            Ok(())
        }

        fn visit(
            &self,
            _: &ProgramPoint,
            _: &mut DataFlowSolver,
        ) -> Result<(), midenc_hir::Report> {
            Ok(())
        }
    }

    #[test]
    fn subscriptions_preserve_identity_order_and_notification_deduplication() {
        let mut test = Test::new("subscriptions", &[], &[Type::U32]);
        let (value, constant, ret) = {
            let mut builder = test.function_builder();
            let value = builder.u32(7, SourceSpan::UNKNOWN);
            let ret = builder.ret([value], SourceSpan::UNKNOWN).unwrap().as_operation_ref();
            (value, value.borrow().get_defining_op().unwrap(), ret)
        };
        let solver = DataFlowSolver::default();
        let anchor = solver.create_lattice_anchor(value);
        let alloc = blink_alloc::Blink::new();
        let mut descriptors = Default::default();
        let key = AnalysisStateKey::new::<Lattice<u32>>(anchor);
        let state =
            RawAnalysisStateInfo::<Lattice<u32>>::alloc(&alloc, &mut descriptors, key, anchor);
        let raw_info = RawAnalysisStateInfo::as_info_ptr(state);
        let analysis = TestAnalysis(TestAnalysis(0u8));
        let other = TestAnalysis(1u8);
        let first = NonNull::from(&analysis as &dyn DataFlowAnalysis);
        let same_address = NonNull::from(&analysis.0 as &dyn DataFlowAnalysis);
        let second = NonNull::from(&other as &dyn DataFlowAnalysis);
        // Both halves of the wide analysis pointer are part of subscription identity.
        assert!(core::ptr::addr_eq(first.as_ptr(), same_address.as_ptr()));
        assert!(!core::ptr::eq(first.as_ptr(), same_address.as_ptr()));
        let before = ProgramPoint::before(constant);
        let after = ProgramPoint::after(constant);
        let expected = [
            AnalysisStateSubscription::OnUpdate { analysis: first },
            AnalysisStateSubscription::AtPoint {
                analysis: first,
                point: before,
            },
            AnalysisStateSubscription::Uses { analysis: first },
            AnalysisStateSubscription::AtPoint {
                analysis: first,
                point: after,
            },
            AnalysisStateSubscription::OnUpdate {
                analysis: same_address,
            },
            AnalysisStateSubscription::AtPoint {
                analysis: same_address,
                point: before,
            },
            AnalysisStateSubscription::AtPoint {
                analysis: second,
                point: before,
            },
            AnalysisStateSubscription::AtPoint {
                analysis: first,
                point: ProgramPoint::before(ret),
            },
        ];
        {
            let info = unsafe { raw_info.as_ref() };
            for subscription in expected {
                info.subscribe(subscription);
                info.subscribe(subscription);
            }
            for subscription in expected.into_iter().rev() {
                info.subscribe(subscription);
            }
            assert!(*info.subscriptions() == expected);
            assert_eq!(info.on_update_subscribers_count(), 2);
            assert!(info.on_update_subscribers().as_slice() == [first, same_address]);
        }

        let worklist = Rc::new(RefCell::new(AnalysisQueue::new_in(Default::default())));
        {
            let mut state =
                unsafe { AnalysisStateGuardMut::<Lattice<u32>>::new(raw_info, worklist.clone()) };
            *state.value_mut() = 1;
        }
        let queue = worklist.borrow();
        let expected_notifications = [
            (before, first),
            (ProgramPoint::after(ret), first),
            (after, first),
            (before, same_address),
            (before, second),
            (ProgramPoint::before(ret), first),
            (ProgramPoint::after(ret), first),
            (ProgramPoint::after(ret), same_address),
        ];
        assert_eq!(queue.len(), expected_notifications.len());
        for (actual, (point, analysis)) in queue.iter().zip(expected_notifications) {
            assert_eq!(actual.point, point);
            assert!(core::ptr::eq(actual.analysis.as_ptr(), analysis.as_ptr()));
        }
    }

    #[derive(Clone, Copy)]
    struct Counted<'a> {
        value: usize,
        equalities: &'a Cell<usize>,
        hashes: &'a Cell<usize>,
    }

    impl PartialEq for Counted<'_> {
        fn eq(&self, other: &Self) -> bool {
            self.equalities.set(self.equalities.get() + 1);
            self.value == other.value
        }
    }

    impl Eq for Counted<'_> {}

    impl Hash for Counted<'_> {
        fn hash<H: Hasher>(&self, state: &mut H) {
            self.hashes.set(self.hashes.get() + 1);
            self.value.hash(state);
        }
    }

    #[test]
    fn subscription_membership_stays_inline_until_fifth_unique_item() {
        let equalities = Cell::new(0);
        let hashes = Cell::new(0);
        let item = |value| Counted {
            value,
            equalities: &equalities,
            hashes: &hashes,
        };
        let mut subscriptions = OrderedSubscriptions::default();
        assert!(!subscriptions.items.spilled());
        assert!(subscriptions.membership.is_none());
        for value in 0..4 {
            assert!(subscriptions.insert(item(value)));
            assert!(!subscriptions.items.spilled());
            assert!(subscriptions.membership.is_none());
        }
        assert!(!subscriptions.insert(item(3)));
        assert!(!subscriptions.items.spilled());
        assert!(subscriptions.membership.is_none());
        assert_eq!(hashes.get(), 0);

        assert!(subscriptions.insert(item(4)));
        assert!(subscriptions.items.spilled());
        assert!(subscriptions.membership.is_some());
        for value in (0..5).rev() {
            assert!(!subscriptions.insert(item(value)));
        }
        assert!(subscriptions.as_slice().iter().map(|item| item.value).eq(0..5));
    }

    #[test]
    fn subscription_fanout_has_bounded_membership_work() {
        const FANOUT: usize = 4096;
        let equalities = Cell::new(0);
        let hashes = Cell::new(0);
        let mut subscriptions = OrderedSubscriptions::default();
        for value in (0..FANOUT).chain((0..FANOUT).rev()) {
            subscriptions.insert(Counted {
                value,
                equalities: &equalities,
                hashes: &hashes,
            });
        }
        assert_eq!(subscriptions.as_slice().len(), FANOUT);
        assert!(subscriptions.as_slice().iter().map(|item| item.value).eq(0..FANOUT));
        assert!(
            equalities.get() < 32 * FANOUT,
            "{} equality checks for {FANOUT} subscribers",
            equalities.get()
        );
        assert!(hashes.get() < 16 * FANOUT, "{} hashes for {FANOUT} subscribers", hashes.get());
    }
}
