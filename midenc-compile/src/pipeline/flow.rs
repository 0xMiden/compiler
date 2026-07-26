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
/// `Continue` hands the artifact back so the frontend can keep working with it.
/// `Stop` means the requested goal was reached and the artifact has been captured.
#[derive(Debug)]
pub enum Flow<T> {
    /// Compilation should continue; the artifact is returned to the caller.
    Continue(T),
    /// Compilation reached its goal; the artifact has been captured.
    Stop(Stopped),
}

impl<T> Flow<T> {
    /// Transform the artifact carried by `Continue`, leaving `Stop` untouched.
    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> Flow<U> {
        match self {
            Self::Continue(value) => Flow::Continue(f(value)),
            Self::Stop(stopped) => Flow::Stop(stopped),
        }
    }

    /// Returns true if compilation should stop here.
    pub const fn is_stop(&self) -> bool {
        matches!(self, Self::Stop(_))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::CheckpointId;

    #[test]
    fn flow_continue_carries_the_artifact() {
        match Flow::Continue(42u32) {
            Flow::Continue(value) => assert_eq!(value, 42),
            Flow::Stop(_) => panic!("expected Continue"),
        }
    }

    #[test]
    fn flow_map_transforms_only_the_continue_payload() {
        assert!(matches!(Flow::Continue(2u32).map(|v| v * 3), Flow::Continue(6)));

        let stopped: Flow<u32> = Flow::Stop(Stopped::new(CheckpointId::HIR_INITIAL));
        match stopped.map(|v| v * 3) {
            Flow::Stop(s) => assert_eq!(s.checkpoint(), CheckpointId::HIR_INITIAL),
            Flow::Continue(_) => panic!("expected Stop"),
        }
    }

    #[test]
    fn is_stop_distinguishes_the_variants() {
        assert!(!Flow::Continue(0u32).is_stop());
        assert!(Flow::<u32>::Stop(Stopped::new(CheckpointId::HIR_INITIAL)).is_stop());
    }
}
