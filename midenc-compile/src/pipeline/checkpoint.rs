use core::fmt;

/// Identifies a point in a frontend's route at which an artifact exists.
///
/// Checkpoints are namespaced by the artifact family they produce, e.g. `hir.initial`.
/// They carry no global ordering: ordering is defined only within a single frontend's
/// declared route. See [`crate::pipeline::FrontendRegistration::route`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CheckpointId(&'static str);

impl CheckpointId {
    /// HIR after semantic analysis and lints.
    pub const HIR_ANALYZED: Self = Self("hir.analyzed");
    /// HIR as first produced by a frontend, before any analysis or rewrites.
    pub const HIR_INITIAL: Self = Self("hir.initial");
    /// HIR after all rewrites have been applied.
    pub const HIR_TRANSFORMED: Self = Self("hir.transformed");
    /// Assembly-ready Miden Assembly, whether lowered from HIR or parsed directly.
    pub const MASM_LOWERED: Self = Self("masm.lowered");
    /// Miden Assembly as parsed directly from MASM sources.
    pub const MASM_PARSED: Self = Self("masm.parsed");
    /// The final assembled MAST package.
    pub const PACKAGE_ASSEMBLED: Self = Self("package.assembled");
    /// WebAssembly produced by a source-language frontend, before HIR translation.
    pub const WASM_PARSED: Self = Self("wasm.parsed");

    /// Construct a checkpoint id from a static string.
    ///
    /// Frontends use this to declare checkpoints the compiler core does not know about.
    pub const fn new(id: &'static str) -> Self {
        Self(id)
    }

    /// The stable string form of this checkpoint id.
    pub const fn as_str(&self) -> &'static str {
        self.0
    }
}

impl fmt::Display for CheckpointId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checkpoint_ids_are_stable_strings() {
        assert_eq!(CheckpointId::HIR_INITIAL.as_str(), "hir.initial");
        assert_eq!(CheckpointId::HIR_TRANSFORMED.as_str(), "hir.transformed");
        assert_eq!(CheckpointId::MASM_LOWERED.as_str(), "masm.lowered");
        assert_eq!(CheckpointId::PACKAGE_ASSEMBLED.as_str(), "package.assembled");
    }

    #[test]
    fn checkpoint_ids_display_as_their_string_form() {
        assert_eq!(alloc::format!("{}", CheckpointId::HIR_INITIAL), "hir.initial");
    }

    #[test]
    fn checkpoint_ids_compare_by_identity_not_order() {
        assert_eq!(CheckpointId::HIR_INITIAL, CheckpointId::HIR_INITIAL);
        assert_ne!(CheckpointId::HIR_INITIAL, CheckpointId::HIR_TRANSFORMED);
    }
}
