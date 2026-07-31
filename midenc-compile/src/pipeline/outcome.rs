use alloc::{format, sync::Arc};
use core::any::Any;

use miden_mast_package::Package as MastPackage;
use midenc_session::diagnostics::Report;

use super::{Artifact, ArtifactId, CheckpointId};
use crate::CompilerResult;

/// The result of a compilation request.
///
/// An outcome owns exactly one artifact: the one produced at the checkpoint where
/// compilation stopped. A request that ran to completion stops at
/// [`CheckpointId::PACKAGE_ASSEMBLED`].
#[derive(Debug)]
pub struct Outcome {
    checkpoint: CheckpointId,
    artifact: Artifact,
}

impl Outcome {
    /// Construct an outcome for `artifact`, produced at `checkpoint`.
    pub fn new(checkpoint: CheckpointId, artifact: Artifact) -> Self {
        Self {
            checkpoint,
            artifact,
        }
    }

    /// The checkpoint at which compilation stopped.
    pub fn checkpoint(&self) -> CheckpointId {
        self.checkpoint
    }

    /// Borrow the produced artifact.
    pub fn artifact(&self) -> &Artifact {
        &self.artifact
    }

    /// Take ownership of the produced artifact.
    pub fn into_artifact(self) -> Artifact {
        self.artifact
    }

    /// Recover the concrete artifact value, returning it unchanged if `T` does not match.
    pub fn downcast<T: Any>(self) -> Result<T, Artifact> {
        self.artifact.downcast::<T>()
    }

    /// Take the assembled package, failing if compilation stopped before assembly.
    pub fn into_package(self) -> CompilerResult<Arc<MastPackage>> {
        let checkpoint = self.checkpoint;
        if self.artifact.id() != ArtifactId::PACKAGE {
            return Err(Report::msg(format!(
                "expected an assembled package, but compilation stopped at '{checkpoint}' \
                 producing a '{}' artifact",
                self.artifact.id()
            )));
        }
        self.artifact.downcast::<Arc<MastPackage>>().map_err(|artifact| {
            Report::msg(format!(
                "artifact at '{checkpoint}' is tagged '{}' but does not hold a package",
                artifact.id()
            ))
        })
    }
}

#[cfg(test)]
mod tests {
    use alloc::format;

    use super::*;

    #[derive(Debug, PartialEq)]
    struct Hir(u32);

    #[test]
    fn outcome_exposes_its_checkpoint_and_artifact() {
        let outcome =
            Outcome::new(CheckpointId::HIR_INITIAL, Artifact::new(ArtifactId::HIR, Hir(1)));
        assert_eq!(outcome.checkpoint(), CheckpointId::HIR_INITIAL);
        assert_eq!(outcome.artifact().id(), ArtifactId::HIR);
        assert_eq!(outcome.downcast::<Hir>().expect("should downcast"), Hir(1));
    }

    #[test]
    fn into_package_rejects_a_non_package_artifact() {
        let outcome =
            Outcome::new(CheckpointId::HIR_INITIAL, Artifact::new(ArtifactId::HIR, Hir(1)));
        let err = outcome.into_package().expect_err("hir is not a package");
        let rendered = format!("{err}");
        assert!(
            rendered.contains("hir.initial"),
            "diagnostic should name the checkpoint reached, got: {rendered}"
        );
        assert!(
            rendered.contains("compilation stopped at"),
            "diagnostic should come from the artifact id guard, not the downcast failure, got: \
             {rendered}"
        );
    }

    #[test]
    fn into_package_rejects_a_package_tagged_artifact_holding_the_wrong_type() {
        let outcome = Outcome::new(
            CheckpointId::PACKAGE_ASSEMBLED,
            Artifact::new(ArtifactId::PACKAGE, Hir(1)),
        );
        let err = outcome.into_package().expect_err("payload is not a package");
        assert!(format!("{err}").contains("does not hold a package"));
    }
}
