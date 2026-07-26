use alloc::{rc::Rc, vec::Vec};
use core::cell::RefCell;

use super::{Artifact, CaptureSlot, CheckpointId, Goal, Observer, Outcome, TargetRole};
use crate::CompilerResult;

/// The parts of a compilation request that are identical for every target.
///
/// The assembler invokes frontend callbacks once per target, so a
/// [`TargetContext`](super::TargetContext) is built per callback; the goal, the observers
/// and the single capture slot are fixed for the whole request. This owns them, and every
/// target context borrows it.
pub struct RequestState {
    /// The checkpoint the root target stops at; see [`RequestState::goal`].
    goal: Goal,
    /// The observers to notify at every checkpoint, in the order they were given.
    ///
    /// The elements are `Rc<RefCell<dyn Observer>>` because
    /// [`Observer::on_checkpoint`] takes `&mut self`, and observation is only useful if
    /// the caller who supplied an observer can read back what it saw once the request is
    /// over — so the observers are shared, not owned outright. The `Vec` itself needs no
    /// cell: the list is fixed at construction and only its elements are ever mutated.
    observers: Vec<Rc<RefCell<dyn Observer>>>,
    /// The request's single captured artifact, private to this type: it is written only by
    /// [`RequestState::capture`] and read only by [`RequestState::take_outcome`].
    capture: RefCell<CaptureSlot>,
}

impl RequestState {
    /// Construct the state for a request that runs to `goal`, notifying `observers`.
    pub fn new(goal: Goal, observers: Vec<Rc<RefCell<dyn Observer>>>) -> Self {
        Self {
            goal,
            observers,
            capture: RefCell::new(CaptureSlot::default()),
        }
    }

    /// The checkpoint at which this request stops.
    ///
    /// Only the root target may stop here; every other role is compiled to completion, so
    /// this is not a goal for them. See
    /// [`TargetContext::checkpoint`](super::TargetContext::checkpoint).
    pub fn goal(&self) -> Goal {
        self.goal
    }

    /// Notify every observer, in order, that `artifact` was produced at `checkpoint` by a
    /// target in `role`.
    ///
    /// Observation is orthogonal to capture: this never stores anything.
    pub fn notify(&self, checkpoint: CheckpointId, role: TargetRole, artifact: &Artifact) {
        for observer in &self.observers {
            observer.borrow_mut().on_checkpoint(checkpoint, role, artifact);
        }
    }

    /// Capture `artifact` as the outcome of this request, produced at `checkpoint`.
    ///
    /// Capturing twice is an internal invariant violation and is reported as an error; the
    /// artifact captured first is kept. See [`CaptureSlot::put`].
    pub fn capture(&self, checkpoint: CheckpointId, artifact: Artifact) -> CompilerResult<()> {
        self.capture.borrow_mut().put(checkpoint, artifact)
    }

    /// Take the captured outcome, if the request reached its goal, leaving the slot empty.
    ///
    /// This is how the driver gets the artifact out: nothing else can observe the capture
    /// slot.
    ///
    /// Takes `&self` rather than consuming the state, because the state is shared. Every
    /// [`FrontendProvider`](super::FrontendProvider) of a request holds an
    /// `Rc<RequestState>`, and the providers are owned by the `ProjectAssembler` that ran
    /// them — so a consuming form would oblige the driver to drop the assembler and then
    /// `Rc::try_unwrap`, which is correct only for as long as no other clone outlives it,
    /// and a runtime panic the day one does. Taking through a shared reference cannot be
    /// wrong that way, and the slot is still emptied, so the artifact is handed out once.
    pub fn take_outcome(&self) -> Option<Outcome> {
        self.capture.borrow_mut().take()
    }
}

#[cfg(test)]
mod tests {
    use alloc::{format, rc::Rc, vec, vec::Vec};
    use core::cell::RefCell;

    use super::*;
    use crate::pipeline::{Artifact, ArtifactId, CheckpointId, Goal, Observer, TargetRole};

    #[derive(Debug, PartialEq)]
    struct Payload(u32);

    /// One entry of the shared notification log: who was notified, and with what.
    type Record = (&'static str, CheckpointId, TargetRole, ArtifactId);

    /// An observer that appends to a log shared with every other observer.
    ///
    /// A per-observer trace could only show that each observer saw the checkpoints in
    /// order; one shared log also shows the order the observers themselves were notified
    /// in, which is the ordering guarantee callers depend on.
    struct LoggingObserver {
        name: &'static str,
        log: Rc<RefCell<Vec<Record>>>,
    }

    /// An observer named `name` that logs into `log`, ready to hand to [`RequestState::new`].
    fn logging(name: &'static str, log: &Rc<RefCell<Vec<Record>>>) -> Rc<RefCell<dyn Observer>> {
        Rc::new(RefCell::new(LoggingObserver {
            name,
            log: log.clone(),
        }))
    }

    impl Observer for LoggingObserver {
        fn on_checkpoint(
            &mut self,
            checkpoint: CheckpointId,
            role: TargetRole,
            artifact: &Artifact,
        ) {
            self.log.borrow_mut().push((self.name, checkpoint, role, artifact.id()));
        }
    }

    #[test]
    fn notification_reaches_every_observer_in_order() {
        let log = Rc::new(RefCell::new(Vec::new()));
        let state = RequestState::new(
            Goal::at(CheckpointId::MASM_LOWERED),
            vec![logging("first", &log), logging("second", &log)],
        );

        state.notify(
            CheckpointId::HIR_INITIAL,
            TargetRole::Root,
            &Artifact::new(ArtifactId::HIR, Payload(1)),
        );
        state.notify(
            CheckpointId::MASM_LOWERED,
            TargetRole::Dependency,
            &Artifact::new(ArtifactId::MASM, Payload(2)),
        );

        assert_eq!(
            &*log.borrow(),
            &[
                ("first", CheckpointId::HIR_INITIAL, TargetRole::Root, ArtifactId::HIR),
                ("second", CheckpointId::HIR_INITIAL, TargetRole::Root, ArtifactId::HIR),
                ("first", CheckpointId::MASM_LOWERED, TargetRole::Dependency, ArtifactId::MASM),
                ("second", CheckpointId::MASM_LOWERED, TargetRole::Dependency, ArtifactId::MASM),
            ],
            "every observer must be notified of every checkpoint, in registration order, with the \
             publishing target's role and the published artifact"
        );
    }

    #[test]
    fn notification_does_not_capture() {
        let state = RequestState::new(Goal::at(CheckpointId::HIR_INITIAL), Vec::new());

        state.notify(
            CheckpointId::HIR_INITIAL,
            TargetRole::Root,
            &Artifact::new(ArtifactId::HIR, Payload(1)),
        );

        assert!(
            state.take_outcome().is_none(),
            "notifying observers must not capture; deciding what to capture is the caller's"
        );
    }

    #[test]
    fn take_outcome_yields_what_was_captured_once() {
        let state = RequestState::new(Goal::at(CheckpointId::HIR_INITIAL), Vec::new());
        assert_eq!(state.goal(), Goal::at(CheckpointId::HIR_INITIAL));

        state
            .capture(CheckpointId::HIR_INITIAL, Artifact::new(ArtifactId::HIR, Payload(7)))
            .expect("the first capture of a request must succeed");

        let outcome = state.take_outcome().expect("a captured artifact must be recoverable");
        assert_eq!(outcome.checkpoint(), CheckpointId::HIR_INITIAL);
        assert_eq!(
            outcome.downcast::<Payload>().expect("the captured payload must survive intact"),
            Payload(7)
        );

        assert!(
            state.take_outcome().is_none(),
            "taking through a shared reference must empty the slot, so one request's artifact is \
             handed out exactly once however many holders of the state remain"
        );
    }

    #[test]
    fn a_second_capture_is_rejected_and_the_first_survives() {
        let state = RequestState::new(Goal::at(CheckpointId::HIR_INITIAL), Vec::new());
        state
            .capture(CheckpointId::HIR_INITIAL, Artifact::new(ArtifactId::HIR, Payload(1)))
            .expect("the first capture should succeed");

        let err = state
            .capture(CheckpointId::HIR_TRANSFORMED, Artifact::new(ArtifactId::HIR, Payload(2)))
            .expect_err("a request may only capture one artifact");
        let msg = format!("{err}");
        assert!(msg.contains("already captured"), "the rejection must say why: {msg}");

        let outcome = state.take_outcome().expect("the first capture must still be there");
        assert_eq!(outcome.checkpoint(), CheckpointId::HIR_INITIAL);
        assert_eq!(
            outcome.downcast::<Payload>().expect("payload"),
            Payload(1),
            "a rejected capture must not overwrite the artifact already captured"
        );
    }
}
