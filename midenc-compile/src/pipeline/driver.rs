//! The driver: running one compilation request from an input to an [`Outcome`].

use alloc::{boxed::Box, format, rc::Rc, sync::Arc, vec::Vec};
use core::{cell::RefCell, ops::ControlFlow};

use miden_assembly::{
    AssemblyInterrupted, InterruptedTargetRole, ProjectSourceProvider, ProjectTargetSelector,
};
use miden_mast_package::Package as MastPackage;
use miden_package_registry::{PackageCache, PackageId};
use midenc_session::{
    InputFile, Session,
    diagnostics::Report,
    miden_project::{Target, TargetType},
};

use super::{
    Artifact, ArtifactId, CheckpointId, FrontendProvider, FrontendRegistration, FrontendRegistry,
    Observer, Outcome, OutputRequest, PreparedProject, RequestState, RootTarget, TargetRole,
    prepare_project, resolve_goal,
};
use crate::{CompilerResult, CompilerStopped, stages::assemble::prepare_assembler};

/// One compilation, as asked for by a caller.
///
/// The outputs are carried **unresolved**: a [`Goal`](super::Goal) is only meaningful
/// against a frontend's route, and which frontend runs is decided by preparation, from the
/// project's selected target. So the caller supplies the `--emit`/`--stop-after` request it
/// was given, and [`Pipeline::compile`] resolves it once the route is known.
pub struct CompilationRequest {
    /// The session this compilation runs in.
    session: Rc<Session>,
    /// The input to compile: a project manifest locator.
    input: InputFile,
    /// The explicitly requested outputs and optional stop point.
    outputs: OutputRequest,
    /// Observers to notify at every checkpoint of every target.
    observers: Vec<Rc<RefCell<dyn Observer>>>,
}

impl CompilationRequest {
    /// A request to compile `input` within `session`, with no requested outputs and no
    /// observers.
    pub fn new(session: Rc<Session>, input: InputFile) -> Self {
        Self {
            session,
            input,
            outputs: OutputRequest::default(),
            observers: Vec::new(),
        }
    }

    /// Attach `observers`, which are notified in the order given.
    pub fn with_observers(mut self, observers: Vec<Rc<RefCell<dyn Observer>>>) -> Self {
        self.observers = observers;
        self
    }

    /// Request `outputs`, which decide where this compilation stops and what it emits.
    pub fn with_outputs(mut self, outputs: OutputRequest) -> Self {
        self.outputs = outputs;
        self
    }
}

/// Renders the selected target's artifacts as it reaches them, which is what makes `--emit`
/// real for a project build.
///
/// # The session decides what is written, not this observer
///
/// Each renderer travels with its route's own [`ArtifactDecl`](super::ArtifactDecl) and
/// delegates to
/// [`Session::emit`], which consults `should_emit` and resolves file-versus-stdout itself. So
/// this observer renders *unconditionally* at every checkpoint the selected target reaches,
/// and a run that asked for nothing simply writes nothing. Re-deriving "was this requested?"
/// here would mean reading `Options::output_types`, which cannot distinguish an explicit
/// `--emit` from the implicit `masp` that `Options::with_output_types` inserts on every
/// invocation.
///
/// # Why the driver builds it
///
/// An observer is handed only `(checkpoint, role, &Artifact)`, so it needs the
/// [`FrontendRegistration`] to look the checkpoint's declaration up — and which frontend runs
/// is not known until preparation has selected the target. A caller building a
/// [`CompilationRequest`] has neither, so this is appended to the caller's own observers by
/// [`Pipeline::compile`] rather than supplied to it.
struct EmitObserver {
    /// The session whose output configuration decides what is written, and where.
    session: Rc<Session>,
    /// The route whose declarations say how each checkpoint's artifact is rendered.
    ///
    /// By value: [`FrontendRegistration`] is `Copy` and its declarations are `&'static`.
    frontend: FrontendRegistration,
    /// The first render failure; see [`EmitObserver::take_error`].
    error: Option<Report>,
}

impl EmitObserver {
    /// Render `frontend`'s declared artifacts into `session`'s configured destinations.
    fn new(session: Rc<Session>, frontend: FrontendRegistration) -> Self {
        Self {
            session,
            frontend,
            error: None,
        }
    }

    /// Take the first render failure, if one occurred.
    ///
    /// [`Observer::on_checkpoint`] returns `()`, so a failing renderer has nowhere to report
    /// to: the notification happens inside the assembler's source-provider callback, whose
    /// contract is to hand back sources. Propagating from there would mean dressing an
    /// output-writing failure up as a compilation failure of the target being built. The
    /// failure is therefore recorded and surfaced by [`Pipeline::compile`] once assembly
    /// returns — later than the user's `--emit` asked for, but never silent.
    ///
    /// Renders after the first failure are skipped, so one unwritable destination reports
    /// once rather than once per checkpoint.
    ///
    /// # A renderer at the terminal checkpoint would go unread
    ///
    /// [`Pipeline::compile`] reads this *before* [`outcome_of`], which is what publishes
    /// [`CheckpointId::PACKAGE_ASSEMBLED`] — so a failure recorded by a renderer declared at
    /// the terminal checkpoint is recorded after the only read of it and is discarded. That
    /// ordering is deliberate: a build whose requested outputs could not be written must fail
    /// rather than hand back an outcome. Nothing hits the trap today, because every route
    /// declares its `package.assembled` artifact `unrendered` — the assembled package is
    /// written by [`crate::compile`] instead — but a route that gave that checkpoint a real
    /// renderer would need a second read here, after the outcome is mapped.
    fn take_error(&mut self) -> Option<Report> {
        self.error.take()
    }
}

impl Observer for EmitObserver {
    fn on_checkpoint(&mut self, checkpoint: CheckpointId, role: TargetRole, artifact: &Artifact) {
        if !role.is_root() || self.error.is_some() {
            return;
        }
        // Rendering is driven by the declaration, so a checkpoint this route does not declare
        // has nothing to render with. Observers are notified for every target of every
        // frontend, and only the role check above keeps another frontend's checkpoints out of
        // here; this is what keeps that from being the *only* thing that does.
        let Some(decl) = self.frontend.decl_at(checkpoint) else {
            return;
        };
        if let Err(error) = (decl.render)(artifact, &self.session) {
            self.error = Some(error);
        }
    }
}

/// Runs compilation requests against a set of registered frontends.
pub struct Pipeline {
    registry: FrontendRegistry,
}

impl Pipeline {
    /// Construct a pipeline that dispatches to the frontends in `registry`.
    pub fn new(registry: FrontendRegistry) -> Self {
        Self { registry }
    }

    /// Construct a pipeline with every frontend this compiler ships.
    pub fn with_default_frontends() -> CompilerResult<Self> {
        let mut registry = FrontendRegistry::new();
        registry.register(super::frontends::RUST_FRONTEND)?;
        registry.register(super::frontends::MASM_FRONTEND)?;
        Ok(Self::new(registry))
    }

    /// Compile `request`, caching assembled packages in `cache`.
    ///
    /// The project is prepared once — one compiler-side `Project::load` — and the goal is
    /// resolved against the route of the frontend that preparation selected. Assembly then
    /// follows the same sequence the legacy stages use, differing only in that it is driven
    /// through `assemble_interruptible`, so that a request stopping short of assembly is a
    /// success rather than an error.
    pub fn compile(
        &self,
        request: CompilationRequest,
        cache: &mut impl PackageCache,
    ) -> CompilerResult<Outcome> {
        let CompilationRequest {
            session,
            input,
            outputs,
            mut observers,
        } = request;

        let prepared = prepare_project(
            &input,
            &session.options,
            &self.registry,
            session.source_manager.as_ref(),
        )?;
        let goal = resolve_goal(&outputs, &prepared.frontend)?;

        // Appended after the caller's own, so that a caller observing a checkpoint sees it
        // before anything is written for it, and so that a caller cannot displace emission by
        // supplying observers of its own.
        let emitter = Rc::new(RefCell::new(EmitObserver::new(session.clone(), prepared.frontend)));
        observers.push(emitter.clone() as Rc<RefCell<dyn Observer>>);
        let state = Rc::new(RequestState::new(goal, observers));

        let mut assembler = miden_assembly::Assembler::new(session.source_manager.clone())
            .with_warnings_as_errors(session.options.diagnostics.warnings.warnings_as_errors());
        prepare_assembler(&mut assembler, &prepared.package, &session)?;

        let providers = self.providers(&session, &state, &prepared);
        let mut project_assembler =
            assembler.for_project_with_providers(prepared.package.clone(), cache, providers)?;

        // Derived from the target preparation already selected, rather than re-read from
        // `Options`: the selection rule lives in one place, and a second copy here could
        // pick a different target than the one whose role the providers derive against.
        let selector = if prepared.target.ty.is_executable() {
            ProjectTargetSelector::Executable(prepared.target.name.inner().as_ref())
        } else {
            ProjectTargetSelector::Library
        };

        // Bound rather than `?`-ed, because a render failure has to be surfaced on *both*
        // paths out of assembly; see [`prefer_render_error`].
        let assembled = project_assembler.assemble_interruptible(selector, &prepared.profile_name);
        let render_error = emitter.borrow_mut().take_error();

        // Surfaced here rather than from the observer, which cannot report; see
        // [`EmitObserver::take_error`]. Before the outcome is mapped, so that a build whose
        // requested outputs could not be written is a failure rather than a success with
        // missing files.
        let assembled = match assembled {
            Ok(assembled) => assembled,
            Err(error) => return Err(prefer_render_error(error, render_error)),
        };
        if let Some(error) = render_error {
            return Err(error);
        }

        outcome_of(assembled, &state, &prepared.target)
    }

    /// Build one provider per registered extension, sharing one frontend per registration.
    ///
    /// Every registered frontend gets providers, not only the one selected for the root
    /// target: the assembler chooses a provider per target by *that* target's root
    /// extension, so a Rust root with a MASM dependency needs both. Which callback is the
    /// root's is not decided here at all — every provider carries the same [`RootTarget`],
    /// and derives each callback's role from it.
    ///
    /// [`FrontendRegistration::instantiate`](super::FrontendRegistration::instantiate)
    /// returns a fresh instance per call, so it is called once per registration and the
    /// resulting `Rc` is cloned into each of that registration's extensions. Instantiating
    /// per extension instead would split a frontend's per-target memoization across its own
    /// providers.
    fn providers(
        &self,
        session: &Rc<Session>,
        state: &Rc<RequestState>,
        prepared: &PreparedProject,
    ) -> Vec<Box<dyn ProjectSourceProvider>> {
        let root = RootTarget::new(prepared.package.clone(), &prepared.target);
        let mut providers = Vec::new();
        for registration in self.registry.registrations() {
            let frontend = registration.instantiate(session.clone());
            for extension in registration.extensions() {
                providers.push(Box::new(FrontendProvider::new(
                    extension,
                    frontend.clone(),
                    session.clone(),
                    state.clone(),
                    root.clone(),
                )) as Box<dyn ProjectSourceProvider>);
            }
        }
        providers
    }
}

/// Choose which of a failed assembly's two reports to raise.
///
/// [`CompilerStopped`] is the odd one out: it is not a failure at all, but the signal a
/// frontend raises to end the build early — `midenc-driver` downcasts it into `Ok(())` and
/// exits 0 (`midenc-driver/src/lib.rs`). So a recorded render failure carried alongside it
/// would be dropped and the run would report success having written nothing and said nothing.
/// `midenc <manifest> -Zlint -Canalyze-only --emit=masm=<unwritable>` is exactly that shape:
/// `masm.parsed` renders and fails, then the lint stops the build.
///
/// A *genuine* assembler error wins instead. It is the more informative of the two — the
/// render very likely failed because the thing it was handed was never built correctly — and
/// it already fails the run, so nothing is silently lost by preferring it.
fn prefer_render_error(assembly_error: Report, render_error: Option<Report>) -> Report {
    match render_error {
        Some(render_error) if assembly_error.is::<CompilerStopped>() => render_error,
        _ => assembly_error,
    }
}

/// Map the assembler's result onto this request's outcome.
///
/// A completed assembly publishes [`CheckpointId::PACKAGE_ASSEMBLED`] here, because no
/// frontend can: the package is produced by the assembler, after every frontend callback
/// has returned. An interruption is the frontend's stop, and is handled by
/// [`interrupted_outcome`].
///
/// # A completed assembly must have been a full build
///
/// Completion is only a valid outcome for a request whose goal *is* the terminal checkpoint.
/// A request that asked to stop short and nonetheless ran to completion never reached
/// [`TargetContext::checkpoint`](super::TargetContext::checkpoint)'s capturing branch — the
/// target that should have stopped was never classified as the root, or never published the
/// goal at all — so nothing was captured, and returning the package here would hand the
/// caller a full build for a request whose goal was, say, `masm.parsed`. This is the
/// symmetric half of [`interrupted_outcome`]'s checks: those reject a stop that should not
/// have happened, this rejects a completion that should have been a stop. Like them it is an
/// internal-error report, because no command line can produce it: the only caller resolves
/// every project request to the terminal checkpoint.
fn outcome_of(
    assembled: ControlFlow<AssemblyInterrupted, Arc<MastPackage>>,
    state: &RequestState,
    selected: &Target,
) -> CompilerResult<Outcome> {
    match assembled {
        ControlFlow::Continue(package) => {
            let goal = state.goal().checkpoint();
            if goal != CheckpointId::PACKAGE_ASSEMBLED {
                return Err(Report::msg(format!(
                    "internal error: assembly of the target selected for this request, '{}' \
                     (type={}), ran to completion, but the request asked to stop at '{goal}'; the \
                     target that should have stopped was never the root, so no artifact was \
                     captured",
                    selected.name.inner(),
                    selected.ty,
                )));
            }
            let artifact = Artifact::new(ArtifactId::PACKAGE, package);
            state.notify(CheckpointId::PACKAGE_ASSEMBLED, TargetRole::Root, &artifact);
            Ok(Outcome::new(CheckpointId::PACKAGE_ASSEMBLED, artifact))
        }
        // `AssemblyInterrupted` is `#[non_exhaustive]`, hence the `..`.
        ControlFlow::Break(AssemblyInterrupted {
            package,
            target_name,
            target_type,
            role,
            ..
        }) => interrupted_outcome(&package, &target_name, target_type, role, state, selected),
    }
}

/// Recover the artifact a stopped request captured, validating that the stop was the
/// selected target's.
///
/// Both checks are internal-error reports rather than user diagnostics: nothing a user can
/// write reaches them. Only the root target is given a goal short of assembly, and only the
/// root target's publication captures — so an interruption is well-formed exactly when it
/// came from the selected target *and* left an artifact behind.
///
/// The empty-slot check catches a *stop without a capture*: a frontend that returned
/// [`Flow::Stop`](super::Flow::Stop) having never gone through
/// [`TargetContext::checkpoint`](super::TargetContext::checkpoint), which is the only thing
/// that writes the slot. Nothing in tree does that, but nothing prevents it either —
/// [`Stopped::new`](super::Stopped::new) is public, so any frontend can synthesize a stop —
/// and without this check such a stop would end the build early and hand the caller success
/// with no artifact at all. A target wrongly classified as *non-root* does not arrive here:
/// it never stops, so assembly runs to completion and [`outcome_of`]'s own completion guard
/// is what catches it.
///
/// The fields are taken apart rather than passed as an `AssemblyInterrupted`, which is
/// `#[non_exhaustive]` and so cannot be constructed outside the assembler — including by
/// the tests that exercise the roles a real run through these providers cannot produce.
fn interrupted_outcome(
    package: &PackageId,
    target_name: &str,
    target_type: TargetType,
    role: InterruptedTargetRole,
    state: &RequestState,
    selected: &Target,
) -> CompilerResult<Outcome> {
    if role != InterruptedTargetRole::Root {
        return Err(Report::msg(format!(
            "internal error: assembly of package '{package}' was interrupted while building its \
             {role} target '{target_name}' (type={target_type}); only the root target is given a \
             goal short of assembly, so no other role can stop"
        )));
    }

    let expected_name = selected.name.inner().as_ref();
    if target_name != expected_name || target_type != selected.ty {
        return Err(Report::msg(format!(
            "internal error: assembly of package '{package}' was interrupted at the root target \
             '{target_name}' (type={target_type}), but the target selected for this request was \
             '{expected_name}' (type={})",
            selected.ty
        )));
    }

    state.take_outcome().ok_or_else(|| {
        Report::msg(format!(
            "internal error: assembly of the root target '{target_name}' of package '{package}' \
             was interrupted before reaching '{}', but no artifact was captured for the request",
            state.goal().checkpoint()
        ))
    })
}

#[cfg(test)]
mod tests {
    use alloc::{
        string::{String, ToString},
        vec,
    };
    use std::path::{Path, PathBuf};

    use miden_assembly::{
        ModuleParser, ProjectSourceInputs, ProjectSourceProvenanceInputs, SourceFileProvenance,
        ast::ModuleKind,
    };
    use miden_package_registry::NoPackageStore;
    use midenc_session::{
        Options,
        diagnostics::{DefaultSourceManager, SourceManager},
        miden_project::TargetType,
    };

    use super::*;
    use crate::pipeline::{
        ArtifactDecl, Flow, Frontend, FrontendId, FrontendRegistration, Goal, RecordingObserver,
        TargetContext, testing::VirtualProject,
    };

    // -------------------------------------------------------------------------------------
    // A frontend to drive the assembler with.
    // -------------------------------------------------------------------------------------

    /// The extension the fixture frontend claims.
    ///
    /// Deliberately not one of the shipped frontends' extensions. What the driver does is
    /// independent of the language a target is written in, so binding these tests to a
    /// shipped frontend would make them fail for that frontend's reasons; and the one
    /// frontend that could run a whole build in a unit test — MASM — is covered by its own
    /// tests, while the Rust frontend would shell out to cargo.
    const STUB: &str = "stub";

    /// What the fixture frontend publishes at `masm.parsed`: the name of the target it was
    /// invoked for.
    ///
    /// Carrying the target name is what makes a captured artifact identifiable: a driver
    /// that returned the wrong target's capture, or an artifact from the wrong checkpoint,
    /// could not produce this value.
    #[derive(Debug, PartialEq, Eq)]
    struct Parsed(String);

    /// A frontend that publishes twice and then hands back trivial Miden Assembly.
    struct StubFrontend;

    impl StubFrontend {
        /// A trivial module in this target's namespace, standing in for compiled output.
        fn sources(cx: &TargetContext<'_>) -> CompilerResult<ProjectSourceInputs> {
            let namespace = cx.assembly().target.namespace.inner().clone();
            let root = ModuleParser::new(Some(ModuleKind::Library)).parse_str(
                Some(namespace.as_ref()),
                "pub proc main\n    push.1\n    drop\nend\n",
                cx.session().source_manager.clone(),
            )?;
            Ok(ProjectSourceInputs {
                root,
                support: Vec::new(),
            })
        }
    }

    impl Frontend for StubFrontend {
        fn compile(&self, cx: &TargetContext<'_>) -> CompilerResult<Flow<ProjectSourceInputs>> {
            let name = cx.assembly().target.name.inner().to_string();
            if let Flow::Stop(stopped) =
                cx.checkpoint(CheckpointId::MASM_PARSED, ArtifactId::MASM, Parsed(name))?
            {
                return Ok(Flow::Stop(stopped));
            }
            let sources = Self::sources(cx)?;
            cx.checkpoint(CheckpointId::MASM_LOWERED, ArtifactId::MASM, sources)
        }

        fn provenance(
            &self,
            cx: &TargetContext<'_>,
        ) -> CompilerResult<ProjectSourceProvenanceInputs> {
            Ok(ProjectSourceProvenanceInputs {
                root: SourceFileProvenance {
                    path: cx.assembly().resolved_target_root.clone(),
                    content: String::from("stub").into_boxed_str(),
                },
                support: Vec::new(),
            })
        }
    }

    /// Build the fixture frontend.
    fn make_stub(_session: Rc<Session>) -> Rc<dyn Frontend> {
        Rc::new(StubFrontend)
    }

    /// A renderer that writes nothing; the tests using it never emit.
    fn unrendered(_artifact: &Artifact, _session: &Session) -> CompilerResult<()> {
        Ok(())
    }

    // -------------------------------------------------------------------------------------
    // Renderers that record rather than write.
    //
    // What the driver owes the route is that each declared renderer runs, once, for the
    // selected target's checkpoints. Writing real files to check that would test
    // `Session::emit` — which the route's own renderer delegates to, and which
    // `frontends::masm` covers — rather than the wiring. A renderer is a bare `fn` pointer
    // with no state and no reliable address, so recording what it *did* is the only way to
    // tell one from another. Tests each run on their own thread, so the log is per-test.
    // -------------------------------------------------------------------------------------

    std::thread_local! {
        /// The checkpoints whose renderers have run, in order.
        static RENDERED: RefCell<Vec<CheckpointId>> = const { RefCell::new(Vec::new()) };
    }

    /// Note that the renderer declared for `checkpoint` ran.
    fn note_render(checkpoint: CheckpointId) {
        RENDERED.with_borrow_mut(|rendered| rendered.push(checkpoint));
    }

    /// The checkpoints rendered so far, in order.
    fn rendered() -> Vec<CheckpointId> {
        RENDERED.with_borrow(|rendered| rendered.clone())
    }

    fn render_parsed(_artifact: &Artifact, _session: &Session) -> CompilerResult<()> {
        note_render(CheckpointId::MASM_PARSED);
        Ok(())
    }

    fn render_lowered(_artifact: &Artifact, _session: &Session) -> CompilerResult<()> {
        note_render(CheckpointId::MASM_LOWERED);
        Ok(())
    }

    fn render_assembled(_artifact: &Artifact, _session: &Session) -> CompilerResult<()> {
        note_render(CheckpointId::PACKAGE_ASSEMBLED);
        Ok(())
    }

    /// What a failing renderer reports, and the only place this text appears.
    const RENDER_FAILURE: &str = "the fixture renderer refuses to write";

    /// A renderer that records that it ran and then fails.
    fn render_failing(_artifact: &Artifact, _session: &Session) -> CompilerResult<()> {
        note_render(CheckpointId::MASM_PARSED);
        Err(Report::msg(RENDER_FAILURE))
    }

    /// The fixture frontend's registration.
    const STUB_FRONTEND: FrontendRegistration = FrontendRegistration::new(
        FrontendId::new("stub"),
        &[STUB],
        &[
            CheckpointId::MASM_PARSED,
            CheckpointId::MASM_LOWERED,
            CheckpointId::PACKAGE_ASSEMBLED,
        ],
        &[
            ("parse", CheckpointId::MASM_PARSED),
            ("lower", CheckpointId::MASM_LOWERED),
            ("assemble", CheckpointId::PACKAGE_ASSEMBLED),
        ],
        &[
            ArtifactDecl {
                checkpoint: CheckpointId::MASM_PARSED,
                id: ArtifactId::MASM,
                render: render_parsed,
            },
            ArtifactDecl {
                checkpoint: CheckpointId::MASM_LOWERED,
                id: ArtifactId::MASM,
                render: render_lowered,
            },
            ArtifactDecl {
                checkpoint: CheckpointId::PACKAGE_ASSEMBLED,
                id: ArtifactId::PACKAGE,
                render: render_assembled,
            },
        ],
        make_stub,
    );

    /// A frontend that publishes `masm.parsed` and then ends the build with its own error.
    ///
    /// Stands in for [`MasmProjectFrontend`](crate::pipeline::frontends::MasmProjectFrontend)
    /// under `-Zlint -Canalyze-only`, which publishes and then raises [`CompilerStopped`] —
    /// the shape in which a render failure can be lost, since the stop is not a failure.
    struct EndingFrontend(fn() -> Report);

    impl Frontend for EndingFrontend {
        fn compile(&self, cx: &TargetContext<'_>) -> CompilerResult<Flow<ProjectSourceInputs>> {
            let name = cx.assembly().target.name.inner().to_string();
            if let Flow::Stop(stopped) =
                cx.checkpoint(CheckpointId::MASM_PARSED, ArtifactId::MASM, Parsed(name))?
            {
                return Ok(Flow::Stop(stopped));
            }
            Err(self.0())
        }

        fn provenance(
            &self,
            cx: &TargetContext<'_>,
        ) -> CompilerResult<ProjectSourceProvenanceInputs> {
            Ok(ProjectSourceProvenanceInputs {
                root: SourceFileProvenance {
                    path: cx.assembly().resolved_target_root.clone(),
                    content: String::from("stub").into_boxed_str(),
                },
                support: Vec::new(),
            })
        }
    }

    /// Build a frontend that ends the build the way the MASM lint does: a clean stop.
    fn make_clean_stop(_session: Rc<Session>) -> Rc<dyn Frontend> {
        Rc::new(EndingFrontend(|| CompilerStopped("analyze-only").into()))
    }

    /// What a genuinely failing build reports, and the only place this text appears.
    const ASSEMBLY_FAILURE: &str = "the fixture frontend could not compile this target";

    /// Build a frontend that ends the build with a real error.
    fn make_hard_error(_session: Rc<Session>) -> Rc<dyn Frontend> {
        Rc::new(EndingFrontend(|| Report::msg(ASSEMBLY_FAILURE)))
    }

    /// The route the three [`EndingFrontend`] registrations below share.
    ///
    /// They differ only in how they render `masm.parsed` and how they end the build, which is
    /// the pair of axes the tests vary. `package.assembled` is on the route because a
    /// registration must declare its terminal checkpoint, though none of these reaches it.
    const ENDING_ROUTE: &[CheckpointId] =
        &[CheckpointId::MASM_PARSED, CheckpointId::PACKAGE_ASSEMBLED];

    /// Stops cleanly, having failed to render.
    const STOP_AFTER_FAILED_RENDER: FrontendRegistration = FrontendRegistration::new(
        FrontendId::new("stop_after_failed_render"),
        &["stopboom"],
        ENDING_ROUTE,
        &[],
        &[
            ArtifactDecl {
                checkpoint: CheckpointId::MASM_PARSED,
                id: ArtifactId::MASM,
                render: render_failing,
            },
            ArtifactDecl {
                checkpoint: CheckpointId::PACKAGE_ASSEMBLED,
                id: ArtifactId::PACKAGE,
                render: render_assembled,
            },
        ],
        make_clean_stop,
    );

    /// Stops cleanly, having rendered successfully.
    const STOP_AFTER_CLEAN_RENDER: FrontendRegistration = FrontendRegistration::new(
        FrontendId::new("stop_after_clean_render"),
        &["stopok"],
        ENDING_ROUTE,
        &[],
        &[
            ArtifactDecl {
                checkpoint: CheckpointId::MASM_PARSED,
                id: ArtifactId::MASM,
                render: render_parsed,
            },
            ArtifactDecl {
                checkpoint: CheckpointId::PACKAGE_ASSEMBLED,
                id: ArtifactId::PACKAGE,
                render: render_assembled,
            },
        ],
        make_clean_stop,
    );

    /// Fails outright, having also failed to render.
    const FAIL_AFTER_FAILED_RENDER: FrontendRegistration = FrontendRegistration::new(
        FrontendId::new("fail_after_failed_render"),
        &["hardboom"],
        ENDING_ROUTE,
        &[],
        &[
            ArtifactDecl {
                checkpoint: CheckpointId::MASM_PARSED,
                id: ArtifactId::MASM,
                render: render_failing,
            },
            ArtifactDecl {
                checkpoint: CheckpointId::PACKAGE_ASSEMBLED,
                id: ArtifactId::PACKAGE,
                render: render_assembled,
            },
        ],
        make_hard_error,
    );

    /// The extension the failing fixture frontend claims; see [`FAILING_FRONTEND`].
    const BOOM: &str = "boom";

    /// The fixture frontend again, but with a renderer that fails at its first checkpoint.
    ///
    /// A separate registration rather than a flag on [`STUB_FRONTEND`], because a renderer is
    /// a `const` `fn` pointer with nothing to configure.
    const FAILING_FRONTEND: FrontendRegistration = FrontendRegistration::new(
        FrontendId::new("failing"),
        &[BOOM],
        &[
            CheckpointId::MASM_PARSED,
            CheckpointId::MASM_LOWERED,
            CheckpointId::PACKAGE_ASSEMBLED,
        ],
        &[],
        &[
            ArtifactDecl {
                checkpoint: CheckpointId::MASM_PARSED,
                id: ArtifactId::MASM,
                render: render_failing,
            },
            ArtifactDecl {
                checkpoint: CheckpointId::MASM_LOWERED,
                id: ArtifactId::MASM,
                render: render_lowered,
            },
            ArtifactDecl {
                checkpoint: CheckpointId::PACKAGE_ASSEMBLED,
                id: ArtifactId::PACKAGE,
                render: render_assembled,
            },
        ],
        make_stub,
    );

    /// The extension the goal-skipping fixture frontend claims; see [`SKIPS_THE_GOAL`].
    const SKIPGOAL: &str = "skipgoal";

    /// The fixture frontend with its first publication removed.
    ///
    /// `masm.parsed` stays on the route, so `--stop-after=parse` still resolves against it,
    /// but is never published. That is the constructible stand-in for the root target's stop
    /// going missing — a target wrongly classified as non-root would look the same from the
    /// driver's side: the goal is never captured, and assembly runs to completion.
    struct SkippingFrontend;

    impl Frontend for SkippingFrontend {
        fn compile(&self, cx: &TargetContext<'_>) -> CompilerResult<Flow<ProjectSourceInputs>> {
            let sources = StubFrontend::sources(cx)?;
            cx.checkpoint(CheckpointId::MASM_LOWERED, ArtifactId::MASM, sources)
        }

        fn provenance(
            &self,
            cx: &TargetContext<'_>,
        ) -> CompilerResult<ProjectSourceProvenanceInputs> {
            StubFrontend.provenance(cx)
        }
    }

    /// Build the goal-skipping fixture frontend.
    fn make_skipping(_session: Rc<Session>) -> Rc<dyn Frontend> {
        Rc::new(SkippingFrontend)
    }

    /// A registration whose route offers `parse` as a stop point that its frontend never
    /// reaches.
    const SKIPS_THE_GOAL: FrontendRegistration = FrontendRegistration::new(
        FrontendId::new("skips_the_goal"),
        &[SKIPGOAL],
        &[
            CheckpointId::MASM_PARSED,
            CheckpointId::MASM_LOWERED,
            CheckpointId::PACKAGE_ASSEMBLED,
        ],
        &[("parse", CheckpointId::MASM_PARSED)],
        &[
            ArtifactDecl {
                checkpoint: CheckpointId::MASM_PARSED,
                id: ArtifactId::MASM,
                render: unrendered,
            },
            ArtifactDecl {
                checkpoint: CheckpointId::MASM_LOWERED,
                id: ArtifactId::MASM,
                render: unrendered,
            },
            ArtifactDecl {
                checkpoint: CheckpointId::PACKAGE_ASSEMBLED,
                id: ArtifactId::PACKAGE,
                render: unrendered,
            },
        ],
        make_skipping,
    );

    // -------------------------------------------------------------------------------------
    // Fixtures.
    // -------------------------------------------------------------------------------------

    /// The effective name of the manifest's library target.
    const TARGET_NAME: &str = "driver_fixture";

    /// A pipeline that dispatches roots of `registration`'s extensions to it.
    fn pipeline_of(registration: FrontendRegistration) -> Pipeline {
        let mut registry = FrontendRegistry::new();
        registry.register(registration).expect("the fixture frontend should register");
        Pipeline::new(registry)
    }

    /// A pipeline that dispatches `.stub` roots to the fixture frontend.
    fn pipeline() -> Pipeline {
        pipeline_of(STUB_FRONTEND)
    }

    /// Materialize a fixture project rooted at a `.<extension>` file in its own directory,
    /// returning its manifest path.
    fn manifest_rooted_at(dir: &str, extension: &str) -> PathBuf {
        let root = format!("lib.{extension}");
        crate::pipeline::testing::fixture_source(dir, &root, "stub");
        crate::pipeline::testing::fixture_source(
            dir,
            "miden-project.toml",
            &format!(
                r#"
[package]
name = "{TARGET_NAME}"
version = "0.1.0"

[lib]
namespace = "{TARGET_NAME}"
path = "{root}"
"#
            ),
        )
    }

    /// The compiler input naming `manifest`.
    fn input(manifest: &Path) -> InputFile {
        InputFile::from_path(manifest).expect("a manifest is a valid compiler input")
    }

    /// A session opened over the fixture project in `dir`, and that project's manifest path.
    fn session(dir: &str) -> (Rc<Session>, PathBuf) {
        session_rooted_at(dir, STUB)
    }

    /// A session opened over a fixture project rooted at a `.<extension>` file.
    fn session_rooted_at(dir: &str, extension: &str) -> (Rc<Session>, PathBuf) {
        let manifest = manifest_rooted_at(dir, extension);
        let options = Box::new(Options::default()).with_output_types(Default::default(), None);
        let source_manager: Arc<dyn SourceManager + Send + Sync> =
            Arc::new(DefaultSourceManager::default());
        let session = Session::new(input(&manifest), options, None, source_manager)
            .expect("the fixture project should open a session");
        (Rc::new(session), manifest)
    }

    /// A recording observer, ready to attach to a request.
    fn recorder() -> Rc<RefCell<RecordingObserver>> {
        Rc::new(RefCell::new(RecordingObserver::default()))
    }

    /// The trace `observer` recorded.
    fn trace(observer: &Rc<RefCell<RecordingObserver>>) -> Vec<(CheckpointId, TargetRole)> {
        observer.borrow().records().to_vec()
    }

    /// A single-target virtual project, for the mapping tests, which never assemble.
    fn virtual_project(name: &str) -> VirtualProject {
        let root = crate::pipeline::testing::wat_fixture(name, "lib.wat");
        VirtualProject::new(name, &root, TargetType::Library).expect("should build")
    }

    /// A request state holding a captured artifact, as a stopped root target leaves behind.
    ///
    /// The mapping tests below start from a *populated* slot deliberately: a mapping that
    /// ignored the role, or the target identity, and simply handed back whatever was
    /// captured would return `Ok` for every one of them.
    fn state_with_capture() -> RequestState {
        let state = RequestState::new(Goal::at(CheckpointId::MASM_PARSED), Vec::new());
        state
            .capture(
                CheckpointId::MASM_PARSED,
                Artifact::new(ArtifactId::MASM, Parsed("captured".to_string())),
            )
            .expect("the first capture of a request must succeed");
        state
    }

    // -------------------------------------------------------------------------------------
    // The result table.
    // -------------------------------------------------------------------------------------

    #[test]
    fn a_full_build_returns_the_assembled_package() {
        let (session, manifest) = session("driver_full_build");
        let observer = recorder();
        let request = CompilationRequest::new(session, input(&manifest))
            .with_observers(vec![observer.clone() as Rc<RefCell<dyn Observer>>]);

        let outcome = pipeline()
            .compile(request, &mut NoPackageStore)
            .expect("an uncapped request should build a package");

        assert_eq!(outcome.checkpoint(), CheckpointId::PACKAGE_ASSEMBLED);
        assert_eq!(
            trace(&observer),
            vec![
                (CheckpointId::MASM_PARSED, TargetRole::Root),
                (CheckpointId::MASM_LOWERED, TargetRole::Root),
                (CheckpointId::PACKAGE_ASSEMBLED, TargetRole::Root),
            ],
            "the frontend's checkpoints are published as it reaches them, and the driver \
             publishes the terminal one no frontend can"
        );

        let package = outcome.into_package().expect("a full build must yield a package");
        assert_eq!(
            format!("{}", package.name),
            "driver_fixture",
            "the package assembled must be the fixture project's"
        );
    }

    #[test]
    fn stopping_at_the_root_targets_goal_returns_what_it_captured() {
        let (session, manifest) = session("driver_stop_after");
        let observer = recorder();
        let request = CompilationRequest::new(session, input(&manifest))
            .with_observers(vec![observer.clone() as Rc<RefCell<dyn Observer>>])
            .with_outputs(OutputRequest::default().with_stop_after(Some("parse".to_string())));

        let outcome = pipeline()
            .compile(request, &mut NoPackageStore)
            .expect("stopping short of assembly is a success, not an error");

        assert_eq!(outcome.checkpoint(), CheckpointId::MASM_PARSED);
        assert_eq!(outcome.artifact().id(), ArtifactId::MASM);
        assert_eq!(
            trace(&observer),
            vec![(CheckpointId::MASM_PARSED, TargetRole::Root)],
            "no work may happen past the stop point, so neither the frontend's later checkpoint \
             nor the driver's terminal one is reached"
        );
        assert_eq!(
            outcome.downcast::<Parsed>().expect("the captured payload must survive intact"),
            Parsed(TARGET_NAME.to_string()),
            "the artifact returned must be the one the selected target published"
        );
    }

    #[test]
    fn an_interruption_at_a_non_root_role_is_an_invariant_error() {
        // Driven through the mapping rather than a whole build, because these providers
        // cannot produce a non-root interruption: `TargetContext::checkpoint` stops only for
        // the root target. That is precisely why the mapping must reject the case rather
        // than assume it away.
        let project = virtual_project("driver_non_root_break");
        let target = project.target();
        let state = state_with_capture();

        for role in [InterruptedTargetRole::RequiredLibrary, InterruptedTargetRole::Dependency] {
            let err = interrupted_outcome(
                &PackageId::from("driver_fixture"),
                target.name.inner().as_ref(),
                target.ty,
                role,
                &state,
                target,
            )
            .expect_err("only the root target may stop a build");

            let rendered = format!("{err}");
            assert!(
                rendered.contains("internal error"),
                "a non-root stop is a compiler bug, not a user error: {rendered}"
            );
            assert!(
                rendered.contains(&role.to_string()),
                "the report must name the role that stopped: {rendered}"
            );
        }
    }

    #[test]
    fn an_interruption_at_a_target_other_than_the_selected_one_is_an_invariant_error() {
        let project = virtual_project("driver_wrong_target_break");
        let target = project.target();
        let state = state_with_capture();

        // Each mismatch on its own: the name alone, then the type alone. Checking only one
        // axis would let the other through, and an executable and a library of one package
        // can differ in type while sharing everything else the interruption reports.
        let mismatches =
            [("other", target.ty), (target.name.inner().as_ref(), TargetType::Executable)];
        for (name, ty) in mismatches {
            let err = interrupted_outcome(
                &PackageId::from("driver_fixture"),
                name,
                ty,
                InterruptedTargetRole::Root,
                &state,
                target,
            )
            .expect_err("the interrupted target must be the one this request selected");

            let rendered = format!("{err}");
            assert!(rendered.contains("internal error"), "{rendered}");
            assert!(
                rendered.contains(&format!("'{name}' (type={ty})")),
                "the report must name the target the interruption came from: {rendered}"
            );
            assert!(
                rendered.contains(&format!("'{}' (type={})", target.name.inner(), target.ty)),
                "and the target this request selected, or the mismatch is unreadable: {rendered}"
            );
        }
    }

    #[test]
    fn a_root_interruption_that_captured_nothing_is_an_invariant_error() {
        // A stop with nothing captured: the slot is written only by
        // `TargetContext::checkpoint`, but `Stopped::new` is public, so a frontend can end
        // the build early without ever having gone through it. Absent this check the caller
        // would receive success and no artifact.
        let project = virtual_project("driver_empty_capture");
        let target = project.target();
        let state = RequestState::new(Goal::at(CheckpointId::MASM_PARSED), Vec::new());

        let err = interrupted_outcome(
            &PackageId::from("driver_fixture"),
            target.name.inner().as_ref(),
            target.ty,
            InterruptedTargetRole::Root,
            &state,
            target,
        )
        .expect_err("a stop that captured nothing cannot be reported as a success");

        let rendered = format!("{err}");
        assert!(rendered.contains("internal error"), "{rendered}");
        assert!(
            rendered.contains("masm.parsed"),
            "the report must name the goal nothing was captured for: {rendered}"
        );
    }

    #[test]
    fn a_completed_build_for_a_request_that_asked_to_stop_is_an_invariant_error() {
        // The discriminating half first: the same frontend, with nothing asking it to stop,
        // assembles a package. So the failure below is the goal being short of the terminal
        // checkpoint on a run that completed — not this fixture being unable to build.
        let (session, manifest) = session_rooted_at("driver_skips_goal_uncapped", SKIPGOAL);
        let outcome = pipeline_of(SKIPS_THE_GOAL)
            .compile(CompilationRequest::new(session, input(&manifest)), &mut NoPackageStore)
            .expect("this frontend assembles a package when nothing asks it to stop");
        assert_eq!(
            outcome.checkpoint(),
            CheckpointId::PACKAGE_ASSEMBLED,
            "the uncapped run must reach the terminal checkpoint, or the guard below would be \
             firing for the wrong reason"
        );

        let (session, manifest) = session_rooted_at("driver_skips_goal", SKIPGOAL);
        let request = CompilationRequest::new(session, input(&manifest))
            .with_outputs(OutputRequest::default().with_stop_after(Some("parse".to_string())));

        let err = pipeline_of(SKIPS_THE_GOAL)
            .compile(request, &mut NoPackageStore)
            .expect_err("a full package cannot be the outcome of a request that asked to stop");

        let rendered = format!("{err}");
        assert!(
            rendered.contains("internal error"),
            "a goal the root target never stopped at is a compiler bug, not a user error: \
             {rendered}"
        );
        assert!(
            rendered.contains("masm.parsed"),
            "the report must name the goal the run blew past: {rendered}"
        );
        assert!(
            rendered.contains(TARGET_NAME),
            "and the target selected for the request: {rendered}"
        );
    }

    // -------------------------------------------------------------------------------------
    // Rendering.
    // -------------------------------------------------------------------------------------

    #[test]
    fn every_checkpoint_the_selected_target_reaches_is_rendered_exactly_once() {
        let (session, manifest) = session("driver_render_full");
        let observer = recorder();
        let request = CompilationRequest::new(session, input(&manifest))
            .with_observers(vec![observer.clone() as Rc<RefCell<dyn Observer>>]);

        pipeline()
            .compile(request, &mut NoPackageStore)
            .expect("the fixture project should build");

        assert_eq!(
            rendered(),
            vec![
                CheckpointId::MASM_PARSED,
                CheckpointId::MASM_LOWERED,
                CheckpointId::PACKAGE_ASSEMBLED,
            ],
            "each checkpoint's declared renderer must run once, in route order — including the \
             terminal one, which the driver publishes itself"
        );
        assert_eq!(
            trace(&observer),
            vec![
                (CheckpointId::MASM_PARSED, TargetRole::Root),
                (CheckpointId::MASM_LOWERED, TargetRole::Root),
                (CheckpointId::PACKAGE_ASSEMBLED, TargetRole::Root),
            ],
            "the driver appends its own observer, so a caller's must still see everything"
        );
    }

    #[test]
    fn a_request_stopping_short_renders_only_what_it_reached() {
        // The discriminating half of the test above: a renderer table walked eagerly, rather
        // than driven by the checkpoints actually reached, would render all three here too.
        let (session, manifest) = session("driver_render_stop_after");
        let request = CompilationRequest::new(session, input(&manifest))
            .with_outputs(OutputRequest::default().with_stop_after(Some("parse".to_string())));

        pipeline()
            .compile(request, &mut NoPackageStore)
            .expect("stopping short of assembly is a success");

        assert_eq!(
            rendered(),
            vec![CheckpointId::MASM_PARSED],
            "nothing past the stop point exists to render"
        );
    }

    #[test]
    fn only_the_root_targets_artifacts_are_rendered() {
        // Driven through the observer directly: the driver's fixture project has a single
        // target, so a whole build cannot produce a non-root notification. `provider.rs`
        // covers which callback *is* the root; this covers what the emit observer does with
        // the answer.
        let (session, _manifest) = session("driver_render_roles");
        let mut observer = EmitObserver::new(session, STUB_FRONTEND);
        let artifact = Artifact::new(ArtifactId::MASM, Parsed(TARGET_NAME.to_string()));

        for role in [TargetRole::Root, TargetRole::RequiredLibrary, TargetRole::Dependency] {
            observer.on_checkpoint(CheckpointId::MASM_PARSED, role, &artifact);
        }

        assert_eq!(
            rendered(),
            vec![CheckpointId::MASM_PARSED],
            "observers are notified for every target, but only the selected target's artifacts \
             are the ones the user asked to emit"
        );
        assert!(observer.take_error().is_none(), "no renderer here fails");
    }

    #[test]
    fn a_checkpoint_this_route_does_not_declare_renders_nothing() {
        // A root target's checkpoints are its own route's, but the observer is handed a
        // `CheckpointId` and nothing constrains it to that route. Rendering must be driven by
        // the declaration, so an undeclared checkpoint is skipped rather than mapped onto
        // whatever renderer happens to be first.
        let (session, _manifest) = session("driver_render_off_route");
        let mut observer = EmitObserver::new(session, STUB_FRONTEND);
        let artifact = Artifact::new(ArtifactId::HIR, Parsed(TARGET_NAME.to_string()));

        observer.on_checkpoint(CheckpointId::HIR_ANALYZED, TargetRole::Root, &artifact);

        assert!(
            rendered().is_empty(),
            "hir.analyzed is not on the stub route, so it has no declaration to render with"
        );
        assert!(observer.take_error().is_none(), "an undeclared checkpoint is not an error");
    }

    #[test]
    fn a_render_failure_is_reported_after_the_build_it_did_not_interrupt() {
        let (session, manifest) = session_rooted_at("driver_render_failure", BOOM);
        let observer = recorder();
        let request = CompilationRequest::new(session, input(&manifest))
            .with_observers(vec![observer.clone() as Rc<RefCell<dyn Observer>>]);

        let err = pipeline_of(FAILING_FRONTEND)
            .compile(request, &mut NoPackageStore)
            .expect_err("a render failure must not be swallowed");

        let rendered_err = format!("{err}");
        assert!(
            rendered_err.contains(RENDER_FAILURE),
            "the renderer's own report must reach the caller: {rendered_err}"
        );
        assert_eq!(
            trace(&observer),
            vec![
                (CheckpointId::MASM_PARSED, TargetRole::Root),
                (CheckpointId::MASM_LOWERED, TargetRole::Root),
            ],
            "`on_checkpoint` cannot report, so the failure is deferred rather than propagated: \
             the frontend runs past the failing checkpoint and assembly completes. \
             `package.assembled` is absent only because the driver publishes it while mapping the \
             outcome, which the error check precedes."
        );
        assert_eq!(
            rendered(),
            vec![CheckpointId::MASM_PARSED],
            "once a renderer has failed, the rest are skipped rather than repeating the failure"
        );
    }

    /// Compile the `.<extension>`-rooted fixture project with `registration`, and return the
    /// error it must fail with.
    fn compile_expecting_error(
        dir: &str,
        extension: &str,
        registration: FrontendRegistration,
    ) -> Report {
        let (session, manifest) = session_rooted_at(dir, extension);
        let request = CompilationRequest::new(session, input(&manifest));
        match pipeline_of(registration).compile(request, &mut NoPackageStore) {
            Err(err) => err,
            // `Outcome` is `Debug`, but naming it in an `expect_err` message would make the
            // three call sites below read as if the outcome were the interesting part.
            Ok(_) => panic!("the fixture frontend ends every build with an error"),
        }
    }

    #[test]
    fn a_render_failure_survives_a_clean_stop() {
        // The hole a `?` on `assemble_interruptible` leaves. `CompilerStopped` is not a
        // failure: `midenc-driver` downcasts it into `Ok(())` and exits 0. So a render
        // failure recorded before the stop must displace it, or
        // `-Zlint -Canalyze-only --emit=masm=<unwritable>` exits 0 having written nothing and
        // said nothing.
        let err =
            compile_expecting_error("driver_render_stop", "stopboom", STOP_AFTER_FAILED_RENDER);

        assert!(
            !err.is::<CompilerStopped>(),
            "a stop signal here would be downcast into a clean exit, making the failure silent"
        );
        let rendered = format!("{err}");
        assert!(
            rendered.contains(RENDER_FAILURE),
            "the renderer's own report is what must reach the caller: {rendered}"
        );
    }

    #[test]
    fn a_clean_stop_survives_when_nothing_failed_to_render() {
        // The half that keeps the fix from being "any stop is now an error": the same
        // frontend, the same stop, and a renderer that succeeds must still exit cleanly.
        // `midenc::unconstrained_advice_inter_module.shtest` depends on this.
        let err =
            compile_expecting_error("driver_render_stop_ok", "stopok", STOP_AFTER_CLEAN_RENDER);

        assert!(
            err.is::<CompilerStopped>(),
            "a stop with nothing to report must stay a stop, or every -Canalyze-only run fails: \
             {err}"
        );
        assert_eq!(
            rendered(),
            vec![CheckpointId::MASM_PARSED],
            "the fixture must actually have rendered, or it does not discriminate"
        );
    }

    #[test]
    fn a_genuine_assembly_error_wins_over_a_render_failure() {
        // The other half: only the stop *sentinel* is displaced. A real failure is the more
        // informative report — the render very likely failed because what it was handed was
        // never built correctly — so it must not be replaced.
        let err =
            compile_expecting_error("driver_render_hard", "hardboom", FAIL_AFTER_FAILED_RENDER);

        let rendered_err = format!("{err}");
        assert!(
            rendered_err.contains(ASSEMBLY_FAILURE),
            "the build's own failure is the one worth reading: {rendered_err}"
        );
        assert!(
            !rendered_err.contains(RENDER_FAILURE),
            "and it must not be displaced by the render failure it caused: {rendered_err}"
        );
    }

    // -------------------------------------------------------------------------------------
    // Provider construction.
    // -------------------------------------------------------------------------------------

    #[test]
    fn every_registered_extension_gets_a_provider_sharing_one_frontend_per_registration() {
        // Asserted against `providers` directly: the property is about a target whose root
        // extension is *not* the selected frontend's — a MASM dependency of a Rust root —
        // and materializing a dependency package to observe it through a whole build would
        // test the assembler's dispatch rather than this construction.
        std::thread_local! {
            /// How many times the two-extension registration has been instantiated.
            static INSTANTIATED: RefCell<usize> = const { RefCell::new(0) };
        }

        fn make_counted(_session: Rc<Session>) -> Rc<dyn Frontend> {
            INSTANTIATED.with_borrow_mut(|count| *count += 1);
            Rc::new(StubFrontend)
        }

        const COUNTED: FrontendRegistration = FrontendRegistration::new(
            FrontendId::new("counted"),
            &["counted", "counted2"],
            &[CheckpointId::PACKAGE_ASSEMBLED],
            &[],
            &[ArtifactDecl {
                checkpoint: CheckpointId::PACKAGE_ASSEMBLED,
                id: ArtifactId::PACKAGE,
                render: unrendered,
            }],
            make_counted,
        );

        let (session, manifest) = session("driver_providers");
        let mut registry = FrontendRegistry::new();
        registry.register(STUB_FRONTEND).expect("stub should register");
        registry.register(COUNTED).expect("counted should register");
        let pipeline = Pipeline::new(registry);

        let prepared = prepare_project(
            &input(&manifest),
            &session.options,
            &pipeline.registry,
            session.source_manager.as_ref(),
        )
        .expect("the fixture project should prepare");
        let state = Rc::new(RequestState::new(Goal::at(CheckpointId::MASM_PARSED), Vec::new()));

        let providers = pipeline.providers(&session, &state, &prepared);

        assert_eq!(
            providers.iter().map(|provider| provider.file_type()).collect::<Vec<_>>(),
            vec!["counted", "counted2", STUB],
            "every registered extension must be served, not only the selected frontend's: the \
             assembler picks a provider per target by that target's root extension"
        );
        assert_eq!(
            INSTANTIATED.with_borrow(|count| *count),
            1,
            "a registration is instantiated once and shared across its extensions; a second \
             instance would split the frontend's per-target memoization"
        );
    }

    #[test]
    fn the_default_pipeline_registers_the_shipped_frontends() {
        let pipeline = Pipeline::with_default_frontends().expect("the shipped frontends register");
        assert_eq!(
            pipeline.registry.for_extension("rs").map(|found| found.id()),
            Some(FrontendId::new("rust"))
        );
        assert_eq!(
            pipeline.registry.for_extension("masm").map(|found| found.id()),
            Some(FrontendId::new("masm"))
        );
    }
}
