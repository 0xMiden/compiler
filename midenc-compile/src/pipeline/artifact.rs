use alloc::boxed::Box;
use core::{any::Any, fmt};

/// Identifies the kind of value carried by an [`Artifact`].
///
/// Artifact ids are a distinct vocabulary from [`midenc_session::OutputType`]; the
/// mapping between them lives in [`crate::pipeline::artifact_id_for_output`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ArtifactId(&'static str);

impl ArtifactId {
    /// The staged dependency exchange of a consumer project: the package-cache directory
    /// holding every resolved dependency package and the compiler-recorded resolution.
    pub const DEPENDENCIES: Self = Self("dependencies");
    /// Miden IR.
    pub const HIR: Self = Self("hir");
    /// Miden Assembly.
    pub const MASM: Self = Self("masm");
    /// An assembled MAST package.
    pub const PACKAGE: Self = Self("package");
    /// A WebAssembly module.
    pub const WASM: Self = Self("wasm");

    /// Construct an artifact id from a static string.
    pub const fn new(id: &'static str) -> Self {
        Self(id)
    }

    /// The stable string form of this artifact id.
    pub const fn as_str(&self) -> &'static str {
        self.0
    }
}

impl fmt::Display for ArtifactId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

/// An owned compilation artifact, tagged with a stable [`ArtifactId`].
///
/// The envelope is type-erased so the pipeline core never enumerates frontend artifact
/// types. Consumers recover the concrete value with [`Artifact::downcast`].
pub struct Artifact {
    id: ArtifactId,
    value: Box<dyn Any>,
}

impl Artifact {
    /// Wrap `value` as the artifact identified by `id`.
    pub fn new<T: Any>(id: ArtifactId, value: T) -> Self {
        Self {
            id,
            value: Box::new(value),
        }
    }

    /// The id this artifact was tagged with.
    pub fn id(&self) -> ArtifactId {
        self.id
    }

    /// Recover the concrete value, returning the artifact unchanged if `T` does not match.
    pub fn downcast<T: Any>(self) -> Result<T, Self> {
        let Self { id, value } = self;
        match value.downcast::<T>() {
            Ok(value) => Ok(*value),
            Err(value) => Err(Self { id, value }),
        }
    }

    /// Borrow the concrete value, if `T` matches.
    pub fn downcast_ref<T: Any>(&self) -> Option<&T> {
        self.value.downcast_ref::<T>()
    }
}

impl fmt::Debug for Artifact {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Artifact").field("id", &self.id).finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, PartialEq)]
    struct Hir(u32);

    #[derive(Debug, PartialEq)]
    struct Masm(&'static str);

    #[test]
    fn artifact_preserves_its_id() {
        let artifact = Artifact::new(ArtifactId::HIR, Hir(7));
        assert_eq!(artifact.id(), ArtifactId::HIR);
        assert_eq!(ArtifactId::HIR.as_str(), "hir");
    }

    #[test]
    fn artifact_ids_are_stable_strings() {
        // These strings are user-visible: they are what `--emit=<name>` names, so changing
        // one is a change to the CLI surface rather than an internal rename.
        assert_eq!(ArtifactId::WASM.as_str(), "wasm");
        assert_eq!(ArtifactId::HIR.as_str(), "hir");
        assert_eq!(ArtifactId::MASM.as_str(), "masm");
        assert_eq!(ArtifactId::PACKAGE.as_str(), "package");
    }

    #[test]
    fn artifact_ids_display_as_their_string_form() {
        assert_eq!(alloc::format!("{}", ArtifactId::PACKAGE), "package");
    }

    #[test]
    fn a_frontend_constructed_id_equals_the_core_constant_it_names() {
        // Equality is by content, so a frontend that declares its own artifact id for one
        // of the core's families is recognized as naming that family.
        assert_eq!(ArtifactId::new("masm"), ArtifactId::MASM);
        assert_ne!(ArtifactId::new("masm"), ArtifactId::HIR);
        // And an id no core constant names stays distinct from all of them.
        let foreign = ArtifactId::new("synthetic");
        assert_eq!(foreign.as_str(), "synthetic");
        for known in [ArtifactId::WASM, ArtifactId::HIR, ArtifactId::MASM, ArtifactId::PACKAGE] {
            assert_ne!(foreign, known);
        }
    }

    #[test]
    fn downcast_returns_the_owned_value_on_a_type_match() {
        let artifact = Artifact::new(ArtifactId::HIR, Hir(7));
        assert_eq!(artifact.downcast::<Hir>().expect("should downcast"), Hir(7));
    }

    #[test]
    fn downcast_returns_the_artifact_unchanged_on_a_type_mismatch() {
        let artifact = Artifact::new(ArtifactId::HIR, Hir(7));
        let returned = artifact.downcast::<Masm>().expect_err("should not downcast");
        assert_eq!(returned.id(), ArtifactId::HIR);
        // The value survives the failed downcast and can still be recovered.
        assert_eq!(returned.downcast::<Hir>().expect("should downcast"), Hir(7));
    }

    #[test]
    fn downcast_ref_borrows_without_consuming() {
        let artifact = Artifact::new(ArtifactId::MASM, Masm("begin end"));
        assert_eq!(artifact.downcast_ref::<Masm>(), Some(&Masm("begin end")));
        assert_eq!(artifact.downcast_ref::<Hir>(), None);
        assert_eq!(artifact.id(), ArtifactId::MASM);
    }
}
