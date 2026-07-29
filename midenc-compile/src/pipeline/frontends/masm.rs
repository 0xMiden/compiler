//! The frontend for projects whose targets are rooted at Miden Assembly sources.

use alloc::rc::Rc;

use miden_assembly::{
    MasmSourceProvider, ProjectSourceInputs, ProjectSourceProvenanceInputs, ProjectSourceProvider,
};
use midenc_frontend_masm::{DisassembledWorld, DisassemblerConfig};
use midenc_session::{
    FileName, InputType, OutputMode, Session,
    diagnostics::{IntoDiagnostic, Report},
};

use crate::{
    CompilerResult,
    pipeline::{
        Artifact, ArtifactDecl, ArtifactId, CheckpointId, Flow, Frontend, FrontendId,
        FrontendRegistration, TargetContext,
    },
};

/// Declares the frontend that handles targets rooted at a `.masm` file.
pub const MASM_FRONTEND: FrontendRegistration = FrontendRegistration::new(
    FrontendId::new("masm"),
    &["masm"],
    &[
        CheckpointId::MASM_PARSED,
        CheckpointId::HIR_ANALYZED,
        CheckpointId::MASM_LOWERED,
        CheckpointId::PACKAGE_ASSEMBLED,
    ],
    &[
        ("parse", CheckpointId::MASM_PARSED),
        ("analyze", CheckpointId::HIR_ANALYZED),
        ("lower", CheckpointId::MASM_LOWERED),
        ("assemble", CheckpointId::PACKAGE_ASSEMBLED),
    ],
    &[
        // The one checkpoint on this route that writes: `--emit=masm` lands here.
        ArtifactDecl {
            checkpoint: CheckpointId::MASM_PARSED,
            id: ArtifactId::MASM,
            render: render_masm_sources,
        },
        // Written inline by [`MasmProjectFrontend::analyze`], which calls
        // `crate::emit_hir_if_requested` before the advice-taint diagnostics are raised and
        // before the lint's own early stop. See [`unrendered`].
        ArtifactDecl {
            checkpoint: CheckpointId::HIR_ANALYZED,
            id: ArtifactId::HIR,
            render: unrendered,
        },
        // Written at `masm.parsed` above: this route publishes the very same
        // [`ProjectSourceInputs`] at both checkpoints, so rendering here as well would emit
        // every module twice. See [`unrendered`].
        ArtifactDecl {
            checkpoint: CheckpointId::MASM_LOWERED,
            id: ArtifactId::MASM,
            render: unrendered,
        },
        // Written by [`crate::compile`], which emits the assembled package in both `mast` and
        // `masp` form once the pipeline hands it back. See [`unrendered`].
        ArtifactDecl {
            checkpoint: CheckpointId::PACKAGE_ASSEMBLED,
            id: ArtifactId::PACKAGE,
            render: unrendered,
        },
    ],
    make_masm,
);

/// Build the frontend this registration declares.
fn make_masm(_session: Rc<Session>) -> Rc<dyn Frontend> {
    Rc::new(MasmProjectFrontend::default())
}

/// A renderer for the checkpoints on this route whose artifact something else already writes.
///
/// Every use of it names its writer at the declaration site. A second emission would not be a
/// harmless duplicate: the destination for an output type is resolved once from the session,
/// so two writers of one artifact either race for the same file or, when that destination is
/// stdout, print the artifact twice.
fn unrendered(_artifact: &Artifact, _session: &Session) -> CompilerResult<()> {
    Ok(())
}

/// Write this target's Miden Assembly, one document per module.
///
/// This is the shape [`ArtifactDecl::render`] exists to let a route choose: a MASM target is a
/// module *tree*, and it is written as a file per module — support modules first, then the
/// root — rather than as one concatenated document. The order and the shape are the standalone
/// `.masm` path's, in `ParseMasmStage`, so the two paths emit a given project alike.
///
/// Nothing here decides *whether* to write. [`Session::emit`] checks `should_emit` itself and
/// resolves the destination from the session's output files, so this runs at every root
/// `masm.parsed` and writes only what was asked for.
fn render_masm_sources(artifact: &Artifact, session: &Session) -> CompilerResult<()> {
    let inputs = artifact.downcast_ref::<ProjectSourceInputs>().ok_or_else(|| {
        Report::msg("cannot emit 'masm': the artifact is not a set of project source inputs")
    })?;
    for module in inputs.support.iter() {
        session.emit(OutputMode::Text, module).into_diagnostic()?;
    }
    session.emit(OutputMode::Text, &inputs.root).into_diagnostic()
}

/// Compiles a target whose root is a Miden Assembly source file.
///
/// # Source provision is upstream's, not ours
///
/// Registering this frontend for the `masm` extension **displaces**
/// [`MasmSourceProvider`], the assembler's own built-in: `SourceProviderRegistry::new`
/// installs it only when nothing else has claimed `"masm"`. That happens for *every* MASM
/// target in *every* dependency graph, including MASM dependencies of Rust projects — so
/// this frontend must not merely resemble the built-in, it must *be* it. It therefore holds
/// a [`MasmSourceProvider`] and delegates both [`Frontend::compile`]'s source path and
/// [`Frontend::provenance`] to it verbatim.
///
/// Reimplementing the read is the specific mistake to avoid. The compiler's own
/// `ParseMasmStage` looks like a drop-in and is not, and it is wrong on two independent
/// axes:
///
/// * **Module path.** It passes no module path, leaving the parser to derive one from the
///   source, where upstream passes `target.namespace`.
/// * **Module kind.** It derives the kind from `session.options.target_type`, which describes
///   the target being *compiled*, where upstream derives it from `target.ty`, which every
///   target in the graph has its own of.
///
/// `load_target_sources` checks both, and checks them independently
/// (`crates/assembly/src/project.rs:826-845`), so getting either one wrong fails MASM
/// dependencies. Getting the path wrong is the louder failure and the easier one to notice;
/// the kind is the one that survives a fixture suite made entirely of library targets, since
/// `TargetType::default()` is `Library`.
///
/// What this frontend adds over the built-in is the checkpoints, and the compiler's own
/// disassemble-to-HIR lint — nothing on the source path itself.
///
/// # No per-target state
///
/// [`MasmSourceProvider`] is stateless and re-reads its sources on each call, and this
/// frontend keeps that: memoizing `provenance` here would be a change in behaviour relative
/// to the built-in it replaces, not merely an optimization, and the MASM path is meant to
/// cost exactly what it costs today. That is also why `make_masm` ignores the session it
/// is handed — there is nothing to key.
pub struct MasmProjectFrontend {
    /// The assembler's own MASM source provider, which every source read delegates to.
    sources: MasmSourceProvider,
}

impl Default for MasmProjectFrontend {
    fn default() -> Self {
        Self {
            sources: MasmSourceProvider,
        }
    }
}

impl MasmProjectFrontend {
    /// Read this target's Miden Assembly sources.
    ///
    /// Almost always [`MasmSourceProvider`]'s job, and the type doc says why it must stay that
    /// way. The exception is the one case a path cannot serve: a standalone input piped in on
    /// standard input exists only in memory, and the target root synthesized for it names a
    /// file nobody ever wrote. Those bytes reach a frontend only through
    /// [`TargetContext::input`], so they are parsed here.
    ///
    /// Preferring the path whenever there *is* one is not merely equivalent, it is the safer of
    /// the two. A provider is built for an extension and serves every callback for it, so a
    /// `.masm` dependency of a standalone `.masm` request would otherwise be compiled from the
    /// root's bytes. That combination is expected to be unreachable — a synthesized project has
    /// one target and a registry-only dependency, with no sources of its own — and reading each
    /// target's own root means it does not have to be. [`WasmFrontend`](super::wasm::WasmFrontend)
    /// splits the same way, for the same reason.
    fn provide(&self, cx: &TargetContext<'_>) -> CompilerResult<ProjectSourceInputs> {
        match cx.input().map(|input| &input.file) {
            Some(InputType::Stdin { name, input }) => Self::parse_stdin(name, input, cx),
            _ => self.sources.provide_sources(cx.assembly()),
        }
    }

    /// Parse the root module of a target backed by `bytes` read from standard input.
    ///
    /// This is `ParseMasmStage::parse_masm_from_bytes` re-homed, and corrected on the two axes
    /// the type doc names. It differs from the legacy version in exactly those:
    ///
    /// * **Module path.** The legacy version named the module after the *input* — `stdin` —
    ///   which `load_target_sources` rejects on sight. The path is the target's namespace, as
    ///   it is for every other MASM target.
    /// * **Module kind.** From `target.ty`, not from the session's requested target type.
    ///
    /// The *source file's* URI is still the input's name, so a diagnostic raised in these bytes
    /// points at `stdin` rather than at a namespace, which is the name the user would recognize.
    /// Only the module's path is the namespace.
    ///
    /// There is no module tree to walk — bytes in memory have no directory — so a standalone
    /// `.masm` piped in is a single module, as it has always been.
    fn parse_stdin(
        name: &FileName,
        bytes: &[u8],
        cx: &TargetContext<'_>,
    ) -> CompilerResult<ProjectSourceInputs> {
        use alloc::{format, string::ToString, vec::Vec};

        use miden_assembly::{ModuleParser, ast::ModuleKind};
        use midenc_session::{
            diagnostics::{SourceLanguage, Uri, WrapErr},
            miden_project::TargetType,
        };

        let assembly = cx.assembly();
        let source = core::str::from_utf8(bytes)
            .into_diagnostic()
            .wrap_err_with(|| format!("input '{name}' contains invalid utf-8"))?;
        let source_file = assembly.source_manager.load(
            SourceLanguage::Masm,
            Uri::new(name.as_str()),
            source.to_string(),
        );

        let kind = match assembly.target.ty {
            TargetType::Executable => ModuleKind::Executable,
            TargetType::Kernel => ModuleKind::Kernel,
            _ => ModuleKind::Library,
        };
        let mut parser = ModuleParser::new(Some(kind));
        parser.set_warnings_as_errors(assembly.warnings_as_errors);
        let root = parser.parse(
            Some(assembly.target.namespace.inner().as_ref()),
            source_file,
            assembly.source_manager.clone(),
        )?;

        Ok(ProjectSourceInputs {
            root,
            support: Vec::new(),
        })
    }

    /// Disassemble this target to HIR and run the advice-taint lint over it.
    ///
    /// This is `MasmAnalysisStage`'s lint-only branch, re-homed. It performs the analysis and
    /// nothing else: where the legacy stage — and, until the `-C` stop flags became goals,
    /// this method — ended the run itself on `-Canalyze-only`, that stop is now resolved to a
    /// goal at `hir.analyzed` (see [`StopFlag`](crate::pipeline::StopFlag)) and taken by the
    /// caller's [`TargetContext::checkpoint`] below. A frontend-local stop would fire *before*
    /// that publication, so the goal would never be reached and the request would come back
    /// with nothing captured.
    ///
    /// What is *not* kept either is the legacy stage's conflation of that stop with a lint
    /// that raised errors. The two shared one `CompilerStopped`, which `midenc-driver`
    /// downcasts into a clean exit, so broken code exited zero. Errors fail the compilation
    /// with an ordinary [`Report`], as they do in
    /// [`backend::analyze`](crate::pipeline::backend::analyze), the other live copy of this
    /// rule.
    ///
    /// `hir.analyzed` is therefore published by the caller only when this *returns*, which
    /// preserves the legacy ordering: a run the lint fails publishes nothing for it.
    ///
    /// # A lint-only run of a project now pays for its dependencies first
    ///
    /// `-Zlint -Canalyze-only` still stops the build here, but no longer before the project's
    /// dependency closure has been built. The legacy route ran `MasmAnalysisStage` ahead of
    /// `AssembleProjectStage`, so an analyze-only run returned having assembled nothing. This
    /// analysis is now reached only through the assembler's source-provider callback, and by
    /// the time that callback fires, `assemble_source_package` has already resolved, linked
    /// and cached every dependency (`crates/assembly/src/project.rs:406-425`) — it calls
    /// `load_target_sources`, and so this frontend, only afterwards (`:428-434`). For an
    /// executable root, `assemble_interruptible` also assembles the project's library target
    /// before either (`:303-320`). So a lint-only run builds the dependency closure and writes
    /// `.masp` files into `target/miden/packages` that it previously never produced.
    ///
    /// That is an accepted consequence of routing project compilation through the assembler,
    /// not an oversight. The frontend is reachable only as an assembler callback, so any goal
    /// short of full assembly pays for dependency resolution first; moving the lint ahead of
    /// it would mean running it outside the frontend, which is the arrangement this design
    /// replaces.
    ///
    /// # The one behavioural deviation: which target gets linted
    ///
    /// Unlike the legacy stage, the already-parsed `inputs` are passed through rather than
    /// `None`. We have them, and letting the disassembler resolve the target from the project
    /// would read every source a second time; the `Some(..)` form is the one the standalone
    /// `.masm` path already takes.
    ///
    /// External metadata is unaffected — `resolve_project_target` calls the same
    /// `collect_dependency_metadata` the `Some(..)` branch does — but **target selection**
    /// is. The `None` branch resolves a target by preferring `library_target()` and only then
    /// the executables, ignoring which target is actually being compiled. So for a MASM
    /// project declaring both a `[lib]` and a `[[bin]]`, with no `-Cname` to disambiguate,
    /// the legacy path always linted the library; this lints the target being compiled.
    /// That is arguably the fix, but it *is* a behaviour difference, and it is the one to
    /// watch for when the Toml branch is flipped over to this frontend.
    ///
    /// The legacy stage wraps this body in `#[cfg(feature = "std")]` with a `log::warn!`
    /// fallback. That guard is not reproduced: [`crate::pipeline`] is itself `std`-gated, so
    /// a build without `std` does not compile this module at all and the fallback could never
    /// run.
    fn analyze(
        &self,
        inputs: &ProjectSourceInputs,
        cx: &TargetContext<'_>,
    ) -> CompilerResult<DisassembledWorld> {
        use midenc_hir::Op;

        let session = cx.session();
        let context = cx.context();
        let config = DisassemblerConfig {
            infer_missing_signatures: true,
        };
        let world = midenc_frontend_masm::disassemble_project_target(
            &session.project,
            // The target name selects which of the project's targets to *resolve*, and is
            // read only when no sources are supplied. We always supply them, so nothing here
            // is selected by name and any value passed would be inert. `None` says so.
            None,
            // Cloned, because `masm.lowered` must publish the very sources `masm.parsed`
            // did, unchanged. This mirrors the clone the legacy stage made.
            Some(ProjectSourceInputs {
                root: inputs.root.clone(),
                support: inputs.support.clone(),
            }),
            &config,
            context.clone(),
        )?;
        crate::emit_hir_if_requested(world.world.borrow().as_operation(), context.clone())?;

        let analysis_manager =
            midenc_hir::pass::AnalysisManager::new(world.world.as_operation_ref(), None);
        let analysis =
            analysis_manager.get_analysis::<midenc_dialect_hir::analyses::AdviceTaintAnalysis>()?;
        let source_manager = context.source_manager();
        for diagnostic in analysis.diagnostics(&source_manager) {
            session.diagnostics.emit(diagnostic);
        }
        if session.diagnostics.has_errors() {
            return Err(Report::msg(crate::pipeline::lint_errors_reported(&session.options)));
        }

        Ok(world)
    }
}

impl Frontend for MasmProjectFrontend {
    /// Parse this target's sources, optionally lint them, and hand them on for assembly.
    ///
    /// The sources come from [`MasmSourceProvider::provide_sources`] and are published
    /// unchanged at both `masm.parsed` and `masm.lowered`: there is no lowering step for a
    /// target that is already Miden Assembly, and the second checkpoint exists so that a
    /// route-wide `--stop-after=lower` means the same thing whatever the source language.
    ///
    /// `hir.analyzed` sits between them on the route but is only *reached* when something asks
    /// for it, and then only for the root target. The root gate is a correctness requirement:
    /// `analyze` disassembles `session.project`, which is the *root* project no matter which
    /// target the callback is for, so running it for a MASM dependency of a Rust project would
    /// disassemble the wrong project — and would pay for a full disassembly once per
    /// dependency.
    ///
    /// # Why `-Canalyze-only` runs the analysis, and not only `-Zlint`
    ///
    /// The lint gate is the legacy stage's, and on its own it is not enough now that
    /// `-Canalyze-only` resolves to a goal at `hir.analyzed`: a run that asked to stop there
    /// and never published it would sail past the point it was told to stop at, and the driver
    /// would report that — correctly — as an internal error.
    ///
    /// So the flag turns the analysis on. That is what it means: *run up to and including the
    /// analysis step, then exit*. This route is the only one where the two questions coincide,
    /// because here the analysis **is** the lint; the HIR-producing routes reach
    /// [`backend::analyze`](crate::pipeline::backend::analyze), which is a step that always
    /// runs and whose lints are separately gated on `-Zlint`.
    ///
    /// The visible consequence is that `-Canalyze-only` alone now reports advice-taint findings
    /// for a Miden Assembly target, where before it silently built the whole package. **And
    /// those findings can fail the build.** They are `Severity::Warning`, so ordinarily the run
    /// stops cleanly at `hir.analyzed` and exits zero — but under `-Dwarnings` they promote to
    /// errors, `has_errors()` trips, and `analyze` returns an ordinary [`Report`] rather than
    /// reaching the checkpoint at all. That is the correct outcome (a lint error must not exit
    /// zero, which is the conflation the legacy stage made), and it is why
    /// [`lint_errors_reported`](crate::pipeline) takes the options: the summary line has to name
    /// `-Canalyze-only` on such a run, not the `-Zlint` the user never passed.
    fn compile(&self, cx: &TargetContext<'_>) -> CompilerResult<Flow<ProjectSourceInputs>> {
        let inputs = self.provide(cx)?;
        let inputs = match cx.checkpoint(CheckpointId::MASM_PARSED, ArtifactId::MASM, inputs)? {
            Flow::Continue(inputs) => inputs,
            Flow::Break(stopped) => return Ok(Flow::Break(stopped)),
        };

        let session = cx.session();
        if (session.options.lint || session.options.analyze_only) && cx.role().is_root() {
            let world = self.analyze(&inputs, cx)?;
            if let Flow::Break(stopped) =
                cx.checkpoint(CheckpointId::HIR_ANALYZED, ArtifactId::HIR, world)?
            {
                return Ok(Flow::Break(stopped));
            }
        }

        cx.checkpoint(CheckpointId::MASM_LOWERED, ArtifactId::MASM, inputs)
    }

    /// This target's build provenance, as the assembler's own provider computes it.
    ///
    /// Note the asymmetry with [`Frontend::compile`], which is deliberate: that one grew a
    /// branch for a stdin-backed input, and this one did not. It cannot be reached for one. A
    /// synthesized project has no manifest path, which makes its dependency-graph node a
    /// `ProjectSource::Virtual`, and `build_source_provenance` returns `None` for those without
    /// consulting any provider (`miden-assembly/src/project/dependency_graph.rs`, the
    /// `ProjectSource::Virtual` arm) — while a stdin-backed input is only ever a *standalone*
    /// one. So this serves manifest-backed MASM targets, every one of which has a path on disk,
    /// and delegating verbatim stays correct.
    ///
    /// Were that to change, this would fail loudly rather than quietly: the upstream provider
    /// reads `resolved_target_root`, which for a stdin target names a file nobody wrote.
    fn provenance(&self, cx: &TargetContext<'_>) -> CompilerResult<ProjectSourceProvenanceInputs> {
        self.sources.provide_source_provenance(cx.assembly())
    }
}

#[cfg(test)]
mod tests {
    use alloc::{
        boxed::Box,
        rc::Rc,
        string::{String, ToString},
        sync::Arc,
        vec,
        vec::Vec,
    };
    use core::cell::RefCell;

    use miden_assembly::ast::ModuleKind;
    use midenc_hir::Context;
    use midenc_session::{
        InputFile, Options, OutputFile, OutputType, OutputTypeSpec, OutputTypes, Session,
        diagnostics::{DefaultSourceManager, SourceManager},
        miden_project::TargetType,
    };

    use super::*;
    use crate::pipeline::{
        FrontendRegistry, Goal, Observer, Outcome, RequestState, TargetRole,
        testing::{self, VirtualProject},
    };

    // ---------------------------------------------------------------------------------------
    // Fixtures.
    // ---------------------------------------------------------------------------------------

    /// The root module of every fixture project below.
    ///
    /// It declares a submodule, so a source path that merely parses the root file would
    /// produce an empty `support` where upstream's walk of the module tree produces one
    /// entry. Nothing in it touches the advice provider, so the lint below has nothing to
    /// complain about and cannot stop the compilation for reasons unrelated to the test.
    const ROOT: &str =
        "pub mod support\n\npub proc entry() -> u32\n    push.1\n    exec.support::clean\nend\n";

    /// The submodule [`ROOT`] declares.
    const SUPPORT: &str = "pub proc clean\n    push.1\n    u32wrapping_add\nend\n";

    /// A root module the advice-taint lint *does* complain about.
    ///
    /// Modelled directly on `tests/fixtures/masm/cross_module_advice_taint`: `adv_push` obtains
    /// unconstrained advice data, which [`DIRTY_SUPPORT`] then consumes as a constrained value.
    /// The finding is a `Severity::Warning`, so it fails a build only under
    /// warnings-as-errors — which is exactly the case
    /// [`analyze_only_alone_does_not_blame_a_flag_the_user_never_passed`] needs.
    ///
    /// The two operand-stack values matter: `u32wrapping_add` consumes two, and `adv_push`
    /// supplies one, so folding this into a single procedure fails disassembly with a stack
    /// underflow before the lint ever runs.
    const DIRTY_ROOT: &str = "pub mod support\n\npub proc entry() -> u32\n    adv_push\n    \
                              exec.support::consume\nend\n";

    /// The submodule [`DIRTY_ROOT`] taints.
    const DIRTY_SUPPORT: &str = "pub proc consume\n    push.1\n    u32wrapping_add\nend\n";

    /// The root module the standard-input fixtures are handed as bytes.
    ///
    /// Deliberately unlike [`ROOT`]: it declares no submodule, and its procedure has a name
    /// nothing on disk carries — so a run that read the target root off disk instead of taking
    /// the bytes it was given cannot produce it. It has to open with a Miden Assembly top-level
    /// item, because that is what [`InputFile::from_bytes`] classifies standard input by.
    const STDIN_ROOT: &str = "pub proc from_stdin\n    push.1\nend\n";

    /// A whole MASM *program*, for the one fixture whose target is an executable.
    ///
    /// A `begin … end` body only parses as [`ModuleKind::Executable`], which is what makes it
    /// able to tell the two kind derivations apart.
    const PROGRAM: &str = "begin\n    push.1\n    drop\nend\n";

    /// Materialize a MASM library project named `name` on disk.
    ///
    /// Neither module declares a `namespace` of its own — the normal shape for a MASM
    /// project target, see `tests/fixtures/masm/cross_module_advice_taint` — so the root
    /// module's path can only come from the one the caller supplies to the parser. That is
    /// what `the_root_modules_path_is_the_targets_namespace` turns on.
    fn project(name: &str) -> VirtualProject {
        let root = testing::fixture_source(name, "lib.masm", ROOT);
        testing::fixture_source(name, "support.masm", SUPPORT);
        VirtualProject::new(name, &root, TargetType::Library).expect("should build project")
    }

    /// Materialize a MASM library project whose root trips the advice-taint lint.
    fn dirty_project(name: &str) -> VirtualProject {
        let root = testing::fixture_source(name, "lib.masm", DIRTY_ROOT);
        testing::fixture_source(name, "support.masm", DIRTY_SUPPORT);
        VirtualProject::new(name, &root, TargetType::Library).expect("should build project")
    }

    /// Materialize a MASM *executable* project named `name` on disk.
    ///
    /// The counterpart to [`project`] for the module-kind half of the invariant: every other
    /// fixture here is a library, and `TargetType::default()` is `Library` too, so a library
    /// fixture cannot tell a kind derived from the target from one derived from the session.
    fn executable_project(name: &str) -> VirtualProject {
        let root = testing::fixture_source(name, "main.masm", PROGRAM);
        VirtualProject::new(name, &root, TargetType::Executable).expect("should build project")
    }

    /// Prepare `<dir>/lib.masm` holding `source` as a standalone request, and lift the target
    /// preparation synthesized into a project this frontend can be run over.
    ///
    /// Both halves of the namespace arrangement in one place: `prepare_standalone` decides the
    /// target, and what comes back is what the frontend is then asked to parse — which is the
    /// only way to observe the two agreeing, or refusing to.
    ///
    /// The session is built **without** `--name` unless `configure` sets one, because that flag
    /// is what suppresses the pre-scan. The artifact name is therefore the input's file stem,
    /// `lib`.
    fn prepare_standalone_masm(
        dir: &str,
        source: &str,
        configure: impl FnOnce(&mut Options),
    ) -> (crate::pipeline::PreparedProject, VirtualProject) {
        let root = testing::fixture_source(dir, "lib.masm", source);
        let input = InputFile::from_path(&root).expect("a `.masm` file is a compiler input");
        let mut options = Box::<Options>::default();
        configure(&mut options);
        let source_manager: Arc<dyn SourceManager + Send + Sync> =
            Arc::new(DefaultSourceManager::default());
        let session = Session::new(input.clone(), options, None, source_manager)
            .expect("a source file input should open a compiler session");
        let mut registry = FrontendRegistry::new();
        registry
            .register(MASM_FRONTEND)
            .expect("the masm registration must be well-formed");

        let prepared = crate::pipeline::prepare_standalone(&input, &session, &registry)
            .expect("a `.masm` file is a standalone input this frontend handles");
        let project = VirtualProject::for_prepared_target(&prepared)
            .expect("the prepared target should assemble into a virtual project");
        (prepared, project)
    }

    /// The compiler input a `midenc <flags> -` invocation piping Miden Assembly produces.
    fn stdin_input() -> InputFile {
        InputFile::from_bytes(STDIN_ROOT.as_bytes().to_vec(), "stdin".into())
            .expect("miden assembly on standard input is a recognized compiler input")
    }

    /// A default HIR context, which is also the source of a target's session.
    fn context() -> Rc<Context> {
        Rc::new(Context::default())
    }

    /// A context whose session has `-Zlint` enabled.
    fn linting_context() -> Rc<Context> {
        context_with(|options| options.lint = true)
    }

    /// A context whose session was configured by `configure`.
    fn context_with(configure: impl FnOnce(&mut Options)) -> Rc<Context> {
        context_emitting(Default::default(), configure)
    }

    /// A context whose session emits `output_types`, and was otherwise configured by
    /// `configure`.
    fn context_emitting(
        output_types: OutputTypes,
        configure: impl FnOnce(&mut Options),
    ) -> Rc<Context> {
        let mut options = Box::new(Options::default());
        configure(&mut options);
        let options = options.with_output_types(output_types, None);
        let source_manager = Arc::new(DefaultSourceManager::default());
        let session = Session::new(InputFile::empty(), options, None, source_manager)
            .expect("should build a session");
        Rc::new(Context::new(Rc::new(session)))
    }

    /// A context whose session writes `--emit=masm` into `out_dir`.
    fn masm_emitting_context(out_dir: &std::path::Path) -> Rc<Context> {
        let output_types = OutputTypes::new([OutputTypeSpec::Typed {
            output_type: OutputType::Masm,
            path: Some(OutputFile::Directory(out_dir.to_path_buf())),
        }])
        .expect("masm is a valid output type");
        context_emitting(output_types, |_| {})
    }

    /// Every `.masm` document in `dir`, as `(file stem, contents)`, sorted by stem.
    fn masm_documents(dir: &std::path::Path) -> Vec<(String, String)> {
        let mut documents = std::fs::read_dir(dir)
            .expect("the output directory should exist")
            .map(|entry| entry.expect("should read a directory entry").path())
            .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("masm"))
            .map(|path| {
                let stem = path
                    .file_stem()
                    .and_then(|stem| stem.to_str())
                    .expect("a written document has a name")
                    .to_string();
                (stem, std::fs::read_to_string(&path).expect("should read the document"))
            })
            .collect::<Vec<_>>();
        documents.sort();
        documents
    }

    /// The parsed sources of `project`, as the frontend publishes them at `masm.parsed`.
    fn parsed_sources(project: &VirtualProject, context: Rc<Context>) -> Outcome {
        run(project, context, Goal::at(CheckpointId::MASM_PARSED))
            .captured
            .expect("stopping at masm.parsed must capture the parsed sources")
    }

    /// The declaration `MASM_FRONTEND` makes for `checkpoint`.
    fn decl_at(checkpoint: CheckpointId) -> &'static ArtifactDecl {
        MASM_FRONTEND.decl_at(checkpoint).expect("the checkpoint is on the masm route")
    }

    /// Records the checkpoint trace, and the identity of every set of sources published.
    #[derive(Default)]
    struct Trace {
        checkpoints: Vec<CheckpointId>,
        /// The address of the root [`Module`](miden_assembly::ast::Module) carried by each
        /// [`ProjectSourceInputs`] published, in order.
        ///
        /// Addresses rather than a structural comparison, because `ProjectSourceInputs` is
        /// neither `Clone` nor `PartialEq` and "the same value" is precisely the claim: a
        /// `Box` keeps its allocation when it is moved, so two publications of one
        /// `ProjectSourceInputs` report one address, while a re-parse or a clone reports
        /// two.
        roots: Vec<usize>,
    }

    impl Observer for Trace {
        fn on_checkpoint(
            &mut self,
            checkpoint: CheckpointId,
            _role: TargetRole,
            artifact: &Artifact,
        ) {
            self.checkpoints.push(checkpoint);
            if let Some(inputs) = artifact.downcast_ref::<ProjectSourceInputs>() {
                self.roots.push(core::ptr::from_ref(&*inputs.root) as usize);
            }
        }
    }

    /// What one run of the frontend did.
    struct Run {
        stopped: bool,
        trace: Vec<CheckpointId>,
        roots: Vec<usize>,
        captured: Option<Outcome>,
    }

    /// Run the MASM frontend over `project` to `goal`, within `context`'s session.
    ///
    /// The frontend comes from [`FrontendRegistration::instantiate`] rather than being
    /// constructed here, so what these tests exercise is what a caller holding only the
    /// registration would get.
    fn run(project: &VirtualProject, context: Rc<Context>, goal: Goal) -> Run {
        run_with_input(project, context, goal, None)
    }

    /// [`run`], with the request's own compiler input supplied to the target context.
    ///
    /// Only a standalone request carries one; a project target's frontend is handed `None` and
    /// reads `assembly().resolved_target_root`.
    fn run_with_input(
        project: &VirtualProject,
        context: Rc<Context>,
        goal: Goal,
        input: Option<&InputFile>,
    ) -> Run {
        let assembly = project.assembly_context().expect("assembly context");
        let observer = Rc::new(RefCell::new(Trace::default()));
        let state = RequestState::new(goal, vec![observer.clone() as Rc<RefCell<dyn Observer>>]);
        let cx = TargetContext::new(&assembly, context, input, TargetRole::Root, &state);

        let frontend = MASM_FRONTEND.instantiate(cx.session());
        let flow = frontend.compile(&cx).expect("the masm frontend should compile");
        let (trace, roots) = {
            let observed = observer.borrow();
            (observed.checkpoints.clone(), observed.roots.clone())
        };
        Run {
            stopped: flow.is_break(),
            trace,
            roots,
            captured: state.take_outcome(),
        }
    }

    // ---------------------------------------------------------------------------------------
    // The tests.
    // ---------------------------------------------------------------------------------------

    /// A full run publishes the parsed sources and then the very same sources as lowered
    /// Miden Assembly.
    #[test]
    fn the_parsed_sources_are_published_and_lowered_unchanged() {
        let project = project("masm_frontend_route");
        let run = run(&project, context(), Goal::at(CheckpointId::PACKAGE_ASSEMBLED));

        assert!(!run.stopped, "package.assembled is the orchestrator's to publish, not ours");
        assert_eq!(
            run.trace,
            vec![CheckpointId::MASM_PARSED, CheckpointId::MASM_LOWERED],
            "a MASM target is parsed and then lowered, with nothing in between when lint is off"
        );
        assert_eq!(run.roots.len(), 2, "both publications must carry `ProjectSourceInputs`");
        assert_eq!(
            run.roots[0], run.roots[1],
            "masm.lowered must publish the same sources masm.parsed did, not a re-parse"
        );
        assert!(run.captured.is_none(), "nothing may be captured before the goal is reached");
    }

    /// Stopping at `masm.parsed` captures the parsed sources, which is the only way to see
    /// that the artifact really is a `ProjectSourceInputs` and really came from the fixture.
    #[test]
    fn stopping_at_masm_parsed_captures_the_parsed_sources() {
        let project = project("masm_frontend_parsed");
        let run = run(&project, context(), Goal::at(CheckpointId::MASM_PARSED));

        assert!(run.stopped, "the goal is on this frontend's route, so it must stop there");
        assert_eq!(run.trace, vec![CheckpointId::MASM_PARSED], "lowering must not have run");

        let captured = run.captured.expect("stopping at the goal must capture an artifact");
        assert_eq!(captured.checkpoint(), CheckpointId::MASM_PARSED);
        assert_eq!(captured.artifact().id(), ArtifactId::MASM);
        let inputs = captured
            .downcast::<ProjectSourceInputs>()
            .expect("the parsed artifact must be the assembler's own source inputs");
        assert_eq!(
            inputs.support.len(),
            1,
            "the root declares one submodule, so the module tree must have been walked"
        );
    }

    /// With neither `-Zlint` nor `-Canalyze-only` the disassemble-to-HIR analysis is never
    /// reached.
    ///
    /// Asserted on the trace rather than on the absence of an error: an implementation that
    /// ran the analysis and threw the result away would raise no error either.
    #[test]
    fn without_lint_the_analysis_is_not_reached() {
        let project = project("masm_frontend_no_lint");
        let run = run(&project, context(), Goal::at(CheckpointId::PACKAGE_ASSEMBLED));

        assert!(
            !run.trace.contains(&CheckpointId::HIR_ANALYZED),
            "hir.analyzed is on the route but must only be reached when something asks for it, \
             got {:?}",
            run.trace
        );
    }

    /// With `-Zlint` it is, between the two Miden Assembly checkpoints.
    #[test]
    fn with_lint_the_analysis_runs() {
        let project = project("masm_frontend_lint");
        let full = run(&project, linting_context(), Goal::at(CheckpointId::PACKAGE_ASSEMBLED));

        assert_eq!(
            full.trace,
            vec![
                CheckpointId::MASM_PARSED,
                CheckpointId::HIR_ANALYZED,
                CheckpointId::MASM_LOWERED
            ],
            "under -Zlint the analysis runs between parsing and lowering"
        );
        assert_eq!(
            full.roots.len(),
            2,
            "the analysis must publish HIR, not a third copy of the sources"
        );
        assert_eq!(full.roots[0], full.roots[1], "and must leave the sources untouched");

        // And what it published there really is the disassembled HIR, not a placeholder that
        // happens to occupy the checkpoint.
        let stopped = run(&project, linting_context(), Goal::at(CheckpointId::HIR_ANALYZED));
        let captured = stopped.captured.expect("stopping at hir.analyzed must capture");
        assert_eq!(captured.artifact().id(), ArtifactId::HIR);
        let world = captured
            .downcast::<DisassembledWorld>()
            .expect("the analyzed artifact is what the disassembler produced");
        assert!(
            !world.module.borrow().body().is_empty(),
            "the lifted root module must hold the fixture's procedures"
        );
    }

    /// `-Canalyze-only` runs the analysis, whether or not `-Zlint` was given.
    ///
    /// The flag names a stop point, and `hir.analyzed` is where it stops — so if the analysis
    /// did not run, the goal would never be published and the build would run to completion
    /// past the point the user asked to stop at, which the driver reports as an internal
    /// error. On this route the analysis *is* the lint, so the flag has to turn it on; on the
    /// HIR-producing routes `backend::analyze` already runs unconditionally.
    #[test]
    fn analyze_only_reaches_the_analysis_without_the_lint_flag() {
        let project = project("masm_frontend_analyze_only");
        let context = context_with(|options| options.analyze_only = true);
        let run = run(&project, context, Goal::at(CheckpointId::HIR_ANALYZED));

        assert!(run.stopped, "hir.analyzed is the goal, so the frontend must stop there");
        assert_eq!(
            run.trace,
            vec![CheckpointId::MASM_PARSED, CheckpointId::HIR_ANALYZED],
            "the analysis must run and publish, with no lint flag in sight"
        );
        let captured = run.captured.expect("stopping at the goal must capture");
        assert_eq!(captured.artifact().id(), ArtifactId::HIR);
    }

    /// A lint failure reached through `-Canalyze-only` alone must not blame `-Zlint`.
    ///
    /// The findings are warnings, so under `-Dwarnings` they promote to errors and the run
    /// fails with the summary line rather than stopping cleanly at `hir.analyzed`. Before the
    /// flags became goals this command exited zero with a full package, so the message is the
    /// only thing the user has to go on — and naming a flag they never passed sends them
    /// looking for the wrong thing.
    #[test]
    fn analyze_only_alone_does_not_blame_a_flag_the_user_never_passed() {
        use midenc_session::Warnings;

        /// Compile `project` under `configure`d options, which must fail.
        ///
        /// Not `expect_err`: `Flow<ProjectSourceInputs>` is not `Debug`, because
        /// `ProjectSourceInputs` is not.
        fn failure(name: &str, configure: impl FnOnce(&mut Options)) -> Report {
            let project = dirty_project(name);
            let assembly = project.assembly_context().expect("assembly context");
            let state = RequestState::new(Goal::at(CheckpointId::HIR_ANALYZED), vec![]);
            let cx = TargetContext::for_testing(
                &assembly,
                context_with(configure),
                TargetRole::Root,
                &state,
            );
            match MASM_FRONTEND.instantiate(cx.session()).compile(&cx) {
                Err(err) => err,
                Ok(_) => panic!("a promoted advice-taint warning must fail the build"),
            }
        }

        let err = failure("masm_frontend_analyze_only_warnings_as_errors", |options| {
            options.analyze_only = true;
            options.diagnostics.warnings = Warnings::Error;
        });

        let rendered = format!("{err}");
        assert!(
            rendered.contains("-Canalyze-only"),
            "the summary must name the flag that asked for the analysis: {rendered}"
        );
        assert!(
            !rendered.contains("-Zlint"),
            "and must not name one the user never passed: {rendered}"
        );
        assert!(
            !err.is::<crate::CompilerStopped>(),
            "a lint error is a failure, not a stop: exiting zero here is the conflation the \
             legacy stage made"
        );

        // The other half: with `-Zlint` actually given, that is what the message names.
        let err = failure("masm_frontend_lint_warnings_as_errors", |options| {
            options.lint = true;
            options.diagnostics.warnings = Warnings::Error;
        });
        assert!(format!("{err}").contains("-Zlint"), "{err}");
    }

    /// And the frontend no longer ends the build itself when the flag is set.
    ///
    /// The stop belongs to the goal machinery now (`pipeline::goal`), which resolves
    /// `-Canalyze-only` to `hir.analyzed`. A second, frontend-local stop would fire *before*
    /// the checkpoint published, so nothing would be captured and the request would come back
    /// with no artifact at all.
    #[test]
    fn analyze_only_does_not_end_the_build_before_the_checkpoint() {
        let project = project("masm_frontend_analyze_only_continues");
        let context = context_with(|options| {
            options.analyze_only = true;
            options.lint = true;
        });
        // A goal past the analysis: nothing here asks the frontend to stop, so a
        // frontend-local `-Canalyze-only` exit would surface as an error from `compile`.
        let run = run(&project, context, Goal::at(CheckpointId::PACKAGE_ASSEMBLED));

        assert!(!run.stopped, "package.assembled is the orchestrator's to publish");
        assert_eq!(
            run.trace,
            vec![
                CheckpointId::MASM_PARSED,
                CheckpointId::HIR_ANALYZED,
                CheckpointId::MASM_LOWERED
            ],
            "with the goal past the analysis the flag must not stop anything"
        );
    }

    /// The analysis is skipped for a target that is not the root, whatever the lint flag
    /// says.
    ///
    /// `disassemble_project_target` is given the *root* project, so running it for a MASM
    /// dependency would disassemble the wrong project — and would do so once per dependency.
    #[test]
    fn a_non_root_target_is_not_analyzed() {
        let project = project("masm_frontend_dependency");
        let assembly = project.assembly_context().expect("assembly context");
        let observer = Rc::new(RefCell::new(Trace::default()));
        let state = RequestState::new(
            Goal::at(CheckpointId::PACKAGE_ASSEMBLED),
            vec![observer.clone() as Rc<RefCell<dyn Observer>>],
        );
        let cx = TargetContext::for_testing(
            &assembly,
            linting_context(),
            TargetRole::Dependency,
            &state,
        );

        let frontend = MASM_FRONTEND.instantiate(cx.session());
        let _ = frontend.compile(&cx).expect("a dependency target should compile");

        assert_eq!(
            observer.borrow().checkpoints,
            vec![CheckpointId::MASM_PARSED, CheckpointId::MASM_LOWERED],
            "a dependency must not be disassembled, even with -Zlint on"
        );
    }

    /// The root module's path is the target's namespace.
    ///
    /// This is the invariant `load_target_sources` enforces, and the one a source path built
    /// from `ParseMasmStage` would violate: that stage passes no module path, leaving the
    /// parser to derive one from the source.
    #[test]
    fn the_root_modules_path_is_the_targets_namespace() {
        let project = project("masm_frontend_namespace");
        let namespace = project.target().namespace.inner().clone();
        let root_path = project
            .assembly_context()
            .expect("assembly context")
            .resolved_target_root
            .clone();

        let run = run(&project, context(), Goal::at(CheckpointId::MASM_PARSED));
        let inputs = run
            .captured
            .expect("stopping at masm.parsed must capture")
            .downcast::<ProjectSourceInputs>()
            .expect("the parsed artifact must be the assembler's own source inputs");
        assert_eq!(
            inputs.root.path(),
            namespace.as_ref(),
            "the assembler rejects any root module whose path is not the target's namespace"
        );

        // And the assertion above is not vacuous. `ParseMasmStage` passes *no* module path,
        // leaving the parser to derive one from the source; for a target root that declares
        // no `namespace` of its own — the normal shape, see
        // `tests/fixtures/masm/cross_module_advice_taint` — that does not merely produce the
        // wrong path, it fails outright. Either way it cannot satisfy the assertion above,
        // so this test discriminates rather than passing for free.
        let source_manager: Arc<dyn SourceManager> = Arc::new(DefaultSourceManager::default());
        let derived = miden_assembly_syntax::parser::read_modules_from_root(
            &root_path,
            None,
            Some(ModuleKind::Library),
            source_manager,
            false,
        );
        assert!(
            !derived.is_ok_and(|(root, _)| root.path() == namespace.as_ref()),
            "the fixture must distinguish the two source paths, or this test proves nothing"
        );
    }

    /// The root module's kind comes from the *target's* type, not from the *session's*.
    ///
    /// The other half of the invariant `load_target_sources` enforces, and the other half of
    /// what `ParseMasmStage` gets wrong: that stage derives the kind from
    /// `session.options.target_type.unwrap_or_default()` (`stages/parse/masm.rs:89-93`) where
    /// upstream derives it from `target.ty`. The two are not the same thing — the session's
    /// target type describes the target being *compiled*, while every target in the graph has
    /// its own — so an implementation that passed the namespace correctly but kept
    /// `ParseMasmStage`'s kind rule would mis-parse every target whose type differs from the
    /// root's.
    ///
    /// Which is why this fixture is a **library target compiled inside a session that says
    /// `executable`** — precisely the shape a MASM library dependency of a MASM program takes,
    /// since `Session::new` sets `options.target_type` from the *root* project's manifest. A
    /// library fixture in a default session cannot show anything, because
    /// `TargetType::default()` is `Library` and the two derivations then agree by coincidence.
    ///
    /// Note that an *executable* fixture cannot show it either, for the opposite reason: the
    /// kind passed to the parser is only a hint, and `sema::analyze` overrides it with
    /// `Executable` whenever the source has a `begin … end` block
    /// (`crates/assembly-syntax/src/sema/mod.rs:168-199`). The source wins, so both
    /// derivations agree there too. See [`an_executable_target_is_parsed_as_a_program`].
    #[test]
    fn the_root_modules_kind_is_the_targets_type_not_the_sessions() {
        let project = project("masm_frontend_kind");
        assert!(project.target().is_library(), "the fixture must be a library target");

        // The two derivations must actually disagree here, or the assertion below holds
        // whichever rule is in force.
        let context = context_with(|options| options.target_type = Some(TargetType::Executable));
        assert_eq!(
            context.session().options.target_type,
            Some(TargetType::Executable),
            "the session must disagree with the target, or this test proves nothing"
        );

        let run = run(&project, context, Goal::at(CheckpointId::MASM_PARSED));
        let inputs = run
            .captured
            .expect("stopping at masm.parsed must capture")
            .downcast::<ProjectSourceInputs>()
            .expect("the parsed artifact must be the assembler's own source inputs");
        assert_eq!(
            inputs.root.kind(),
            ModuleKind::Library,
            "the kind must follow the target being assembled, not the session's target type"
        );
    }

    /// An executable target is parsed as a whole program, under the `$exec` namespace.
    ///
    /// This covers the executable shape end to end — `Target::executable`'s `$exec`
    /// namespace, a `begin … end` root — which every other fixture here, being a library,
    /// does not. It does **not** discriminate between the two kind derivations: a `begin`
    /// block forces [`ModuleKind::Executable`] whatever kind the caller passed, so both rules
    /// agree. [`the_root_modules_kind_is_the_targets_type_not_the_sessions`] is the test that
    /// discriminates.
    #[test]
    fn an_executable_target_is_parsed_as_a_program() {
        let project = executable_project("masm_frontend_executable");
        let namespace = project.target().namespace.inner().clone();

        let run = run(&project, context(), Goal::at(CheckpointId::MASM_PARSED));
        let inputs = run
            .captured
            .expect("stopping at masm.parsed must capture")
            .downcast::<ProjectSourceInputs>()
            .expect("the parsed artifact must be the assembler's own source inputs");
        assert_eq!(inputs.root.kind(), ModuleKind::Executable);
        assert_eq!(inputs.root.path(), namespace.as_ref(), "an executable roots at `$exec`");
        assert!(inputs.support.is_empty(), "the program declares no submodules");
    }

    // ---------------------------------------------------------------------------------------
    // Standard input.
    // ---------------------------------------------------------------------------------------

    /// A stdin-backed input is compiled from the bytes it carries, not from a path.
    ///
    /// This is the one case a path cannot serve: the input exists only in memory, and the
    /// target root synthesized for it names a file nobody wrote. The fixture makes that
    /// discriminating by putting a *different* module on disk at the target root, so a
    /// delegation to the assembler's own provider succeeds — and produces the wrong module.
    #[test]
    fn a_stdin_backed_input_is_compiled_from_the_bytes_it_carries() {
        let project = project("masm_frontend_stdin");
        let input = stdin_input();

        let run =
            run_with_input(&project, context(), Goal::at(CheckpointId::MASM_PARSED), Some(&input));
        let inputs = run
            .captured
            .expect("stopping at masm.parsed must capture")
            .downcast::<ProjectSourceInputs>()
            .expect("the parsed artifact must be the assembler's own source inputs");

        let rendered = inputs.root.to_string();
        assert!(
            rendered.contains("from_stdin"),
            "the root module must be the one piped in: {rendered}"
        );
        assert!(
            !rendered.contains("proc entry"),
            "and not the one sitting at the target root on disk: {rendered}"
        );
        assert!(
            inputs.support.is_empty(),
            "bytes in memory have no directory to walk, so standard input contributes no support \
             modules"
        );
    }

    /// And the module it parses sits at the target's namespace.
    ///
    /// The legacy `ParseMasmStage::parse_masm_from_bytes` named the module after the *input*
    /// — `stdin` — which `load_target_sources` rejects out of hand.
    #[test]
    fn a_stdin_root_module_sits_at_the_targets_namespace() {
        let project = project("masm_frontend_stdin_namespace");
        let namespace = project.target().namespace.inner().clone();
        let input = stdin_input();

        let run =
            run_with_input(&project, context(), Goal::at(CheckpointId::MASM_PARSED), Some(&input));
        let inputs = run
            .captured
            .expect("stopping at masm.parsed must capture")
            .downcast::<ProjectSourceInputs>()
            .expect("the parsed artifact must be the assembler's own source inputs");

        assert!(
            inputs.root.to_string().contains("from_stdin"),
            "the module under test must be the one piped in, or this asserts nothing about the \
             branch that parses it"
        );
        assert_eq!(
            inputs.root.path(),
            namespace.as_ref(),
            "the assembler rejects any root module whose path is not the target's namespace, \
             whichever source the bytes came from"
        );
    }

    /// And its kind comes from the target's type, not the session's.
    ///
    /// The same invariant [`the_root_modules_kind_is_the_targets_type_not_the_sessions`] pins
    /// for the path, restated for the bytes: `parse_masm_from_bytes` derived the kind from
    /// `session.options.target_type` too, and this branch is new code that could reproduce it.
    #[test]
    fn a_stdin_root_modules_kind_is_the_targets_type_not_the_sessions() {
        let project = project("masm_frontend_stdin_kind");
        assert!(project.target().is_library(), "the fixture must be a library target");
        let context = context_with(|options| options.target_type = Some(TargetType::Executable));
        let input = stdin_input();

        let run =
            run_with_input(&project, context, Goal::at(CheckpointId::MASM_PARSED), Some(&input));
        let inputs = run
            .captured
            .expect("stopping at masm.parsed must capture")
            .downcast::<ProjectSourceInputs>()
            .expect("the parsed artifact must be the assembler's own source inputs");

        assert!(
            inputs.root.to_string().contains("from_stdin"),
            "the module under test must be the one piped in, or this asserts nothing about the \
             branch that parses it"
        );
        assert_eq!(
            inputs.root.kind(),
            ModuleKind::Library,
            "the kind must follow the target being assembled, not the session's target type"
        );
    }

    /// A *file*-backed input is still served by the assembler's own provider.
    ///
    /// The discriminating half: a frontend that answered every supplied input from memory, or
    /// that simply re-read the input path itself, would lose the module tree — the submodule
    /// this fixture's root declares only arrives because `read_modules_from_root` walked for
    /// it.
    #[test]
    fn a_file_backed_input_is_still_read_through_the_upstream_provider() {
        let project = project("masm_frontend_file_input");
        let root_path = project
            .assembly_context()
            .expect("assembly context")
            .resolved_target_root
            .clone();
        let input = InputFile::from_path(&root_path).expect("a `.masm` file is a compiler input");

        let run =
            run_with_input(&project, context(), Goal::at(CheckpointId::MASM_PARSED), Some(&input));
        let inputs = run
            .captured
            .expect("stopping at masm.parsed must capture")
            .downcast::<ProjectSourceInputs>()
            .expect("the parsed artifact must be the assembler's own source inputs");

        assert_eq!(
            inputs.support.len(),
            1,
            "the root declares one submodule, so the module tree must still have been walked"
        );
    }

    /// Preparation and this frontend must agree about the namespace.
    ///
    /// The two halves are decided in different places and by different rules — preparation
    /// synthesizes the target from a scan of the root file, and the assembler's own provider
    /// parses that root with `Some(target.namespace)`, which semantic analysis then compares
    /// against the declaration it finds there. This is the only test that runs both, and so the
    /// oracle for the whole arrangement: preparation that missed the declaration would
    /// synthesize `::lib` from the artifact name, and the parse below would fail outright with a
    /// namespace conflict rather than merely disagree.
    ///
    /// The three shapes are the ones whose *spelling* could diverge between the two sides.
    /// Preparation absolutizes what it scanned; semantic analysis canonicalizes the declaration
    /// and then absolutizes that. A single bare component cannot tell those apart — a
    /// multi-component path and a quoted one can, and the quoted form is what a
    /// component-rooted module carries.
    #[test]
    fn a_declared_namespace_reaches_the_target_the_frontend_is_asked_to_parse() {
        for (declared, namespace) in [
            ("declared_ns", "::declared_ns"),
            ("foo::bar", "::foo::bar"),
            ("\"miden:base/foo@1.0.0\"", "::\"miden:base/foo@1.0.0\""),
        ] {
            let source = format!("namespace {declared}\n\npub proc entry\n    push.1\nend\n");
            let dir = format!("masm_frontend_prepared_{}", declared.len());
            let (prepared, project) = prepare_standalone_masm(&dir, &source, |_| {});
            assert_eq!(
                prepared.target.namespace.inner().as_str(),
                namespace,
                "the artifact name here is the file stem, `lib`, so a target rooted at `::lib` \
                 would mean the declaration was never read"
            );

            let run = run(&project, context(), Goal::at(CheckpointId::MASM_PARSED));
            let inputs = run
                .captured
                .expect("stopping at masm.parsed must capture")
                .downcast::<ProjectSourceInputs>()
                .expect("the parsed artifact must be the assembler's own source inputs");

            assert_eq!(
                inputs.root.path(),
                prepared.target.namespace.inner().as_ref(),
                "the namespace preparation synthesized is the one the root module must parse at"
            );
        }
    }

    /// `--name` asserts what the root must declare; it does not rewrite it.
    ///
    /// The other outcome of the arrangement above, and the one that matters most: when the flag
    /// and the file disagree the build **fails**, naming both. The flag's name implies the
    /// stronger meaning, so this is the semantics a future simplification would most plausibly
    /// "fix" — and it would do so in the direction of a build that succeeds under a namespace
    /// the source never claimed. Nothing short of driving preparation *and* the parse pins it:
    /// preparation alone succeeds here, and it is semantic analysis that refuses.
    #[test]
    fn a_name_that_disagrees_with_the_root_fails_rather_than_overriding_it() {
        let source = format!("namespace declared_ns\n\n{ROOT}");
        let (prepared, project) =
            prepare_standalone_masm("masm_frontend_prepared_named", &source, |options| {
                options.name = Some("chosen".to_string());
            });
        assert_eq!(
            prepared.target.namespace.inner().as_str(),
            "::chosen",
            "preparation passes --name through unconditionally, so this half succeeds"
        );

        let assembly = project.assembly_context().expect("assembly context");
        let state = RequestState::new(Goal::at(CheckpointId::MASM_PARSED), vec![]);
        let cx = TargetContext::for_testing(&assembly, context(), TargetRole::Root, &state);
        // Not `expect_err`: `Flow<ProjectSourceInputs>` is not `Debug`.
        let err = match MASM_FRONTEND.instantiate(cx.session()).compile(&cx) {
            Err(err) => err,
            Ok(_) => panic!(
                "a root declaring `declared_ns` must not assemble under the `::chosen` namespace \
                 --name asked for"
            ),
        };

        // The whole diagnostic, not just its summary: a namespace conflict reports as
        // "syntax error" at the top and carries the two paths in its labels.
        let rendered = format!(
            "{}",
            midenc_session::diagnostics::reporting::PrintDiagnostic::new_without_color(&err)
        );
        assert!(
            rendered.contains("::chosen"),
            "the failure must name the namespace that was asserted: {rendered}"
        );
        assert!(
            rendered.contains("declared_ns"),
            "and the one the source declares, or the disagreement is unreadable: {rendered}"
        );
    }

    /// Provenance reports this target's sources, without publishing anything.
    #[test]
    fn provenance_reports_the_targets_sources_without_publishing() {
        let project = project("masm_frontend_provenance");
        let assembly = project.assembly_context().expect("assembly context");
        let observer = Rc::new(RefCell::new(Trace::default()));
        let state = RequestState::new(
            Goal::at(CheckpointId::MASM_PARSED),
            vec![observer.clone() as Rc<RefCell<dyn Observer>>],
        );
        let cx = TargetContext::for_testing(&assembly, context(), TargetRole::Root, &state);

        let frontend = MASM_FRONTEND.instantiate(cx.session());
        let provenance = frontend.provenance(&cx).expect("provenance should succeed");

        assert_eq!(&*provenance.root.content, ROOT);
        assert_eq!(provenance.support.len(), 1, "the submodule contributes to the build too");
        assert_eq!(&*provenance.support[0].content, SUPPORT);
        assert!(
            observer.borrow().checkpoints.is_empty(),
            "provenance must not publish checkpoints, even at the goal"
        );
    }

    // ---------------------------------------------------------------------------------------
    // Rendering.
    // ---------------------------------------------------------------------------------------

    /// `masm.parsed` writes one document per module, as the standalone `.masm` path does.
    #[test]
    fn the_parsed_masm_is_rendered_as_one_document_per_module() {
        let project = project("masm_render_documents");
        let out_dir = testing::fixture_dir("masm_render_documents_out");
        let context = masm_emitting_context(&out_dir);
        let captured = parsed_sources(&project, context.clone());

        (decl_at(CheckpointId::MASM_PARSED).render)(captured.artifact(), context.session())
            .expect("rendering the parsed sources should succeed");

        let documents = masm_documents(&out_dir);
        assert_eq!(
            documents.len(),
            2,
            "the module tree is written one document per module, not concatenated into one: {:?}",
            documents.iter().map(|(stem, _)| stem).collect::<Vec<_>>()
        );
        assert!(
            documents.iter().any(|(_, body)| body.contains("proc entry")),
            "the root module must be written: {documents:?}"
        );
        assert!(
            documents.iter().any(|(_, body)| body.contains("proc clean")),
            "and the submodule alongside it, in its own document: {documents:?}"
        );
        assert!(
            documents.iter().all(|(stem, _)| stem.contains("masm_render_documents")),
            "each document is named after its own module's path: {documents:?}"
        );
    }

    /// Whether anything is written is the session's decision, not the renderer's.
    ///
    /// The observer renders at *every* root `masm.parsed`, so nothing upstream of here
    /// consults `--emit`; [`Session::emit`]'s own `should_emit` check is the whole gate. The
    /// two halves differ only in the session, and write to the same directory, so a renderer
    /// that had picked its own destination or its own condition would produce the same result
    /// twice.
    #[test]
    fn whether_masm_is_written_is_the_sessions_decision_not_the_renderers() {
        let project = project("masm_render_gate");
        let out_dir = testing::fixture_dir("masm_render_gate_out");
        let render = decl_at(CheckpointId::MASM_PARSED).render;

        let unrequested = context_emitting(Default::default(), |_| {});
        let captured = parsed_sources(&project, unrequested.clone());
        render(captured.artifact(), unrequested.session())
            .expect("rendering with nothing requested should still succeed");
        assert!(
            masm_documents(&out_dir).is_empty(),
            "a session that did not ask for masm must come away with none"
        );

        let requested = masm_emitting_context(&out_dir);
        let captured = parsed_sources(&project, requested.clone());
        render(captured.artifact(), requested.session())
            .expect("rendering with masm requested should succeed");
        assert_eq!(
            masm_documents(&out_dir).len(),
            2,
            "and the same call, in a session that did, must write the target's modules"
        );
    }

    /// `masm.lowered` writes nothing, because `masm.parsed` already wrote the same modules.
    ///
    /// The two checkpoints publish the very same [`ProjectSourceInputs`] — see
    /// [`the_parsed_sources_are_published_and_lowered_unchanged`] — so declaring the renderer
    /// at both would emit each module twice: harmless when the destination is a file, and
    /// plainly wrong when it is stdout.
    #[test]
    fn masm_lowered_writes_nothing_because_masm_parsed_already_did() {
        let project = project("masm_render_lowered");
        let out_dir = testing::fixture_dir("masm_render_lowered_out");
        let context = masm_emitting_context(&out_dir);
        let captured = parsed_sources(&project, context.clone());

        (decl_at(CheckpointId::MASM_LOWERED).render)(captured.artifact(), context.session())
            .expect("the lowered declaration renders nothing, successfully");

        assert!(
            masm_documents(&out_dir).is_empty(),
            "masm is emitted once per run, at the checkpoint that first produced it"
        );
    }

    /// An artifact of the wrong shape is reported rather than silently skipped.
    #[test]
    fn rendering_an_artifact_that_is_not_project_sources_is_an_error() {
        let context = context();
        let artifact = Artifact::new(ArtifactId::MASM, String::from("not sources"));

        let err = (decl_at(CheckpointId::MASM_PARSED).render)(&artifact, context.session())
            .expect_err("the masm renderer only knows how to write project source inputs");

        assert!(format!("{err}").contains("masm"), "the report must name the output: {err}");
    }

    /// The registration is well-formed: every route checkpoint has an artifact, every alias
    /// names a route checkpoint, and the extension dispatches to it.
    #[test]
    fn the_registration_is_accepted_by_the_registry() {
        let mut registry = FrontendRegistry::new();
        registry
            .register(MASM_FRONTEND)
            .expect("the masm registration must be well-formed");

        let found = registry.for_extension("masm").expect("dispatch is by target-root extension");
        assert_eq!(found.id(), FrontendId::new("masm"));
        assert_eq!(found.terminal(), CheckpointId::PACKAGE_ASSEMBLED);
        assert_eq!(found.resolve_alias("analyze"), Some(CheckpointId::HIR_ANALYZED));
        assert_eq!(found.artifact_at(CheckpointId::HIR_ANALYZED), Some(ArtifactId::HIR));
        assert_eq!(found.artifact_at(CheckpointId::MASM_LOWERED), Some(ArtifactId::MASM));
    }
}
