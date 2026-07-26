use alloc::{format, rc::Rc, sync::Arc, vec, vec::Vec};
use core::{any::Any, cell::RefCell};

use miden_assembly::{ProjectSourceInputs, ProjectSourceProvenanceInputs, TargetAssemblyContext};
use miden_mast_package::Package as MastPackage;
use midenc_session::{PackageId, Session, diagnostics::Report, miden_project::TargetType};

use super::{
    Artifact, ArtifactId, CheckpointId, Flow, Goal, Observer, Outcome, Stopped, TargetRole,
};
use crate::CompilerResult;

/// Uniquely identifies a target within a compilation request.
///
/// Frontend state must be keyed by this, never by package identity alone: a package with
/// both a library and an executable target has one `PackageId` for two targets, and
/// `Package::target_package_name` only disambiguates them for executables.
///
/// This is usable as both a `HashMap` and a `BTreeMap` key. The target type is stored as
/// its `#[repr(u8)]` discriminant rather than as a [`TargetType`], because `TargetType`
/// implements neither `Hash` nor `Ord`, and being `#[non_exhaustive]` it cannot be mapped
/// to a local enum by a wildcard-free match. The discriminant conversion is total and
/// injective, so a variant added upstream gets a distinct key rather than colliding with
/// an existing one.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TargetKey {
    /// The identity of the package the target belongs to.
    package: PackageId,
    /// The effective name of the target within that package.
    name: Arc<str>,
    /// The discriminant of the target's [`TargetType`].
    ty: u8,
}

impl TargetKey {
    /// Construct a key for the target named `name` of type `ty` in package `package`.
    pub fn new(package: PackageId, name: Arc<str>, ty: TargetType) -> Self {
        Self {
            package,
            name,
            ty: u8::from(ty),
        }
    }

    /// The identity of the package this target belongs to.
    pub fn package(&self) -> &PackageId {
        &self.package
    }

    /// The effective name of this target within its package.
    pub fn name(&self) -> &Arc<str> {
        &self.name
    }

    /// The type of this target.
    ///
    /// Returns `None` only if the key was built from a [`TargetType`] variant that this
    /// build of `miden-mast-package` cannot reconstruct, which cannot happen for keys
    /// produced by [`TargetKey::new`].
    pub fn target_type(&self) -> Option<TargetType> {
        TargetType::try_from(self.ty).ok()
    }
}

/// Holds the single owned artifact captured when a target reaches its goal.
#[derive(Debug, Default)]
pub struct CaptureSlot {
    captured: Option<Outcome>,
}

impl CaptureSlot {
    /// Returns true if nothing has been captured.
    pub fn is_empty(&self) -> bool {
        self.captured.is_none()
    }

    /// Store the artifact produced at `checkpoint`.
    ///
    /// Capturing twice is an internal invariant violation: only the root target is given a
    /// goal short of assembly, and it stops the moment it reaches it.
    pub fn put(&mut self, checkpoint: CheckpointId, artifact: Artifact) -> CompilerResult<()> {
        if let Some(existing) = self.captured.as_ref() {
            return Err(Report::msg(format!(
                "internal error: artifact for '{checkpoint}' cannot be captured, an artifact for \
                 '{}' was already captured for this request",
                existing.checkpoint()
            )));
        }
        self.captured = Some(Outcome::new(checkpoint, artifact));
        Ok(())
    }

    /// Take the captured outcome, leaving the slot empty.
    pub fn take(&mut self) -> Option<Outcome> {
        self.captured.take()
    }
}

/// Everything a frontend needs to compile one target.
pub struct TargetContext<'a> {
    assembly: &'a TargetAssemblyContext<'a>,
    // TODO(increment-3): replace with a per-target Session built from
    // TargetAssemblyContext::package, per the Option scoping section of the spec.
    session: Rc<Session>,
    role: TargetRole,
    goal: Goal,
    observers: Vec<Rc<RefCell<dyn Observer>>>,
    capture: Rc<RefCell<CaptureSlot>>,
}

impl<'a> TargetContext<'a> {
    /// Construct a context with a single observer, for use in tests.
    pub fn for_testing(
        assembly: &'a TargetAssemblyContext<'a>,
        session: Rc<Session>,
        role: TargetRole,
        goal: Goal,
        observer: Rc<RefCell<dyn Observer>>,
        capture: Rc<RefCell<CaptureSlot>>,
    ) -> Self {
        Self {
            assembly,
            session,
            role,
            goal,
            observers: vec![observer],
            capture,
        }
    }

    /// The assembler-provided context: dependency graph, target, paths, package registry.
    pub fn assembly(&self) -> &'a TargetAssemblyContext<'a> {
        self.assembly
    }

    /// The session for this target.
    ///
    /// In increment 1 this is lifted from the HIR `Context` the caller supplied.
    /// Increment 3 replaces it with a per-target session.
    pub fn session(&self) -> Rc<Session> {
        self.session.clone()
    }

    /// This target's role in the overall compilation.
    pub fn role(&self) -> TargetRole {
        self.role
    }

    /// Returns true if this target belongs to a synthesized project with no manifest.
    ///
    /// Frontends must use this rather than inspecting `assembly().manifest_path`, which is
    /// an empty sentinel for virtual projects. The condition matches the one the
    /// dependency graph uses to classify a node as `ProjectSource::Virtual`.
    pub fn is_virtual_project(&self) -> bool {
        self.assembly.package.manifest_path().is_none()
    }

    /// The key under which per-target frontend state must be stored.
    ///
    /// The returned key is `Hash` and `Ord`, so it can key either a `HashMap` or a
    /// `BTreeMap`.
    pub fn target_key(&self) -> TargetKey {
        TargetKey::new(
            self.assembly.package.name().into_inner(),
            self.assembly.target.name.inner().clone(),
            self.assembly.target.ty,
        )
    }

    /// Publish `artifact` at `checkpoint`.
    ///
    /// Notifies observers, then compares `checkpoint` against this target's goal. On a
    /// match the artifact is captured and [`Flow::Stop`] is returned; otherwise the
    /// artifact is handed back via [`Flow::Continue`].
    ///
    /// Rendering for `--emit` is added in increment 3, where the resolved output request
    /// is available.
    pub fn checkpoint<T: Any>(
        &self,
        checkpoint: CheckpointId,
        id: ArtifactId,
        artifact: T,
    ) -> CompilerResult<Flow<T>> {
        let artifact = Artifact::new(id, artifact);

        for observer in &self.observers {
            observer.borrow_mut().on_checkpoint(checkpoint, self.role, &artifact);
        }

        if checkpoint != self.goal.checkpoint() {
            let value = artifact.downcast::<T>().map_err(|artifact| {
                Report::msg(format!(
                    "internal error: artifact at '{checkpoint}' changed type while being \
                     published (tagged '{}')",
                    artifact.id()
                ))
            })?;
            return Ok(Flow::Continue(value));
        }

        self.capture.borrow_mut().put(checkpoint, artifact)?;
        Ok(Flow::Stop(Stopped::new(checkpoint)))
    }
}

/// A language frontend: turns a target's sources into assembly-ready Miden Assembly.
///
/// Frontends run imperatively and publish artifacts through
/// [`TargetContext::checkpoint`] as they go.
pub trait Frontend {
    /// Compile this target to assembly-ready Miden Assembly, or stop at the goal.
    fn compile(&self, cx: &TargetContext<'_>) -> CompilerResult<Flow<ProjectSourceInputs>>;

    /// Collect the source inputs that determine this target's build provenance.
    ///
    /// This must not depend on assembled dependencies, must not publish checkpoints, and
    /// must never stop. It is called before `compile` for dependency targets, and
    /// repeatedly while hashing the dependency closure, so implementations must memoize by
    /// [`TargetContext::target_key`], whose [`TargetKey`] can key either a `HashMap` or a
    /// `BTreeMap`.
    fn provenance(&self, cx: &TargetContext<'_>) -> CompilerResult<ProjectSourceProvenanceInputs>;

    /// Apply language-specific post-processing to the assembled package.
    ///
    /// The assembler builds a *fresh* context for this call, so any data carried over from
    /// `compile` must be retrieved by [`TargetContext::target_key`].
    fn post_process(
        &self,
        _package: &mut MastPackage,
        _cx: &TargetContext<'_>,
    ) -> CompilerResult<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use alloc::{format, rc::Rc};
    use core::cell::RefCell;

    use midenc_session::miden_project::TargetType;

    use super::*;
    use crate::pipeline::{Goal, RecordingObserver, testing::VirtualProject};

    #[derive(Debug, PartialEq)]
    struct Payload(u32);

    fn wat_fixture(name: &str, file: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join("midenc-pipeline-fixtures").join(name);
        std::fs::create_dir_all(&dir).expect("should create fixture dir");
        let root = dir.join(file);
        std::fs::write(&root, "(module)").expect("should write fixture source");
        root
    }

    fn library(name: &str) -> VirtualProject {
        let root = wat_fixture(name, "lib.wat");
        VirtualProject::new(name, &root, TargetType::Library).expect("should build")
    }

    /// A default session, lifted from a default HIR context.
    ///
    /// Increment 3 replaces this with a per-target session built from the callback package.
    fn session() -> Rc<midenc_session::Session> {
        midenc_hir::Context::default().session_rc()
    }

    #[test]
    fn checkpoint_before_the_goal_returns_the_artifact() {
        let project = library("ctx_continue");
        let assembly = project.assembly_context().expect("assembly context");
        let capture = Rc::new(RefCell::new(CaptureSlot::default()));
        let observer = Rc::new(RefCell::new(RecordingObserver::default()));

        let cx = TargetContext::for_testing(
            &assembly,
            session(),
            TargetRole::Root,
            Goal::at(CheckpointId::MASM_LOWERED),
            observer.clone(),
            capture.clone(),
        );

        match cx.checkpoint(CheckpointId::HIR_INITIAL, ArtifactId::HIR, Payload(1)).unwrap() {
            Flow::Continue(payload) => assert_eq!(payload, Payload(1)),
            Flow::Stop(_) => panic!("goal is masm.lowered, should not stop at hir.initial"),
        }
        assert!(capture.borrow().is_empty(), "nothing captured before the goal");
        assert_eq!(observer.borrow().records(), &[(CheckpointId::HIR_INITIAL, TargetRole::Root)]);
    }

    #[test]
    fn checkpoint_at_the_goal_captures_and_stops() {
        let project = library("ctx_stop");
        let assembly = project.assembly_context().expect("assembly context");
        let capture = Rc::new(RefCell::new(CaptureSlot::default()));
        let observer = Rc::new(RefCell::new(RecordingObserver::default()));

        let cx = TargetContext::for_testing(
            &assembly,
            session(),
            TargetRole::Root,
            Goal::at(CheckpointId::HIR_INITIAL),
            observer,
            capture.clone(),
        );

        match cx.checkpoint(CheckpointId::HIR_INITIAL, ArtifactId::HIR, Payload(9)).unwrap() {
            Flow::Stop(stopped) => assert_eq!(stopped.checkpoint(), CheckpointId::HIR_INITIAL),
            Flow::Continue(_) => panic!("expected to stop at the goal"),
        }

        let captured = capture.borrow_mut().take().expect("artifact should be captured");
        assert_eq!(captured.checkpoint(), CheckpointId::HIR_INITIAL);
        assert_eq!(captured.downcast::<Payload>().unwrap(), Payload(9));
    }

    #[test]
    fn a_second_capture_is_an_invariant_error() {
        let mut slot = CaptureSlot::default();
        slot.put(CheckpointId::HIR_INITIAL, Artifact::new(ArtifactId::HIR, Payload(1)))
            .expect("first capture should succeed");
        let err = slot
            .put(CheckpointId::HIR_TRANSFORMED, Artifact::new(ArtifactId::HIR, Payload(2)))
            .expect_err("second capture must fail");
        assert!(format!("{err}").contains("already captured"));
    }

    #[test]
    fn target_key_distinguishes_targets_of_one_package() {
        let lib_root = wat_fixture("ctx_key_lib", "lib.wat");
        let exe_root = wat_fixture("ctx_key_exe", "main.wat");
        let lib = VirtualProject::new("shared", &lib_root, TargetType::Library).expect("lib");
        let exe = VirtualProject::new("shared", &exe_root, TargetType::Executable).expect("exe");

        let lib_cx = lib.assembly_context().expect("lib ctx");
        let exe_cx = exe.assembly_context().expect("exe ctx");
        let capture = Rc::new(RefCell::new(CaptureSlot::default()));
        let observer = Rc::new(RefCell::new(RecordingObserver::default()));

        let lib_target = TargetContext::for_testing(
            &lib_cx,
            session(),
            TargetRole::RequiredLibrary,
            Goal::at(CheckpointId::PACKAGE_ASSEMBLED),
            observer.clone(),
            capture.clone(),
        );
        let exe_target = TargetContext::for_testing(
            &exe_cx,
            session(),
            TargetRole::Root,
            Goal::at(CheckpointId::PACKAGE_ASSEMBLED),
            observer,
            capture,
        );

        assert_ne!(
            lib_target.target_key(),
            exe_target.target_key(),
            "lib and exe targets of one package must not share frontend state"
        );

        // The two keys above also differ in their `name` component, because
        // `VirtualProject` routes libraries through `Target::library`, which derives the
        // name from the absolutized namespace (`::shared`) rather than the bare `shared`
        // an executable gets. So pin the target-type component directly: two keys that
        // agree on both package id and name must still be distinguished by target type.
        let package = lib_target.target_key().package().clone();
        let name: Arc<str> = Arc::from("shared");
        let as_library = TargetKey::new(package.clone(), name.clone(), TargetType::Library);
        let as_executable = TargetKey::new(package.clone(), name.clone(), TargetType::Executable);

        assert_eq!(as_library.package(), as_executable.package(), "same package id");
        assert_eq!(as_library.name(), as_executable.name(), "same target name");
        assert_ne!(
            as_library, as_executable,
            "keys differing only in target type must not be equal"
        );
        assert_eq!(as_library.target_type(), Some(TargetType::Library));
        assert_eq!(as_executable.target_type(), Some(TargetType::Executable));
    }

    #[test]
    fn target_key_can_key_both_map_flavors() {
        use std::collections::{BTreeMap, HashMap};

        let package = PackageId::from("shared");
        let name: Arc<str> = Arc::from("shared");
        let as_library = TargetKey::new(package.clone(), name.clone(), TargetType::Library);
        let as_executable = TargetKey::new(package.clone(), name.clone(), TargetType::Executable);

        // `BTreeMap` requires `Ord`.
        let mut ordered = BTreeMap::new();
        ordered.insert(as_library.clone(), "lib state");
        ordered.insert(as_executable.clone(), "exe state");
        assert_eq!(ordered.len(), 2, "both targets must coexist in a BTreeMap");
        assert_eq!(ordered.get(&as_library), Some(&"lib state"));
        assert_eq!(ordered.get(&as_executable), Some(&"exe state"));

        // `HashMap` requires `Hash` + `Eq`.
        let mut hashed = HashMap::new();
        hashed.insert(as_library.clone(), "lib state");
        hashed.insert(as_executable.clone(), "exe state");
        assert_eq!(hashed.len(), 2, "both targets must coexist in a HashMap");
        assert_eq!(hashed.get(&as_library), Some(&"lib state"));
        assert_eq!(hashed.get(&as_executable), Some(&"exe state"));

        // Memoization requires a freshly built, equal key to hit the same entry.
        let rebuilt = TargetKey::new(package, name, TargetType::Library);
        assert_eq!(hashed.get(&rebuilt), Some(&"lib state"), "equal keys must hash alike");
        assert_eq!(ordered.get(&rebuilt), Some(&"lib state"), "equal keys must order alike");
    }

    #[test]
    fn target_key_distinguishes_every_target_type() {
        use std::collections::HashSet;

        let package = PackageId::from("shared");
        let name: Arc<str> = Arc::from("shared");
        let types = [
            TargetType::Library,
            TargetType::Executable,
            TargetType::Kernel,
            TargetType::AccountComponent,
            TargetType::Note,
            TargetType::TransactionScript,
        ];

        let keys = types
            .iter()
            .map(|ty| TargetKey::new(package.clone(), name.clone(), *ty))
            .collect::<HashSet<_>>();

        assert_eq!(keys.len(), types.len(), "every target type must produce a distinct key");
    }

    #[test]
    fn a_virtual_project_is_reported_as_virtual() {
        let project = library("ctx_virtual");
        let assembly = project.assembly_context().expect("assembly context");
        let cx = TargetContext::for_testing(
            &assembly,
            session(),
            TargetRole::Root,
            Goal::at(CheckpointId::PACKAGE_ASSEMBLED),
            Rc::new(RefCell::new(RecordingObserver::default())),
            Rc::new(RefCell::new(CaptureSlot::default())),
        );
        assert!(cx.is_virtual_project());
    }
}
