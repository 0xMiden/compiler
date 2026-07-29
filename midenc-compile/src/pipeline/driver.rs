//! The driver: running one compilation request from an input to an [`Outcome`].

use alloc::{boxed::Box, format, rc::Rc, sync::Arc, vec::Vec};
use core::{cell::RefCell, ops::ControlFlow};

use miden_assembly::{
    AssemblyInterrupted, InterruptedTargetRole, ProjectSourceProvider, ProjectTargetSelector,
};
use miden_mast_package::Package as MastPackage;
use miden_package_registry::{PackageCache, PackageId};
use midenc_session::{
    FileType, InputFile, Session,
    diagnostics::Report,
    miden_project::{Target, TargetType},
};

use super::{
    Artifact, ArtifactId, CheckpointId, FrontendProvider, FrontendRegistration, FrontendRegistry,
    Observer, Outcome, OutputRequest, PreAssemblyHook, PreparedProject, RequestState, RootTarget,
    Start, TargetRole, apply_stop_flags, assembly::prepare_assembler, prepare_project,
    prepare_standalone, require_input_path_for_seed, resolve_goal, seed,
};
use crate::{CompilerResult, CompilerStopped};

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
    /// Where this compilation begins: at the input, or at an artifact already in hand.
    start: Start,
    /// A callback to run against the root target's Miden Assembly, just before assembly.
    pre_assembly: Option<PreAssemblyHook>,
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
            start: Start::Input,
            pre_assembly: None,
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

    /// Begin at `start` rather than at the input.
    ///
    /// A [`Start::At`] resumes the selected target's route from an artifact the caller already
    /// holds; see [`Start`] for what it may carry, and `pipeline/seed.rs` for how it is
    /// installed. Everything else about the request — the project, the target, the goal, the
    /// observers — is unchanged, which is what makes a seeded run comparable with an unseeded
    /// one.
    pub fn with_start(mut self, start: Start) -> Self {
        self.start = start;
        self
    }

    /// Run `pre_assembly` against the root target's Miden Assembly, just before it is
    /// assembled.
    ///
    /// See [`PreAssemblyHook`] for why this is not an [`Observer`], and for the `'static`
    /// bound it carries.
    pub fn with_pre_assembly(mut self, pre_assembly: PreAssemblyHook) -> Self {
        self.pre_assembly = Some(pre_assembly);
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
    ///
    /// Four registrations, one per extension family the compiler can compile from source:
    /// `rs`, `masm`, `wasm`/`wat` and `hir`. They are registered for *every* build, project or
    /// standalone, because a registration answers for any target with its extension — a Rust
    /// project with a Miden Assembly dependency needs the `masm` entry as much as a
    /// `midenc lib.masm` does.
    ///
    /// [`RUST_STANDALONE_FRONTEND`](super::frontends::RUST_STANDALONE_FRONTEND) is deliberately
    /// **not** here, and cannot be: it claims `rs`, which
    /// [`RUST_FRONTEND`](super::frontends::RUST_FRONTEND) already holds, and the registry
    /// rejects a second claim on an extension. The two are different entry points to the same
    /// language — one runs `cargo`, one compiles a file in this process — and choosing between
    /// them is a property of the *request*, not of the registry. `select_standalone_frontend`
    /// in `prepare.rs` makes that choice, for the root target of a standalone request alone.
    pub fn with_default_frontends() -> CompilerResult<Self> {
        let mut registry = FrontendRegistry::new();
        registry.register(super::frontends::RUST_FRONTEND)?;
        registry.register(super::frontends::MASM_FRONTEND)?;
        registry.register(super::frontends::WASM_FRONTEND)?;
        registry.register(super::frontends::HIR_FRONTEND)?;
        Ok(Self::new(registry))
    }

    /// Compile `request`, caching assembled packages in `cache`.
    ///
    /// The project is prepared once — one compiler-side `Project::load` for a manifest input,
    /// one synthesis for a standalone one — and the goal is resolved against the route of the
    /// frontend that preparation selected, after the session's `-C` stop flags have been folded
    /// into the request as a stop point. Everything past preparation is common to both kinds of
    /// input: one [`PreparedProject`], one set of providers, one `assemble_interruptible`, so
    /// that a request stopping short of assembly is a success rather than an error.
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
            start,
            pre_assembly,
        } = request;

        // Before the project is loaded, so that a seeded request naming no input reports what it
        // is missing rather than whatever preparation makes of a locator it cannot use.
        if let Start::At { .. } = &start {
            require_input_path_for_seed(&input)?;
        }

        // Decided here, not re-derived later: it is the one question preparation asked, and the
        // providers below have to give the same answer it did.
        let is_standalone = input.file_type() != FileType::Toml;
        let prepared = self.prepare(&input, &session)?;
        // The `-C` stop flags are folded in here rather than by the caller, because the
        // checkpoint each one names depends on the route, and which frontend runs is not known
        // until preparation has selected the target. Every request this method serves picks
        // them up, so a route flipped over to the pipeline later inherits them.
        let outputs = apply_stop_flags(outputs, &session.options, &prepared.frontend)?;
        let goal = resolve_goal(&outputs, &prepared.frontend)?;

        // Appended after the caller's own, so that a caller observing a checkpoint sees it
        // before anything is written for it, and so that a caller cannot displace emission by
        // supplying observers of its own.
        let emitter = Rc::new(RefCell::new(EmitObserver::new(session.clone(), prepared.frontend)));
        observers.push(emitter.clone() as Rc<RefCell<dyn Observer>>);
        let state = Rc::new(RequestState::new(goal, observers).with_pre_assembly(pre_assembly));

        let mut assembler = miden_assembly::Assembler::new(session.source_manager.clone())
            .with_warnings_as_errors(session.options.diagnostics.warnings.warnings_as_errors());
        prepare_assembler(&mut assembler, &prepared.package, &session)?;

        // The request-scoped providers, in the order they are applied: later wins for the
        // extension it claims. Both serve the *selected* target's root extension, so a seeded
        // standalone request installs the standalone provider and then displaces it — which is
        // right, because the seed replaces reading the input the standalone provider carries.
        let mut request_scoped = Vec::new();
        if is_standalone {
            request_scoped.push(self.standalone_provider(&session, &state, &prepared, input)?);
        }
        if let Start::At {
            checkpoint,
            artifact,
        } = start
        {
            request_scoped.push(seed::provider(checkpoint, artifact, &prepared, &session, &state)?);
        }
        let providers = self.providers(&session, &state, &prepared, request_scoped);
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

    /// Resolve `input` into the project, target and frontend this request runs with.
    ///
    /// # One question decides it: does the input name a manifest?
    ///
    /// A `.toml` input is a *locator*: it names the project to build and nothing is compiled
    /// from it, so the project is loaded from it. Every other input is a *source file* (or
    /// bytes on standard input), from which a project is synthesized around a single target
    /// rooted at that file. Both produce a [`PreparedProject`], and nothing downstream of here
    /// asks which kind of input it came from.
    ///
    /// The frontend is chosen the same way in both cases — by the extension of the *selected
    /// target's root*, never by the input's own — which is what makes a Rust-rooted manifest
    /// and a `midenc foo.rs` reach the same dispatch. The one deliberate difference is which
    /// registration a standalone `rs` root gets; `select_standalone_frontend` in `prepare.rs`
    /// explains it.
    ///
    /// # `.masp` is the one input that is neither
    ///
    /// A `.masp` is an already-assembled package, so there is nothing to compile and no
    /// frontend to dispatch to. It is refused here, with the wording the legacy dispatcher
    /// used, rather than being synthesized into a target no registration answers for — which
    /// would fail later with a diagnostic about extensions rather than about what was asked.
    fn prepare(&self, input: &InputFile, session: &Session) -> CompilerResult<PreparedProject> {
        match input.file_type() {
            FileType::Toml => prepare_project(
                input,
                &session.options,
                &self.registry,
                session.source_manager.as_ref(),
            ),
            FileType::Masp => Err(Report::msg("unsupported input file type '.masp'")),
            _ => prepare_standalone(input, session, &self.registry),
        }
    }

    /// The provider a **standalone** request installs for its root target's extension.
    ///
    /// Two things make it necessary, and either alone would:
    ///
    /// - **The frontend may not be the registry's.** A standalone `.rs` root is compiled in this
    ///   process rather than by `cargo`, so preparation answers with
    ///   [`RUST_STANDALONE_FRONTEND`](super::frontends::RUST_STANDALONE_FRONTEND) — which cannot
    ///   be registered, because [`RUST_FRONTEND`](super::frontends::RUST_FRONTEND) already claims
    ///   `rs` and the registry rejects a second claim. Without this override the assembler would
    ///   consult the registry's `rs` provider and run a `cargo` build of a manifest that does not
    ///   exist.
    /// - **The frontend needs the input itself.** A standalone input may be stdin-backed: it
    ///   exists only in memory, and the target root synthesized for it names a file that was
    ///   never written. [`FrontendProvider::with_input`] is how the bytes reach the frontend, and
    ///   the provider that carries them must be the one serving the target the input backs.
    ///
    /// It is built from `prepared.frontend` rather than from the registry, so it is the *selected*
    /// registration in both cases — a `.wasm` root gets a second instance of the registry's own
    /// wasm frontend, which is the cost [`Pipeline::providers`] describes and which buys the
    /// input.
    fn standalone_provider(
        &self,
        session: &Rc<Session>,
        state: &Rc<RequestState>,
        prepared: &PreparedProject,
        input: InputFile,
    ) -> CompilerResult<Box<dyn ProjectSourceProvider>> {
        Ok(Box::new(
            FrontendProvider::new(
                super::selected_provider_extension(prepared)?,
                prepared.frontend.instantiate(session.clone()),
                session.clone(),
                state.clone(),
                RootTarget::new(prepared.package.clone(), &prepared.target),
            )
            .with_input(input),
        ))
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
    ///
    /// `request_scoped` are providers that must serve *one* extension for this request alone,
    /// displacing the registry's. They go in last, in order, and that is the whole mechanism:
    /// `SourceProviderRegistry::new` collects providers into a map keyed by
    /// [`ProjectSourceProvider::file_type`], so the last one written for an extension is the
    /// one the assembler consults — which is also how two of them compose, the later winning.
    /// The extension each serves is that `&'static str` — the assembler's registry is keyed by
    /// nothing else — so a caller building one takes it from
    /// [`selected_provider_extension`](super::selected_provider_extension), never from the
    /// resolved target root, which is a runtime `Cow<str>`.
    ///
    /// Passing one is *additive*: the registry's provider for that extension is still built,
    /// and simply loses. So an override phrased as "instantiate the selected registration again"
    /// really does produce a second instance of a registration the registry already holds, and
    /// that is worth understanding before writing one.
    ///
    /// # The seed does exactly that, and it is safe
    ///
    /// `seed.rs` wraps `prepared.frontend.instantiate(session)` so that every target the seed is
    /// *not* for is compiled as it would have been. For a project request that is a second
    /// instance of the registry's own registration. Two things make it harmless:
    ///
    /// - **Providers partition by extension.** The seed's provider wins for the selected root's
    ///   extension and serves *every* target with it, so no single target is ever served by both
    ///   instances. What splits is a registration that claims *several* extensions — a seeded
    ///   `.wasm` root with a `.wat` dependency runs the two through different instances.
    /// - **Frontend state is keyed by [`TargetKey`](super::TargetKey), not by instance.** Every
    ///   shipped frontend memoizes into a map keyed that way (`frontends/wasm.rs`,
    ///   `frontends/rust.rs`), and each target's `provide_sources`, `provide_source_provenance`
    ///   and `post_process_package` all arrive through the one provider for its extension. So a
    ///   split across extensions costs a second empty map, not a lost memoization.
    ///
    /// The remaining cost is an allocation. Sharing the registry's instance instead would mean
    /// this method handing its instances back out, which is a wider change than the saving is
    /// worth — but if this ever grows a third override, that is the direction.
    ///
    /// # Why this is a parameter, and not carried by [`PreparedProject`]
    ///
    /// Hanging it off the preparation is the obvious shape, and it does not work: a provider
    /// needs the [`RequestState`] every target of a request shares, and that is built in
    /// [`Pipeline::compile`] from a [`Goal`](super::Goal) resolved *against*
    /// `prepared.frontend` — so it does not exist while preparation is running, which in any
    /// case only receives `&session.options` and not the `Rc<Session>` a
    /// [`FrontendProvider`] also needs. A preparation could therefore only carry a recipe,
    /// not a provider. Whoever needs an override builds it here, where both are in hand.
    fn providers(
        &self,
        session: &Rc<Session>,
        state: &Rc<RequestState>,
        prepared: &PreparedProject,
        request_scoped: Vec<Box<dyn ProjectSourceProvider>>,
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
        providers.extend(request_scoped);
        providers
    }
}

/// Choose which of a failed assembly's two reports to raise.
///
/// [`CompilerStopped`] is the odd one out: it is not a failure at all, but the signal a
/// frontend raises to end the build early — `midenc-driver` downcasts it into `Ok(())` and
/// exits 0 (`midenc-driver/src/lib.rs`). So a recorded render failure carried alongside it
/// would be dropped and the run would report success having written nothing and said nothing:
/// a renderer fails at an early checkpoint, and the frontend then ends the build cleanly.
///
/// No shipped frontend takes that route now that the `-C` stop flags are goals — a goal reached
/// stops the build through [`Flow::Stop`](super::Flow::Stop), which arrives here as
/// `ControlFlow::Break` rather than as an error, and the render failure is surfaced by the
/// caller's own check. The guard stays because [`CompilerStopped`] is public and any frontend
/// may still raise it.
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
            if let Flow::Break(stopped) =
                cx.checkpoint(CheckpointId::MASM_PARSED, ArtifactId::MASM, Parsed(name))?
            {
                return Ok(Flow::Break(stopped));
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
    /// Stands in for any frontend that publishes and then raises [`CompilerStopped`] — the
    /// shape in which a render failure can be lost, since the stop is not a failure. No
    /// shipped frontend does so any more; see [`prefer_render_error`], whose guard this
    /// fixture is the only remaining exercise of.
    struct EndingFrontend(fn() -> Report);

    impl Frontend for EndingFrontend {
        fn compile(&self, cx: &TargetContext<'_>) -> CompilerResult<Flow<ProjectSourceInputs>> {
            let name = cx.assembly().target.name.inner().to_string();
            if let Flow::Break(stopped) =
                cx.checkpoint(CheckpointId::MASM_PARSED, ArtifactId::MASM, Parsed(name))?
            {
                return Ok(Flow::Break(stopped));
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
        session_configured(dir, extension, |_| {})
    }

    /// The same, with `configure` applied to the options the session is built from.
    fn session_configured(
        dir: &str,
        extension: &str,
        configure: impl FnOnce(&mut Options),
    ) -> (Rc<Session>, PathBuf) {
        let manifest = manifest_rooted_at(dir, extension);
        let mut options = Box::new(Options::default());
        configure(&mut options);
        let options = options.with_output_types(Default::default(), None);
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
    // The `-C` stop flags.
    //
    // `goal.rs` owns the mapping and tests it against every shipped route. What is checked
    // here is that a request picks it up at all: a flag left in `Options` and never consulted
    // would satisfy every assertion there.
    // -------------------------------------------------------------------------------------

    #[test]
    fn a_stop_flag_in_the_session_options_caps_the_build() {
        // The stub route has no HIR, so `-Cparse-only` falls through to `masm.parsed` — which
        // is what makes this a test of the mapping rather than of a hardcoded checkpoint.
        let (session, manifest) =
            session_configured("driver_parse_only", STUB, |options| options.parse_only = true);
        let observer = recorder();
        let request = CompilationRequest::new(session, input(&manifest))
            .with_observers(vec![observer.clone() as Rc<RefCell<dyn Observer>>]);

        let outcome = pipeline()
            .compile(request, &mut NoPackageStore)
            .expect("stopping short of assembly is a success, not an error");

        assert_eq!(outcome.checkpoint(), CheckpointId::MASM_PARSED);
        assert_eq!(
            trace(&observer),
            vec![(CheckpointId::MASM_PARSED, TargetRole::Root)],
            "the flag must cap the build exactly as `--stop-after` does"
        );
    }

    #[test]
    fn a_stop_flag_the_route_cannot_express_is_reported() {
        // The stub route reaches no analysis checkpoint: a flag naming a phase the route never
        // reaches must be a diagnostic rather than a silently uncapped build. No shipped route
        // is in that position — the manifest-backed Rust route was the live case and no longer
        // is — which is exactly why the property is pinned on a fixture rather than left to one.
        let (session, manifest) =
            session_configured("driver_analyze_only", STUB, |options| options.analyze_only = true);
        let request = CompilationRequest::new(session, input(&manifest));

        let err = pipeline()
            .compile(request, &mut NoPackageStore)
            .expect_err("the stub route cannot stop after an analysis it never performs");

        let rendered = format!("{err}");
        assert!(rendered.contains("-Canalyze-only"), "{rendered}");
        assert!(rendered.contains("known limitation"), "{rendered}");
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
        // failure recorded before the stop must displace it, or a run whose `--emit`
        // destination could not be written exits 0 having written nothing and said nothing.
        // The stop-at-goal path reaches the same conclusion by a different branch — an
        // interruption is `Ok(ControlFlow::Break)`, so the caller's own render-error check
        // fires — which is why this fixture ends the build with an error instead.
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
            "a stop with nothing to report must stay a stop, or every early-exiting run fails: \
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

        let providers = pipeline.providers(&session, &state, &prepared, Vec::new());

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

    // -------------------------------------------------------------------------------------
    // The request-scoped provider.
    // -------------------------------------------------------------------------------------

    /// A second registration claiming [`STUB`], which no registry may hold beside
    /// [`STUB_FRONTEND`] — `FrontendRegistry::register` rejects the duplicate extension.
    ///
    /// Its frontend skips `masm.parsed`, so which of the two served a callback is readable
    /// off the observer trace rather than having to be inferred from the sources handed back.
    const OVERRIDING: FrontendRegistration = FrontendRegistration::new(
        FrontendId::new("overriding"),
        &[STUB],
        &[CheckpointId::MASM_LOWERED, CheckpointId::PACKAGE_ASSEMBLED],
        &[],
        &[
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

    /// A single-target virtual project rooted at a `.stub` file.
    fn stub_project(name: &str) -> VirtualProject {
        let root = crate::pipeline::testing::fixture_source(name, "lib.stub", "stub");
        VirtualProject::new(name, &root, TargetType::Library).expect("should build")
    }

    /// The preparation a request to build `project`'s selected target with `frontend`
    /// arrives at the driver with.
    ///
    /// Built by hand rather than through [`prepare_project`], which can only ever select a
    /// frontend the registry holds — and a request-scoped provider exists precisely for the
    /// requests whose frontend it does not.
    fn prepared_for(project: &VirtualProject, frontend: FrontendRegistration) -> PreparedProject {
        PreparedProject {
            package: project.package(),
            manifest_path: PathBuf::new(),
            target: project.target().clone(),
            profile_name: "dev".to_string(),
            frontend,
        }
    }

    /// Serve `project`'s selected target from `providers`, the way the assembler does: keyed
    /// by extension, last writer winning.
    fn serve(providers: Vec<Box<dyn ProjectSourceProvider>>, project: &VirtualProject) {
        let assembly = project.assembly_context().expect("assembly context");
        let served = miden_assembly::SourceProviderRegistry::new(providers)
            .get_provider(STUB)
            .expect("some provider must serve the stub extension")
            .provide_sources_interruptible(&assembly)
            .expect("neither fixture frontend errors");
        assert!(
            served.is_continue(),
            "these fixtures publish short of the goal, so the build must run on and the trace \
             must be the whole of what the serving frontend published"
        );
    }

    /// The request state for a full build that records what it reaches.
    fn recording_state(observer: &Rc<RefCell<RecordingObserver>>) -> Rc<RequestState> {
        Rc::new(RequestState::new(
            Goal::at(CheckpointId::PACKAGE_ASSEMBLED),
            vec![observer.clone() as Rc<RefCell<dyn Observer>>],
        ))
    }

    #[test]
    fn a_request_scoped_provider_serves_the_extension_the_registry_would_have() {
        let pipeline = pipeline();
        assert_eq!(
            pipeline.registry.for_extension(STUB).map(FrontendRegistration::id),
            Some(STUB_FRONTEND.id()),
            "the registry serves `.stub` with its own registration, which is what the \
             request-scoped one has to displace"
        );
        assert_ne!(OVERRIDING.id(), STUB_FRONTEND.id(), "two registrations claim one extension");

        let project = stub_project("driver_override_installed");
        let (session, _manifest) = session("driver_override_installed");
        let observer = recorder();
        let state = recording_state(&observer);

        // The extension comes from the overriding registration's own `&'static str`
        // extensions — the type the assembler's provider map is keyed by — and never from the
        // resolved target root, which yields a runtime `Cow<str>`.
        let extension: &'static str = OVERRIDING.extensions()[0];
        let overriding = Box::new(FrontendProvider::new(
            extension,
            OVERRIDING.instantiate(session.clone()),
            session.clone(),
            state.clone(),
            RootTarget::new(project.package(), project.target()),
        )) as Box<dyn ProjectSourceProvider>;
        assert_eq!(
            overriding.file_type(),
            STUB,
            "the override must be installed under the extension it is meant to displace"
        );

        let prepared = prepared_for(&project, STUB_FRONTEND);
        let providers = pipeline.providers(&session, &state, &prepared, alloc::vec![overriding]);
        assert_eq!(
            providers.iter().map(|provider| provider.file_type()).collect::<Vec<_>>(),
            vec![STUB, STUB],
            "installing an override is additive: the registry's provider for the extension is \
             still built, and merely loses. An implementation that replaced it instead would \
             satisfy every other assertion here"
        );

        serve(providers, &project);

        assert_eq!(
            trace(&observer),
            vec![(CheckpointId::MASM_LOWERED, TargetRole::Root)],
            "the request-scoped provider must serve the callback: its frontend never publishes \
             `masm.parsed`, which the registry's does"
        );
    }

    #[test]
    fn without_a_request_scoped_provider_the_registrys_serves() {
        // The discriminating half: the same extension, the same target, and nothing
        // overriding it must still reach the registry's frontend. Without this, a driver
        // that dropped the registry's provider entirely would pass the test above.
        let pipeline = pipeline();
        let project = stub_project("driver_override_absent");
        let (session, _manifest) = session("driver_override_absent");
        let observer = recorder();
        let state = recording_state(&observer);
        let prepared = prepared_for(&project, STUB_FRONTEND);

        serve(pipeline.providers(&session, &state, &prepared, Vec::new()), &project);

        assert_eq!(
            trace(&observer),
            vec![
                (CheckpointId::MASM_PARSED, TargetRole::Root),
                (CheckpointId::MASM_LOWERED, TargetRole::Root),
            ],
            "the registry's own frontend publishes both of its checkpoints"
        );
    }

    #[test]
    fn the_default_pipeline_registers_every_shipped_frontend() {
        // One registration per extension family the compiler compiles from source. A missing
        // one is not a degraded build but a "no frontend is registered for '<ext>'" failure of
        // every standalone input with that extension, since the standalone dispatch selects
        // out of this registry.
        let pipeline = Pipeline::with_default_frontends().expect("the shipped frontends register");
        for (extension, id) in [
            ("rs", "rust"),
            ("masm", "masm"),
            ("wasm", "wasm"),
            ("wat", "wasm"),
            ("hir", "hir"),
        ] {
            assert_eq!(
                pipeline.registry.for_extension(extension).map(|found| found.id()),
                Some(FrontendId::new(id)),
                "'{extension}' must dispatch to the '{id}' frontend"
            );
        }
    }

    // -------------------------------------------------------------------------------------
    // The standalone dispatch.
    //
    // A source file rather than a manifest: preparation synthesizes the project instead of
    // loading one, and the driver installs a provider carrying the request's own input.
    //
    // These stop at what is decidable without a package registry. A synthesized project depends
    // on `miden-core`, which is resolved from the toolchain — so a whole standalone build in
    // process would assert about the environment rather than about this dispatch. The end-to-end
    // claim is the lit suite's, which compiles `.rs` and `.wat` inputs through this branch.
    // -------------------------------------------------------------------------------------

    /// A session over the standalone `input`, with no manifest anywhere.
    fn standalone_session(input: &InputFile) -> Rc<Session> {
        let options = Box::new(Options::default())
            .with_verbosity(midenc_session::Verbosity::Silent)
            .with_output_types(Default::default(), None);
        let source_manager: Arc<dyn SourceManager + Send + Sync> =
            Arc::new(DefaultSourceManager::default());
        Rc::new(
            Session::new(input.clone(), options, None, source_manager)
                .expect("a standalone input should open a session"),
        )
    }

    #[test]
    fn a_source_file_input_is_prepared_as_a_synthesized_project() {
        let root = crate::pipeline::testing::wat_fixture("driver_standalone_wat", "lib.wat");
        let input = input(&root);
        let session = standalone_session(&input);

        let prepared = Pipeline::with_default_frontends()
            .expect("the shipped frontends register")
            .prepare(&input, &session)
            .expect("a `.wat` file is a standalone input, and needs no manifest");

        assert_eq!(
            prepared.manifest_path,
            PathBuf::new(),
            "a synthesized project was named by no locator, and says so"
        );
        assert_eq!(
            prepared.frontend.id(),
            FrontendId::new("wasm"),
            "the frontend is chosen by the synthesized target's root extension, exactly as it is \
             for a manifest"
        );
    }

    #[test]
    fn a_manifest_input_is_prepared_by_loading_it() {
        // The discriminating half: the branch is on the input's file type, so a dispatch that
        // synthesized unconditionally would satisfy the test above and lose every project build.
        let (session, manifest) = session("driver_standalone_manifest");
        let input = input(&manifest);

        let prepared = pipeline()
            .prepare(&input, &session)
            .expect("the fixture project should prepare");

        assert_eq!(
            prepared.manifest_path, manifest,
            "a `.toml` input is a locator: the project is loaded from it, not synthesized"
        );
    }

    #[test]
    fn an_already_assembled_package_is_not_an_input() {
        // `.masp` is neither a locator nor something a frontend compiles, so it is refused in
        // preparation with the wording the legacy dispatcher used — rather than synthesized into
        // a target whose extension no registration claims, which would fail later with a
        // diagnostic about extensions instead of about what was asked for.
        let root =
            crate::pipeline::testing::fixture_source("driver_standalone_masp", "lib.masp", "");
        let input = input(&root);
        let session = standalone_session(&input);

        let err = Pipeline::with_default_frontends()
            .expect("the shipped frontends register")
            .prepare(&input, &session)
            .expect_err("a package is not a compiler input");

        assert!(
            format!("{err}").contains("unsupported input file type '.masp'"),
            "the refusal must name the file type: {err}"
        );
    }

    #[test]
    fn a_standalone_request_hands_its_own_input_to_the_frontend() {
        // The property the standalone provider exists for, beyond substituting a registration:
        // a stdin-backed input lives only in memory, and the target root synthesized for it
        // names a file that was never written — so a frontend reading `resolved_target_root`
        // finds nothing. `FrontendProvider::with_input` is the only way the bytes reach it, and
        // this branch is the only thing that supplies one.
        let project = stub_project("driver_standalone_input");
        let (session, _manifest) = session("driver_standalone_input");
        let observer = recorder();
        let state = recording_state(&observer);
        let prepared = prepared_for(&project, INPUT_OBSERVING_FRONTEND);
        let piped = InputFile::new(
            midenc_session::FileType::Wat,
            midenc_session::InputType::Stdin {
                name: "stdin.wat".into(),
                input: b"(module)".to_vec(),
            },
        );

        let provider = pipeline()
            .standalone_provider(&session, &state, &prepared, piped)
            .expect("the fixture target root has an extension the frontend claims");
        assert_eq!(
            provider.file_type(),
            STUB,
            "the override must be keyed by the selected root's extension, not by the input's"
        );

        serve(alloc::vec![provider], &project);

        assert!(
            SAW_INPUT.with_borrow(|saw| *saw),
            "the frontend must be handed the request's own input, or a piped-in source cannot be \
             read at all"
        );
    }

    std::thread_local! {
        /// Whether [`INPUT_OBSERVING_FRONTEND`] was given the request's input.
        static SAW_INPUT: RefCell<bool> = const { RefCell::new(false) };
    }

    /// A frontend that records whether its context carried the compiler input.
    struct InputObservingFrontend;

    impl Frontend for InputObservingFrontend {
        fn compile(&self, cx: &TargetContext<'_>) -> CompilerResult<Flow<ProjectSourceInputs>> {
            SAW_INPUT.with_borrow_mut(|saw| *saw = cx.input().is_some());
            StubFrontend.compile(cx)
        }

        fn provenance(
            &self,
            cx: &TargetContext<'_>,
        ) -> CompilerResult<ProjectSourceProvenanceInputs> {
            StubFrontend.provenance(cx)
        }
    }

    fn make_input_observing(_session: Rc<Session>) -> Rc<dyn Frontend> {
        Rc::new(InputObservingFrontend)
    }

    const INPUT_OBSERVING_FRONTEND: FrontendRegistration = FrontendRegistration::new(
        FrontendId::new("input-observing"),
        &[STUB],
        &[
            CheckpointId::MASM_PARSED,
            CheckpointId::MASM_LOWERED,
            CheckpointId::PACKAGE_ASSEMBLED,
        ],
        &[],
        &[ArtifactDecl {
            checkpoint: CheckpointId::PACKAGE_ASSEMBLED,
            id: ArtifactId::PACKAGE,
            render: unrendered,
        }],
        make_input_observing,
    );
}
