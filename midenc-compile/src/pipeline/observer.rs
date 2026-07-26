use alloc::vec::Vec;

use super::{Artifact, CheckpointId};

/// The role a target plays in the assembly of the selected top-level target.
///
/// Only [`TargetRole::Root`] is given the caller's requested goal; every other role is
/// always compiled to a full package.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TargetRole {
    /// The selected top-level target.
    Root,
    /// The implicit library target of the root package, linked into an executable.
    RequiredLibrary,
    /// A direct or transitive dependency.
    Dependency,
}

impl TargetRole {
    /// Returns true if this is the selected top-level target.
    pub const fn is_root(&self) -> bool {
        matches!(self, Self::Root)
    }
}

/// Observes artifacts as they are produced, without taking ownership.
///
/// Observers are notified for every target, tagged with the target's role. They must not
/// perform output I/O; artifact rendering is a separate concern.
pub trait Observer {
    /// Called after `artifact` has been produced at `checkpoint` by a target in `role`.
    fn on_checkpoint(&mut self, checkpoint: CheckpointId, role: TargetRole, artifact: &Artifact);
}

/// An [`Observer`] that records the checkpoint trace, for use in tests.
#[derive(Debug, Default)]
pub struct RecordingObserver {
    records: Vec<(CheckpointId, TargetRole)>,
}

impl RecordingObserver {
    /// The recorded trace, in the order checkpoints were reached.
    pub fn records(&self) -> &[(CheckpointId, TargetRole)] {
        &self.records
    }
}

impl Observer for RecordingObserver {
    fn on_checkpoint(&mut self, checkpoint: CheckpointId, role: TargetRole, _artifact: &Artifact) {
        self.records.push((checkpoint, role));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::{Artifact, ArtifactId, CheckpointId};

    #[test]
    fn recording_observer_captures_checkpoint_and_role_in_order() {
        let mut observer = RecordingObserver::default();
        let hir = Artifact::new(ArtifactId::HIR, 1u32);
        let masm = Artifact::new(ArtifactId::MASM, 2u32);

        observer.on_checkpoint(CheckpointId::HIR_INITIAL, TargetRole::Dependency, &hir);
        observer.on_checkpoint(CheckpointId::MASM_LOWERED, TargetRole::Root, &masm);

        assert_eq!(
            observer.records(),
            &[
                (CheckpointId::HIR_INITIAL, TargetRole::Dependency),
                (CheckpointId::MASM_LOWERED, TargetRole::Root),
            ]
        );
    }

    #[test]
    fn only_root_is_root() {
        assert!(TargetRole::Root.is_root());
        assert!(!TargetRole::RequiredLibrary.is_root());
        assert!(!TargetRole::Dependency.is_root());
    }
}
