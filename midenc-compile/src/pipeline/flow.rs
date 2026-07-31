use super::CheckpointId;

/// Records that compilation reached its requested goal and should stop.
///
/// The owned artifact has already been moved into request-local state by
/// [`crate::pipeline::TargetContext::checkpoint`]; this value carries only the
/// checkpoint identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stopped {
    checkpoint: CheckpointId,
}

impl Stopped {
    /// Construct a stop marker for `checkpoint`.
    pub const fn new(checkpoint: CheckpointId) -> Self {
        Self { checkpoint }
    }

    /// The checkpoint at which compilation stopped.
    pub const fn checkpoint(&self) -> CheckpointId {
        self.checkpoint
    }
}

/// The result of publishing an artifact at a checkpoint.
///
/// This is a type alias for [`core::ops::ControlFlow<Stopped, T>`].
///
/// * `Continue` hands the artifact back so the frontend can keep working with it.
/// * `Break` means the requested goal was reached and the artifact has been captured.
pub type Flow<T> = core::ops::ControlFlow<Stopped, T>;
