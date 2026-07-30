use alloc::{format, rc::Rc, sync::Arc};
use core::any::Any;

use miden_assembly::{ProjectSourceInputs, ProjectSourceProvenanceInputs, TargetAssemblyContext};
use miden_mast_package::Package as MastPackage;
use midenc_hir::Context;
use midenc_session::{
    InputFile, PackageId, Session, diagnostics::Report, miden_project::TargetType,
};

use super::{Artifact, ArtifactId, CheckpointId, Flow, Outcome, RequestState, Stopped, TargetRole};
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
    context: Rc<Context>,
    /// The original compiler input, when one backs this target.
    ///
    /// [`TargetAssemblyContext`] exposes only paths, but stdin-backed inputs live in memory
    /// as [`midenc_session::InputType::Stdin`] and have no path at all. Frontends that must
    /// read the raw input bytes go through here.
    input: Option<&'a InputFile>,
    role: TargetRole,
    /// The goal, observers and capture slot of the request this target belongs to.
    ///
    /// These are identical for every target of one request, so they are owned once by the
    /// caller driving the request and borrowed by each per-target context.
    state: &'a RequestState,
}

impl<'a> TargetContext<'a> {
    /// Construct a context for the target described by `assembly`.
    ///
    /// `context` is the HIR context the frontend builds into, and also the source of this
    /// target's [`Session`]. `input` is the original compiler input, if one backs this
    /// target; see the [`TargetContext::input`] accessor for why paths are not enough.
    /// `state` is the request-scoped state every target of this request shares.
    pub fn new(
        assembly: &'a TargetAssemblyContext<'a>,
        context: Rc<Context>,
        input: Option<&'a InputFile>,
        role: TargetRole,
        state: &'a RequestState,
    ) -> Self {
        Self {
            assembly,
            context,
            input,
            role,
            state,
        }
    }

    /// Construct a context with no input, for use in tests.
    pub fn for_testing(
        assembly: &'a TargetAssemblyContext<'a>,
        context: Rc<Context>,
        role: TargetRole,
        state: &'a RequestState,
    ) -> Self {
        Self::new(assembly, context, None, role, state)
    }

    /// The assembler-provided context: dependency graph, target, paths, package registry.
    pub fn assembly(&self) -> &'a TargetAssemblyContext<'a> {
        self.assembly
    }

    /// The HIR context this target's IR must be built into.
    pub fn context(&self) -> Rc<Context> {
        self.context.clone()
    }

    /// The original compiler input backing this target, if there is one.
    ///
    /// Frontends that need the raw input bytes must use this rather than the paths on
    /// [`TargetContext::assembly`]: a stdin-backed input exists only in memory and has no
    /// path to read it back from.
    pub fn input(&self) -> Option<&'a InputFile> {
        self.input
    }

    /// The session for this target.
    ///
    /// This is the session the HIR context owns, so the two can never disagree. The spec's
    /// Option scoping section gives each target its own `Options` and its own
    /// [`Context`], and a `Context` cannot exist without a session, so per-target sessions
    /// arrive via the per-target context rather than by adding a second field here.
    pub fn session(&self) -> Rc<Session> {
        self.context.session_rc()
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
    /// Notifies observers, then, **for the root target only**, compares `checkpoint`
    /// against this target's goal. On a match the artifact is captured and [`Flow::Break`]
    /// is returned; otherwise — and always for a non-root target — the artifact is handed
    /// back via [`Flow::Continue`].
    ///
    /// Only [`TargetRole::Root`] can stop. It alone is given the caller's goal, while every
    /// other role is compiled to completion regardless of what was asked for, so a non-root
    /// target's checkpoints are observable but never terminal: this always returns
    /// [`Flow::Continue`] for them, never captures, and never returns [`Flow::Break`], even
    /// when `checkpoint` equals the goal it was assigned.
    ///
    /// That matters because a request has exactly *one* goal, shared by every target it
    /// builds: it is held by the [`RequestState`] each per-target context borrows, and
    /// nothing assigns a goal per target. So under `--stop-after` a dependency's goal is the
    /// root's goal — short of `package.assembled`, and perfectly reachable on the
    /// dependency's own route — and the role check here is the only thing keeping that
    /// dependency from stopping at it and capturing an artifact that is not the one the
    /// caller asked for. Without `--stop-after` the shared goal is `package.assembled`, the
    /// very checkpoint the driver publishes uniformly for every target once its package is
    /// assembled; treating that as terminal for a dependency would force callers to
    /// special-case non-root notification, so continuing by construction keeps observers and
    /// driver uniform across roles there too.
    ///
    /// Rendering for `--emit` does not happen here. The driver attaches an `EmitObserver`
    /// (`pipeline/driver.rs`) which renders the root target's artifacts through the route's
    /// own [`ArtifactDecl::render`](super::ArtifactDecl) as each checkpoint is reached. It is
    /// deliberately not gated on the request's resolved outputs: [`Session::emit`] consults
    /// `should_emit` itself, so the observer renders unconditionally and a run that asked for
    /// nothing simply writes nothing.
    pub fn checkpoint<T: Any>(
        &self,
        checkpoint: CheckpointId,
        id: ArtifactId,
        artifact: T,
    ) -> CompilerResult<Flow<T>> {
        let artifact = Artifact::new(id, artifact);

        self.state.notify(checkpoint, self.role, &artifact);

        if !self.role.is_root() || checkpoint != self.state.goal().checkpoint() {
            let value = artifact.downcast::<T>().map_err(|artifact| {
                Report::msg(format!(
                    "internal error: artifact at '{checkpoint}' changed type while being \
                     published (tagged '{}')",
                    artifact.id()
                ))
            })?;
            return Ok(Flow::Continue(value));
        }

        self.state.capture(checkpoint, artifact)?;
        Ok(Flow::Break(Stopped::new(checkpoint)))
    }

    /// Run the request's pre-assembly hook against `lowered`, if it has one.
    ///
    /// Called by [`backend::hir_to_masm`](super::backend::hir_to_masm) once a target has been
    /// lowered and the run is going on to assemble it; see
    /// [`PreAssemblyHook`](super::PreAssemblyHook) for what it is for and why it is not an
    /// [`Observer`](super::Observer). A frontend that lowers by some other route calls this
    /// itself or does not offer the hook at all — there is no default.
    pub fn pre_assembly(&self, lowered: &super::backend::LoweredTarget) -> CompilerResult<()> {
        self.state.pre_assembly(self.role, lowered)
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
    /// must never stop. It is called before `compile` for dependency targets, and repeatedly
    /// while hashing the dependency closure.
    ///
    /// Because of that repetition, an implementation that *pays* to produce its provenance —
    /// by compiling, invoking a build tool, or otherwise redoing work `compile` already did —
    /// must memoize by [`TargetContext::target_key`], whose [`TargetKey`] can key either a
    /// `HashMap` or a `BTreeMap`. An implementation whose provenance is a cheap re-read of
    /// its own sources need not, and may prefer not to: see
    /// [`MasmProjectFrontend`](super::frontends::MasmProjectFrontend), which delegates to the
    /// assembler's stateless `MasmSourceProvider` precisely so that its cost stays what the
    /// built-in it displaces cost.
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
    use alloc::{format, rc::Rc, vec, vec::Vec};
    use core::cell::RefCell;

    use midenc_session::{InputType, miden_project::TargetType};

    use super::*;
    use crate::pipeline::{
        Goal, Observer, RecordingObserver,
        testing::{VirtualProject, wat_fixture},
    };

    #[derive(Debug, PartialEq)]
    struct Payload(u32);

    fn library(name: &str) -> VirtualProject {
        let root = wat_fixture(name, "lib.wat");
        VirtualProject::new(name, &root, TargetType::Library).expect("should build")
    }

    /// A default HIR context, which is also the source of the target's session.
    fn context() -> Rc<Context> {
        Rc::new(Context::default())
    }

    /// The request state for a run to `goal` that notifies `observer`.
    fn request(goal: Goal, observer: &Rc<RefCell<RecordingObserver>>) -> RequestState {
        RequestState::new(goal, vec![observer.clone() as Rc<RefCell<dyn Observer>>])
    }

    /// The request state for a run to `goal` whose observations are not inspected.
    fn unobserved_request(goal: Goal) -> RequestState {
        RequestState::new(goal, Vec::new())
    }

    #[test]
    fn checkpoint_before_the_goal_returns_the_artifact() {
        let project = library("ctx_continue");
        let assembly = project.assembly_context().expect("assembly context");
        let observer = Rc::new(RefCell::new(RecordingObserver::default()));
        let state = request(Goal::at(CheckpointId::MASM_LOWERED), &observer);

        let cx = TargetContext::for_testing(&assembly, context(), TargetRole::Root, &state);

        match cx.checkpoint(CheckpointId::HIR_INITIAL, ArtifactId::HIR, Payload(1)).unwrap() {
            Flow::Continue(payload) => assert_eq!(payload, Payload(1)),
            Flow::Break(_) => panic!("goal is masm.lowered, should not stop at hir.initial"),
        }
        assert_eq!(observer.borrow().records(), &[(CheckpointId::HIR_INITIAL, TargetRole::Root)]);
        assert!(state.take_outcome().is_none(), "nothing captured before the goal");
    }

    #[test]
    fn checkpoint_at_the_goal_captures_and_stops() {
        let project = library("ctx_stop");
        let assembly = project.assembly_context().expect("assembly context");
        let state = unobserved_request(Goal::at(CheckpointId::HIR_INITIAL));

        let cx = TargetContext::for_testing(&assembly, context(), TargetRole::Root, &state);

        match cx.checkpoint(CheckpointId::HIR_INITIAL, ArtifactId::HIR, Payload(9)).unwrap() {
            Flow::Break(stopped) => assert_eq!(stopped.checkpoint(), CheckpointId::HIR_INITIAL),
            Flow::Continue(_) => panic!("expected to stop at the goal"),
        }

        let captured = state.take_outcome().expect("artifact should be captured");
        assert_eq!(captured.checkpoint(), CheckpointId::HIR_INITIAL);
        assert_eq!(captured.downcast::<Payload>().unwrap(), Payload(9));
    }

    #[test]
    fn a_non_root_target_continues_even_at_its_goal() {
        let project = library("ctx_role_guard");
        let assembly = project.assembly_context().expect("assembly context");

        // One goal is shared by every target of a request, so a dependency's goal is
        // whatever the root asked for. Here that is `PACKAGE_ASSEMBLED`, which is also what
        // the driver publishes for every target once assembly finishes: reaching it must
        // still be non-terminal for a dependency.
        let goal = Goal::at(CheckpointId::PACKAGE_ASSEMBLED);

        let observer = Rc::new(RefCell::new(RecordingObserver::default()));
        let state = request(goal, &observer);
        let cx = TargetContext::for_testing(&assembly, context(), TargetRole::Dependency, &state);

        match cx
            .checkpoint(CheckpointId::PACKAGE_ASSEMBLED, ArtifactId::PACKAGE, Payload(7))
            .expect("a dependency publishing at its goal must not error")
        {
            // Assert the payload, not just the variant: a guard that returned a default
            // value would otherwise pass.
            Flow::Continue(payload) => assert_eq!(payload, Payload(7)),
            Flow::Break(_) => panic!("a non-root target must never stop, even at its goal"),
        }
        assert_eq!(
            observer.borrow().records(),
            &[(CheckpointId::PACKAGE_ASSEMBLED, TargetRole::Dependency)],
            "a non-root target's checkpoints are still observable"
        );
        assert!(state.take_outcome().is_none(), "nothing may be captured for a non-root target");

        // Mirror: the very same goal and checkpoint on the *root* target must still capture
        // and stop, so the behaviour discriminates on role rather than never capturing.
        let root_state = unobserved_request(goal);
        let root = TargetContext::for_testing(&assembly, context(), TargetRole::Root, &root_state);
        match root
            .checkpoint(CheckpointId::PACKAGE_ASSEMBLED, ArtifactId::PACKAGE, Payload(1))
            .expect("the root target may stop at its goal")
        {
            Flow::Break(stopped) => {
                assert_eq!(stopped.checkpoint(), CheckpointId::PACKAGE_ASSEMBLED)
            }
            Flow::Continue(_) => panic!("expected the root target to stop at its goal"),
        }
        let captured = root_state.take_outcome().expect("root capture");
        assert_eq!(captured.downcast::<Payload>().unwrap(), Payload(1));
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
        // One request state for both targets, as a real request has.
        let state = unobserved_request(Goal::at(CheckpointId::PACKAGE_ASSEMBLED));

        let lib_target =
            TargetContext::for_testing(&lib_cx, context(), TargetRole::RequiredLibrary, &state);
        let exe_target = TargetContext::for_testing(&exe_cx, context(), TargetRole::Root, &state);

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
    fn session_is_derived_from_the_hir_context() {
        let project = library("ctx_session");
        let assembly = project.assembly_context().expect("assembly context");
        let context = context();
        let state = unobserved_request(Goal::at(CheckpointId::PACKAGE_ASSEMBLED));
        let cx = TargetContext::new(&assembly, context.clone(), None, TargetRole::Root, &state);

        assert!(Rc::ptr_eq(&cx.context(), &context), "the frontend builds HIR into this context");
        assert!(
            Rc::ptr_eq(&cx.session(), &context.session_rc()),
            "the session must be the one the HIR context owns, not a second one"
        );
        assert!(cx.input().is_none(), "no input was supplied for this target");
    }

    #[test]
    fn the_original_input_is_reachable_including_stdin_bytes() {
        let project = library("ctx_input");
        let assembly = project.assembly_context().expect("assembly context");
        let input = InputFile::from_bytes(b"(module)".to_vec(), "stdin.wat".into())
            .expect("should build a stdin input");
        let state = unobserved_request(Goal::at(CheckpointId::PACKAGE_ASSEMBLED));
        let cx = TargetContext::new(&assembly, context(), Some(&input), TargetRole::Root, &state);

        let reached = cx.input().expect("a frontend must be able to reach the original input");
        assert!(
            reached.as_path().is_none(),
            "a stdin input has no path, so paths alone cannot reach its bytes"
        );
        match &reached.file {
            InputType::Stdin { input, .. } => {
                assert_eq!(input.as_slice(), b"(module)", "the stdin bytes must be readable")
            }
            other => panic!("expected a stdin-backed input, got {other:?}"),
        }
    }

    #[test]
    fn a_virtual_project_is_reported_as_virtual() {
        let project = library("ctx_virtual");
        let assembly = project.assembly_context().expect("assembly context");
        let state = unobserved_request(Goal::at(CheckpointId::PACKAGE_ASSEMBLED));
        let cx = TargetContext::for_testing(&assembly, context(), TargetRole::Root, &state);
        assert!(cx.is_virtual_project());
    }
}
