use core::fmt;

/// Identifies a point in a frontend's route at which an artifact exists.
///
/// Checkpoints are namespaced by the artifact family they produce, e.g. `hir.initial`.
/// They carry no global ordering: ordering is defined only within a single frontend's
/// declared route. See [`crate::pipeline::FrontendRegistration::route`].
///
/// # Equality is by content
///
/// Two ids naming the same string are the same checkpoint, whether or not they were
/// constructed from the same `&'static str`. Frontends rely on this: a checkpoint they
/// build with [`CheckpointId::new`] must compare equal to the one the core declares as a
/// constant, and to one parsed out of a `--stop-after` value. Comparing the pointers
/// rather than the strings would be faster and silently wrong.
///
/// # `Ord` is not route order
///
/// The derived [`Ord`] exists only so checkpoints can key a `BTreeMap`/`BTreeSet`; it is
/// the lexicographic order of the underlying strings and has no relation to the order in
/// which a route reaches them. `hir.analyzed < hir.initial` lexicographically even though
/// the wasm route reaches `hir.initial` first. To ask which of two checkpoints comes
/// first, use [`FrontendRegistration::position`](crate::pipeline::FrontendRegistration::position)
/// against the route that declares them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CheckpointId(&'static str);

impl CheckpointId {
    /// Every dependency of a consumer project resolved, assembled, and published into the
    /// package cache, together with the compiler-recorded resolution — before the consumer
    /// itself is built.
    pub const DEPENDENCIES_STAGED: Self = Self("dependencies.staged");
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
    fn checkpoint_ids_are_equal_exactly_when_they_name_the_same_checkpoint() {
        assert_eq!(CheckpointId::HIR_INITIAL, CheckpointId::HIR_INITIAL);
        assert_ne!(CheckpointId::HIR_INITIAL, CheckpointId::HIR_TRANSFORMED);
    }

    #[test]
    fn a_frontend_constructed_id_equals_the_core_constant_it_names() {
        // Equality is by content, not by the identity of the `&'static str`. Third-party
        // frontends depend on this: an id they build for themselves has to be recognized as
        // the core's checkpoint of the same name.
        assert_eq!(CheckpointId::new("hir.initial"), CheckpointId::HIR_INITIAL);
        assert_ne!(CheckpointId::new("hir.initial"), CheckpointId::HIR_ANALYZED);

        // The literals above may be merged into one static by the compiler, so on their own
        // they would not notice an "optimization" to pointer comparison. This id is built at
        // runtime and therefore cannot share a pointer with the constant's.
        let built: &'static str = alloc::string::String::from("hir.initial").leak();
        assert_eq!(
            CheckpointId::new(built),
            CheckpointId::HIR_INITIAL,
            "equality must compare the strings, never the pointers to them"
        );
    }

    #[test]
    fn ord_is_lexicographic_and_must_not_be_read_as_route_order() {
        // Pinned so the doc note stays honest: `Ord` exists only for `BTreeMap` keying.
        // The wasm route reaches `hir.initial` before `hir.analyzed`, yet the ids compare
        // the other way around.
        assert!(CheckpointId::HIR_ANALYZED < CheckpointId::HIR_INITIAL);
    }
}
