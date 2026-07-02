//! The shared HIR backend, as a service frontends call.
//!
//! Frontends that produce HIR hand it here; the service runs analysis, rewrites, and
//! codegen, publishing a checkpoint after each. Frontends that produce Miden Assembly
//! directly do not call it.
//!
//! The three phases are also exposed individually — [`analyze`], [`apply_rewrites`] and
//! [`codegen`] — for callers that have no assembler and therefore no [`TargetContext`]: the
//! manifest Rust entry point's WebAssembly leg, and
//! `midenc-compile/tests/codegen_legalization.rs`. They run one phase and nothing else.
//!
//! No *stop policy* lives here, in either form. How far a build runs is the request's goal,
//! which the service answers at a [`CheckpointId`]; the `-C` stop flags are resolved into that
//! goal (see [`StopFlag`](super::StopFlag)) rather than checked at a phase boundary. A phase
//! called directly simply runs.

use alloc::{boxed::Box, rc::Rc, sync::Arc, vec::Vec};

use miden_assembly::{ProjectSourceInputs, ProjectSourceProvenanceInputs};
use midenc_codegen_masm::{LegalizeForMasm, MasmComponent, ToMasmComponent};
use midenc_dialect_hir::transforms::{Local2Reg, TransformSpills};
use midenc_dialect_scf::transforms::LiftControlFlowToSCF;
use midenc_hir::{
    Context, Op, OperationRef,
    dialects::builtin,
    pass::{AnalysisManager, IRPrintingConfig, Nesting, OpPassManager, PassManager},
    patterns::{GreedyRewriteConfig, RegionSimplificationLevel},
};
use midenc_hir_transform::{
    Canonicalizer, CommonSubexpressionElimination, SinkOperandDefs,
    SparseConditionalConstantPropagation,
};
use midenc_session::{
    OutputMode, OutputType,
    diagnostics::{IntoDiagnostic, Report},
};

use super::{ArtifactId, CheckpointId, Flow, TargetContext};
use crate::{CodegenOutput, CompilerResult, MidenComponent};

/// What lowering one target to Miden Assembly produced.
///
/// The [`sources`](LoweredTarget::sources) are what the assembler assembles; the other three
/// fields are what it cannot derive from them and the frontend must therefore keep until
/// [`Frontend::post_process`](super::Frontend::post_process):
///
/// - [`component`](LoweredTarget::component) carries the rodata segments that become the
///   assembled package's advice map. It is **not** optional, and dropping it is not a missing
///   section but a silent miscompile: the lowered code pushes each segment's commitment and
///   asks the advice provider for the data behind it, so a package assembled without the
///   advice map fails at run time, in the VM, with nothing in the build to point at.
/// - [`account_component_metadata_bytes`](LoweredTarget::account_component_metadata_bytes)
///   becomes the package's account-component metadata section.
/// - [`component_wit_bytes`](LoweredTarget::component_wit_bytes) becomes the package's
///   component WIT section.
/// - [`source_provenance`](LoweredTarget::source_provenance) is what the assembler hashes to
///   decide whether a cached build of this target is still current.
///
/// `post_process` is called with a *fresh* [`TargetContext`], so a frontend has to stash this
/// under [`TargetContext::target_key`] and read it back there; see
/// [`RustProjectFrontend`](super::frontends::rust::RustProjectFrontend), which keys its cargo
/// builds the same way. Keeping the stash in the frontend is deliberate: `hir_to_masm` is a
/// free function with no per-target state of its own, and gains none here.
pub struct LoweredTarget {
    /// The Miden Assembly to assemble, as published at [`CheckpointId::MASM_LOWERED`].
    pub sources: ProjectSourceInputs,
    /// The lowered component, whose rodata becomes the package's advice map.
    pub component: Arc<MasmComponent>,
    /// The serialized account-component metadata, if this target has any.
    pub account_component_metadata_bytes: Option<Vec<u8>>,
    /// The component's public WIT source, if this target embeds any.
    pub component_wit_bytes: Option<Vec<u8>>,
    /// The provenance of the sources this target was built from.
    pub source_provenance: ProjectSourceProvenanceInputs,
}

/// Run the shared backend over `hir`, producing assembly-ready Miden Assembly.
///
/// Publishes [`CheckpointId::HIR_ANALYZED`], [`CheckpointId::HIR_TRANSFORMED`], and
/// [`CheckpointId::MASM_LOWERED`], returning early if any of them is the goal.
///
/// The checkpoint at `masm.lowered` publishes the [`ProjectSourceInputs`] alone — that is what
/// [`ArtifactId::MASM`] means on every route — while the caller is handed the whole
/// [`LoweredTarget`], because assembly needs more than the sources. See [`LoweredTarget`].
pub fn hir_to_masm(
    cx: &TargetContext<'_>,
    hir: MidenComponent,
) -> CompilerResult<Flow<LoweredTarget>> {
    let context = hir.world.borrow().as_operation().context_rc();

    let hir = analyze(hir, context.clone())?;
    let hir = match cx.checkpoint(CheckpointId::HIR_ANALYZED, ArtifactId::HIR, hir)? {
        Flow::Continue(hir) => hir,
        Flow::Break(stopped) => return Ok(Flow::Break(stopped)),
    };

    apply_rewrites(hir.world.as_operation_ref(), context)?;
    let hir = match cx.checkpoint(CheckpointId::HIR_TRANSFORMED, ArtifactId::HIR, hir)? {
        Flow::Continue(hir) => hir,
        Flow::Break(stopped) => return Ok(Flow::Break(stopped)),
    };

    masm_from_transformed_hir(cx, hir)
}

/// Lower `hir` — already analyzed and rewritten — to assembly-ready Miden Assembly.
///
/// The tail of [`hir_to_masm`], from codegen onwards: it publishes
/// [`CheckpointId::MASM_LOWERED`] and runs the request's
/// [`PreAssemblyHook`](super::PreAssemblyHook), and nothing before it.
///
/// # Why this is a separate entry point
///
/// A request seeded at `hir.transformed` resumes from *after* the rewrites, so it must reach
/// codegen without passing through them again — and running them twice is not a wasted pass but
/// a miscompile: the greedy rewrite pipeline is not idempotent, and a second run over
/// already-rewritten IR produced `NoSolution` operand-scheduling failures in five 128-bit
/// arithmetic fixtures and two differential proptests. See [`Start::At`](super::Start::At).
///
/// Splitting it out here rather than branching inside [`hir_to_masm`] keeps the checkpoint and
/// the hook in one place: whichever entry point a target arrives through, `masm.lowered` is
/// published exactly once and the hook runs exactly once.
pub fn masm_from_transformed_hir(
    cx: &TargetContext<'_>,
    hir: MidenComponent,
) -> CompilerResult<Flow<LoweredTarget>> {
    let context = hir.world.borrow().as_operation().context_rc();
    let CodegenOutput {
        component,
        account_component_metadata_bytes,
        component_wit_bytes,
        source_provenance,
    } = codegen(hir, context)?;
    let session = cx.session();
    let sources = component.source_inputs(cx.assembly().target, &session)?;
    Ok(match cx.checkpoint(CheckpointId::MASM_LOWERED, ArtifactId::MASM, sources)? {
        Flow::Continue(sources) => {
            let lowered = LoweredTarget {
                sources,
                component,
                account_component_metadata_bytes,
                component_wit_bytes,
                source_provenance,
            };
            // After the checkpoint, so that a run stopping at `masm.lowered` does not reach it:
            // the hook is what a caller asked to see *before assembly*, and such a run has none.
            // See [`PreAssemblyHook`](super::PreAssemblyHook).
            cx.pre_assembly(&lowered)?;
            Flow::Continue(lowered)
        }
        Flow::Break(stopped) => Flow::Break(stopped),
    })
}

/// Analyze `hir`, raising whatever the enabled lints find as diagnostics.
///
/// Only `-Zlint` has anything to run today: the advice-taint analysis, whose findings are
/// emitted against the session's diagnostics handler. They are the *user's* findings, so if
/// any of them was an error the compilation fails here, with an ordinary [`Report`]. It is
/// deliberately not a [`CompilerStopped`](crate::CompilerStopped): that sentinel means "the
/// user asked to stop early", which `midenc-driver` turns into a **successful** exit, and
/// reporting a failed lint that way would exit zero on broken code.
///
/// The advice-taint lint's own diagnostics are `Severity::Warning`, so this branch is
/// reached only when something else — `-Dwarnings`, or an earlier phase — raised an error.
///
/// [`MasmProjectFrontend::analyze`](super::frontends::masm::MasmProjectFrontend) holds the
/// other live copy of this rule, for targets that are already Miden Assembly.
pub fn analyze(hir: MidenComponent, context: Rc<Context>) -> CompilerResult<MidenComponent> {
    let session = context.session();
    if session.options.lint {
        let analysis_manager = AnalysisManager::new(hir.world.as_operation_ref(), None);
        let analysis =
            analysis_manager.get_analysis::<midenc_dialect_hir::analyses::AdviceTaintAnalysis>()?;
        let source_manager = context.source_manager();
        for diagnostic in analysis.diagnostics(&source_manager) {
            session.diagnostics.emit(diagnostic);
        }
        if session.diagnostics.has_errors() {
            return Err(Report::msg(super::lint_errors_reported(&session.options)));
        }
    }

    Ok(hir)
}

/// Apply the default rewrite pipeline to the HIR rooted at `op`, and write the post-rewrite
/// `--emit=hir` document if one was asked for.
///
/// The rewrites are applied **in place**, so `op` is returned only for the caller's
/// convenience; the operation it names is the same one it named on the way in. That is why
/// this takes an [`OperationRef`] rather than a [`MidenComponent`]: what the pass manager
/// needs is an anchor, and both the world a component hangs from and a bare operation parsed
/// from `.hir` are one.
pub fn apply_rewrites(op: OperationRef, context: Rc<Context>) -> CompilerResult<OperationRef> {
    log::debug!(target: "driver", "applying rewrite passes");
    // TODO(pauls): Set up pass registration for new pass infra
    /*
    // Get all registered module rewrites and apply them in the order they appear
    let mut registered = vec![];
    let matches = context.session().matches();
    for rewrite in inventory::iter::<RewritePassRegistration<hir::Module>> {
        log::trace!("checking if flag for rewrite pass '{}' is enabled", rewrite.name);
        let flag = rewrite.name();
        if matches.try_contains_id(flag).is_ok() {
            if let Some(index) = matches.index_of(flag) {
                let is_enabled = matches.get_flag(flag);
                if is_enabled {
                    log::debug!(
                        "rewrite pass '{}' is registered and enabled",
                        rewrite.name
                    );
                    registered.push((index, rewrite.get()));
                }
            }
        }
    }
    registered.sort_unstable_by(|(a, _), (b, _)| a.cmp(b));
    */

    // Construct a pass manager with the default pass pipeline
    let ir_print_config = IRPrintingConfig::try_from(context.session().options.as_ref())?;
    let mut pm = PassManager::on::<builtin::World>(context.clone(), Nesting::Implicit)
        .enable_ir_printing(ir_print_config);

    let mut rewrite_config = GreedyRewriteConfig::default();
    rewrite_config.with_region_simplification_level(RegionSimplificationLevel::Normal);

    // Component passes
    {
        let mut component_pm = pm.nest::<builtin::Component>();
        // Function passes for module-level functions
        {
            let mut module_pm = component_pm.nest::<builtin::Module>();
            let mut func_pm = module_pm.nest::<builtin::Function>();
            func_pm.add_pass(Canonicalizer::create_with_config(&rewrite_config));
            func_pm.add_pass(Box::new(CommonSubexpressionElimination));
            func_pm.add_pass(Box::new(SparseConditionalConstantPropagation));
            func_pm.add_pass(Box::new(SinkOperandDefs));
            //func_pm.add_pass(Box::new(ControlFlowSink));
            func_pm.add_pass(Box::new(Local2Reg));
            func_pm.add_pass(Box::new(TransformSpills));
            func_pm.add_pass(Box::new(LiftControlFlowToSCF));
            // Re-run canonicalization to clean up generated structured control flow
            func_pm.add_pass(Canonicalizer::create_with_config(&rewrite_config));
            func_pm.add_pass(Box::new(SinkOperandDefs));
            func_pm.add_pass(Box::new(TransformSpills));
            //func_pm.add_pass(Box::new(ControlFlowSink));
            //func_pm.add_pass(Box::new(DeadCodeElimination));
        }
        // Function passes for component-level functions
        {
            let mut func_pm = component_pm.nest::<builtin::Function>();
            func_pm.add_pass(Canonicalizer::create_with_config(&rewrite_config));
            func_pm.add_pass(Box::new(CommonSubexpressionElimination));
            func_pm.add_pass(Box::new(SparseConditionalConstantPropagation));
            func_pm.add_pass(Box::new(SinkOperandDefs));
            //func_pm.add_pass(Box::new(ControlFlowSink));
            func_pm.add_pass(Box::new(Local2Reg));
            func_pm.add_pass(Box::new(TransformSpills));
            func_pm.add_pass(Box::new(LiftControlFlowToSCF));
            // Re-run canonicalization to clean up generated structured control flow
            func_pm.add_pass(Canonicalizer::create_with_config(&rewrite_config));
            func_pm.add_pass(Box::new(SinkOperandDefs));
            func_pm.add_pass(Box::new(TransformSpills));
            //func_pm.add_pass(Box::new(ControlFlowSink));
            //func_pm.add_pass(Box::new(DeadCodeElimination));
        }
    }

    log::trace!(target: "driver", "before rewrites: {}", op.borrow());

    // Run pass pipeline
    pm.run(op)?;

    log::trace!(target: "driver", "after rewrites: {}", op.borrow());
    log::debug!(target: "driver", "rewrites successful");

    // Emit HIR if requested.
    //
    // This is the *post*-rewrite `--emit=hir` document — the parse step writes the pre-rewrite
    // one, so a full build prints two. It belongs here, not in a caller: both the pipeline and
    // the legacy `ApplyRewritesStage` reach the rewrites through this one function, so this is
    // the only arrangement in which the document is written exactly once whichever path ran.
    // Splitting it out again would either lose it on the path that dropped it or, if copied to
    // both, print it twice.
    //
    // The corollary for the pipeline: whichever route publishes `hir.transformed` must declare
    // its artifact `unrendered`, since the document is already written here.
    crate::emit_hir_if_requested(&op.borrow(), context)?;

    Ok(op)
}

/// Lower `hir` to a Miden Assembly component.
///
/// Emits `--emit=masm` on the way past, which is the one output this phase owns: it is the
/// only point at which the lowered component exists in a form the session can write.
pub fn codegen(hir: MidenComponent, context: Rc<Context>) -> CompilerResult<CodegenOutput> {
    let MidenComponent {
        world,
        component,
        account_component_metadata_bytes,
        component_wit_bytes,
        #[cfg(feature = "std")]
        source_provenance,
    } = hir;

    log::debug!("lowering miden component to masm");

    let anchor = component.map(|c| c.as_operation_ref()).unwrap_or(world.as_operation_ref());
    legalize_for_masm(anchor, context.clone())?;

    let analysis_manager = AnalysisManager::new(anchor, None);
    let masm_component = match component {
        Some(component) => component.borrow().to_masm_component(analysis_manager).map(Box::new)?,
        None => world.borrow().to_masm_component(analysis_manager).map(Box::new)?,
    };

    let session = context.session();

    if session.should_emit(OutputType::Masm) {
        session.emit(OutputMode::Text, masm_component.as_ref()).into_diagnostic()?;
    }

    Ok(CodegenOutput {
        component: Arc::from(masm_component),
        account_component_metadata_bytes,
        component_wit_bytes,
        #[cfg(feature = "std")]
        source_provenance,
    })
}

/// Rewrite the HIR rooted at `anchor` into the subset code generation can lower.
fn legalize_for_masm(anchor: OperationRef, context: Rc<Context>) -> CompilerResult<()> {
    let ir_print_config = IRPrintingConfig::try_from(context.session().options.as_ref())?;
    let mut pm = PassManager::new(context, OpPassManager::ANY, Nesting::Implicit)
        .enable_ir_printing(ir_print_config);
    pm.add_pass(Box::new(LegalizeForMasm));
    pm.run(anchor)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use alloc::{rc::Rc, vec::Vec};
    use core::cell::RefCell;

    use midenc_session::miden_project::TargetType;

    use super::*;
    use crate::pipeline::{
        ArtifactId, Goal, Observer, Outcome, RecordingObserver, RequestState, TargetContext,
        TargetRole,
        testing::{
            VirtualProject, context_emitting, fixture_dir, hir_documents, hir_emitting_context,
            minimal_component, wat_fixture,
        },
    };

    fn library(name: &str) -> VirtualProject {
        let root = wat_fixture(name, "lib.wat");
        VirtualProject::new(name, &root, TargetType::Library).expect("should build")
    }

    /// The smallest component codegen accepts, in a default context.
    fn build_component() -> (MidenComponent, Rc<midenc_hir::Context>) {
        let context = Rc::new(midenc_hir::Context::default());
        let component = minimal_component(&context);
        (component, context)
    }

    /// A context whose session has `-Zlint` on and writes no diagnostics anywhere.
    ///
    /// Silent so that a planted diagnostic still counts towards `has_errors()` — the counter
    /// is bumped before the emitter is consulted — without printing into the test output.
    fn linting_context() -> Rc<midenc_hir::Context> {
        context_emitting(Default::default(), |options| options.lint = true)
    }

    /// Raise an error diagnostic in `context`'s session, as `-Zlint` would for a real finding.
    ///
    /// The advice-taint lint's own diagnostics are `Severity::Warning`, so a component that
    /// actually tripped the lint would *not* set `has_errors()`. Planting one is the only way
    /// to reach the branch under test.
    fn plant_error(context: &Rc<midenc_hir::Context>) {
        use midenc_session::diagnostics::Severity;

        context
            .session()
            .diagnostics
            .diagnostic(Severity::Error)
            .with_message("planted lint finding")
            .emit();
        assert!(
            context.session().diagnostics.has_errors(),
            "the planted diagnostic must count as an error, or the test proves nothing"
        );
    }

    /// Run the backend to `goal`, returning the observed checkpoint trace and whatever the
    /// target captured when it stopped.
    fn run(name: &str, goal: CheckpointId) -> (Vec<CheckpointId>, Option<Outcome>) {
        let (component, context) = build_component();
        let project = library(name);
        let assembly = project.assembly_context().expect("assembly context");
        let observer = Rc::new(RefCell::new(RecordingObserver::default()));
        let state = RequestState::new(
            Goal::at(goal),
            alloc::vec![observer.clone() as Rc<RefCell<dyn Observer>>],
        );

        let cx = TargetContext::for_testing(&assembly, context, TargetRole::Root, &state);

        let flow = hir_to_masm(&cx, component).expect("backend should succeed");
        assert!(flow.is_break(), "the goal is on the backend's route, so it must stop there");
        let trace = observer.borrow().records().iter().map(|(c, _)| *c).collect();
        (trace, state.take_outcome())
    }

    #[test]
    fn hir_to_masm_publishes_the_backend_checkpoints_in_order() {
        let (trace, captured) = run("backend_order", CheckpointId::MASM_LOWERED);
        assert_eq!(
            trace,
            alloc::vec![
                CheckpointId::HIR_ANALYZED,
                CheckpointId::HIR_TRANSFORMED,
                CheckpointId::MASM_LOWERED,
            ]
        );

        let captured = captured.expect("goal is masm.lowered, so the backend must capture there");
        assert_eq!(captured.checkpoint(), CheckpointId::MASM_LOWERED);
        assert_eq!(captured.artifact().id(), ArtifactId::MASM);
        captured
            .downcast::<ProjectSourceInputs>()
            .expect("the captured artifact must be the lowered Miden Assembly");
    }

    #[test]
    fn hir_to_masm_stops_early_at_hir_transformed() {
        let (trace, captured) = run("backend_early", CheckpointId::HIR_TRANSFORMED);
        assert_eq!(
            trace,
            alloc::vec![CheckpointId::HIR_ANALYZED, CheckpointId::HIR_TRANSFORMED],
            "codegen must not run once the goal is reached"
        );

        let captured = captured.expect("the transformed HIR should be captured");
        assert_eq!(captured.checkpoint(), CheckpointId::HIR_TRANSFORMED);
        assert_eq!(captured.artifact().id(), ArtifactId::HIR);
        captured
            .downcast::<MidenComponent>()
            .expect("the captured artifact must be HIR, not anything codegen produced");
    }

    /// Run the whole backend over `component`, in `component`'s own session, to
    /// `masm.lowered` with nothing observing.
    ///
    /// Unlike [`run`], this hands back the raw result: the tests below are about what the
    /// backend does when something goes *wrong*.
    fn run_to_completion(
        name: &str,
        context: Rc<midenc_hir::Context>,
        component: MidenComponent,
    ) -> CompilerResult<Flow<LoweredTarget>> {
        let project = library(name);
        let assembly = project.assembly_context().expect("assembly context");
        let state = RequestState::new(Goal::at(CheckpointId::MASM_LOWERED), Vec::new());
        let cx = TargetContext::for_testing(&assembly, context, TargetRole::Root, &state);
        hir_to_masm(&cx, component)
    }

    // -------------------------------------------------------------------------------------
    // The pre-assembly hook.
    //
    // What it is *for*: `masm.lowered` publishes the `ProjectSourceInputs` alone, which is what
    // `ArtifactId::MASM` means on every route — so a caller wanting the lowered component (its
    // text, its entrypoint, its rodata) cannot get it from an observer. The hook is handed the
    // whole `LoweredTarget`, at the one point it exists.
    // -------------------------------------------------------------------------------------

    /// Run the backend to `goal` with a pre-assembly hook attached, and report what it saw.
    ///
    /// The hook counts its calls and asserts the target it was handed is the lowered one, so a
    /// hook fired with an empty shell fails here rather than silently counting.
    fn run_with_pre_assembly(name: &str, goal: CheckpointId) -> usize {
        let seen = Rc::new(RefCell::new(0usize));
        let counter = seen.clone();
        let hook = Rc::new(RefCell::new(move |lowered: &LoweredTarget| {
            assert!(
                !lowered.component.modules.is_empty(),
                "the hook must be handed the lowered component, not an empty shell"
            );
            *counter.borrow_mut() += 1;
            Ok(())
        })) as crate::pipeline::PreAssemblyHook;

        let (component, context) = build_component();
        let project = library(name);
        let assembly = project.assembly_context().expect("assembly context");
        let state = RequestState::new(Goal::at(goal), Vec::new()).with_pre_assembly(Some(hook));
        let cx = TargetContext::for_testing(&assembly, context, TargetRole::Root, &state);

        let _ = hir_to_masm(&cx, component).expect("the backend should compile");
        *seen.borrow()
    }

    #[test]
    fn the_pre_assembly_hook_runs_once_for_a_target_that_goes_on_to_assemble() {
        assert_eq!(
            run_with_pre_assembly("backend_pre_assembly", CheckpointId::PACKAGE_ASSEMBLED),
            1,
            "a run that will assemble must reach the hook exactly once"
        );
    }

    #[test]
    fn a_run_that_stops_at_masm_lowered_does_not_reach_the_pre_assembly_hook() {
        // The discriminating half. "Pre-assembly" is meant literally: a run capped at
        // `masm.lowered` has no assembly for the hook to precede. A hook fired the moment
        // codegen finished would run here too, and a caller using it to observe what got
        // assembled would be told about a package that was never built.
        assert_eq!(
            run_with_pre_assembly("backend_pre_assembly_stopped", CheckpointId::MASM_LOWERED),
            0,
            "there was no assembly for the hook to precede"
        );
    }

    #[test]
    fn a_dependency_does_not_reach_the_pre_assembly_hook() {
        // The hook answers "what is about to be assembled for the target you asked about". A
        // dependency is compiled and assembled on its own account, so firing for it would hand
        // the caller Miden Assembly it never asked for and cannot tell apart from the root's.
        let seen = Rc::new(RefCell::new(0usize));
        let counter = seen.clone();
        let hook = Rc::new(RefCell::new(move |_lowered: &LoweredTarget| {
            *counter.borrow_mut() += 1;
            Ok(())
        })) as crate::pipeline::PreAssemblyHook;

        let (component, context) = build_component();
        let project = library("backend_pre_assembly_dependency");
        let assembly = project.assembly_context().expect("assembly context");
        let state = RequestState::new(Goal::at(CheckpointId::PACKAGE_ASSEMBLED), Vec::new())
            .with_pre_assembly(Some(hook));
        let cx = TargetContext::for_testing(&assembly, context, TargetRole::Dependency, &state);

        let _ = hir_to_masm(&cx, component).expect("the backend should compile");

        assert_eq!(*seen.borrow(), 0, "only the root target's Miden Assembly reaches the hook");
    }

    /// Errors raised by `-Zlint` are a compilation failure, reported as one.
    ///
    /// The defect this pins: the analysis used to signal *both* `-Canalyze-only` and
    /// "the lint found errors" with the same `CompilerStopped` sentinel, and the backend
    /// reframed every sentinel as `internal error: legacy <name> stage stopped early`. A
    /// user whose code failed the lint was therefore told the compiler had a bug.
    #[test]
    fn lint_errors_are_a_compilation_failure_not_an_internal_error() {
        use alloc::string::ToString;

        let context = linting_context();
        let component = minimal_component(&context);
        plant_error(&context);

        let err = match run_to_completion("backend_lint_errors", context, component) {
            Ok(_) => panic!("a lint that raised errors must fail the compilation"),
            Err(err) => err,
        };

        let msg = err.to_string();
        assert!(
            !msg.contains("internal error"),
            "a user's own lint errors are not an ICE: {msg}"
        );
        assert!(
            err.downcast_ref::<crate::CompilerStopped>().is_none(),
            "the run failed, so it must not be reported as a clean early stop: {msg}"
        );
        assert!(
            msg.contains("lint"),
            "the message must tell the user which check failed the build: {msg}"
        );
    }

    /// And the branch above discriminates: with `-Zlint` on but nothing raised, the same
    /// component compiles through.
    #[test]
    fn lint_without_errors_does_not_fail_the_compilation() {
        let context = linting_context();
        let component = minimal_component(&context);
        assert!(
            !context.session().diagnostics.has_errors(),
            "the fixture must raise nothing of its own, or the assertion below is vacuous"
        );

        let _ = run_to_completion("backend_lint_clean", context, component)
            .expect("a clean lint run must compile");
    }

    /// `analyze` reports lint errors at its own boundary, not just through [`hir_to_masm`].
    ///
    /// This is the entry point the standalone frontends call directly, so the contract has
    /// to hold here and not merely somewhere downstream of here.
    #[test]
    fn analyze_reports_lint_errors_as_an_ordinary_report() {
        use alloc::string::ToString;

        let context = linting_context();
        let component = minimal_component(&context);
        plant_error(&context);

        let err = match analyze(component, context) {
            Ok(_) => panic!("a lint that raised errors must fail the analysis"),
            Err(err) => err,
        };
        assert!(
            err.downcast_ref::<crate::CompilerStopped>().is_none(),
            "a failed lint is not a clean early stop: {err}"
        );
        let msg = err.to_string();
        assert!(msg.contains("lint"), "the message must name the check that failed: {msg}");
        assert!(!msg.contains("internal error"), "and must not read as an ICE: {msg}");
    }

    // -------------------------------------------------------------------------------------
    // Stop policy is not the service's. None of the four flags the legacy stages consult may
    // have come along with the bodies: the pipeline decides how far a build runs, by goal.
    // -------------------------------------------------------------------------------------

    #[test]
    fn analyze_does_not_stop_on_analyze_only() {
        let context = context_emitting(Default::default(), |options| {
            options.lint = true;
            options.analyze_only = true;
        });
        let component = minimal_component(&context);

        analyze(component, context).expect("-Canalyze-only is the caller's policy, not ours");
    }

    #[test]
    fn apply_rewrites_does_not_stop_when_no_outputs_were_requested() {
        use alloc::boxed::Box;

        use midenc_session::{
            InputFile, Options, Session, Verbosity, diagnostics::DefaultSourceManager,
        };

        // `rewrite_only()` holds when neither linking nor codegen was asked for, and
        // `with_output_types` inserts `masp` on every invocation — so the session has to be
        // built without it for the condition to be reachable at all.
        let options = Box::new(Options::default()).with_verbosity(Verbosity::Silent);
        let source_manager = alloc::sync::Arc::new(DefaultSourceManager::default());
        let session = Session::new(InputFile::empty(), options, None, source_manager)
            .expect("should build a session");
        let context = Rc::new(midenc_hir::Context::new(Rc::new(session)));
        assert!(
            context.session().rewrite_only(),
            "the fixture session must actually be rewrite-only, or this proves nothing"
        );
        let component = minimal_component(&context);

        apply_rewrites(component.world.as_operation_ref(), context)
            .expect("rewrite-only is the caller's policy, not ours");
    }

    #[test]
    fn codegen_does_not_stop_on_link_only() {
        let context = context_emitting(Default::default(), |options| options.link_only = true);
        let component = minimal_component(&context);

        codegen(component, context).expect("-Clink-only is the caller's policy, not ours");
    }

    // -------------------------------------------------------------------------------------
    // The post-rewrite `--emit=hir` document.
    //
    // It is written by `apply_rewrites`, which is the *only* place both compilation paths
    // pass through, so both emit it and neither emits it twice. These count documents on
    // disk; each frontend's `this_route_names_the_hir_emission_once` covers the half a
    // document count cannot see. See `testing::hir_documents`.
    // -------------------------------------------------------------------------------------

    /// The rewrites write the post-rewrite document.
    #[test]
    fn apply_rewrites_writes_the_post_rewrite_hir_document() {
        let out_dir = fixture_dir("backend_rewrites_emit_hir");
        let context = hir_emitting_context(&out_dir);
        let component = minimal_component(&context);

        apply_rewrites(component.world.as_operation_ref(), context)
            .expect("rewrites should succeed");

        let documents = hir_documents(&out_dir);
        assert_eq!(documents.len(), 1, "the rewrites must write the hir document: {documents:?}");
        assert!(
            documents[0].1.contains("builtin.world"),
            "and it must be the rewritten world's HIR: {}",
            documents[0].1
        );
    }

    /// And so does a whole run of the service, which is the path a standalone frontend takes.
    ///
    /// This is the regression the emission's placement exists to prevent: while it lived in
    /// `ApplyRewritesStage`, this path — `hir_to_masm` → `apply_rewrites` — wrote nothing, so
    /// a `--emit=hir` build through the pipeline would have printed one document where the
    /// legacy build printed two.
    #[test]
    fn the_pipeline_path_writes_the_post_rewrite_hir_document() {
        let out_dir = fixture_dir("backend_pipeline_emits_hir");
        let context = hir_emitting_context(&out_dir);
        let component = minimal_component(&context);

        let _ = run_to_completion("backend_emit_hir", context, component)
            .expect("the backend should compile");

        let documents = hir_documents(&out_dir);
        assert_eq!(
            documents.len(),
            1,
            "a run of the service must write the post-rewrite hir document: {documents:?}"
        );
    }

    // -------------------------------------------------------------------------------------
    // What a frontend must be able to hand to assembly.
    //
    // `hir_to_masm` publishes the lowered sources, but assembly needs two more things
    // codegen produced: the serialized account-component metadata, which becomes a package
    // section, and the `MasmComponent` itself, whose rodata segments become the package's
    // advice map. Neither is reachable from `ProjectSourceInputs`, and `Frontend::post_process`
    // runs against a *fresh* context, so both have to survive in the frontend's own per-target
    // state. These tests assert on the assembled package, because that is the only place the
    // loss would show: a frontend that dropped the component would still compile, still
    // assemble, and produce a package whose rodata the VM cannot find.
    // -------------------------------------------------------------------------------------

    /// The read-only data the fixture target's component carries.
    ///
    /// Exactly two words, so that no padding is added on the way to felts and the advice-map
    /// value can be predicted from these bytes alone.
    const RODATA: &[u8] = b"pipeline rodata fixture bytes!!!";

    /// Stands in for the serialized account-component metadata a real account component
    /// carries. Nothing validates it, so arbitrary bytes are enough to see whether the
    /// section reaches the package.
    const METADATA: &[u8] = b"account component metadata";

    /// The extension the fixture frontend claims. Deliberately not a shipped frontend's.
    const LOWERING: &str = "hirfixture";

    /// The name of the fixture project, and the namespace its component is built in.
    ///
    /// Not the namespace its *target* declares: see
    /// [`component_target_namespace`](crate::pipeline::testing::component_target_namespace).
    const TARGET_NAME: &str = "backend_roundtrip";

    /// A frontend that lowers HIR through the shared backend, as the Wasm, HIR and standalone
    /// Rust frontends do from increment 4 on.
    ///
    /// It keeps what [`hir_to_masm`] returned in a per-target map keyed by
    /// [`TargetContext::target_key`], exactly as
    /// [`RustProjectFrontend`](crate::pipeline::frontends::rust::RustProjectFrontend) keys its
    /// cargo builds, and reads it back in [`Frontend::post_process`], which the assembler
    /// calls with a context it built afresh.
    #[derive(Default)]
    struct LoweringFrontend {
        lowered: RefCell<alloc::collections::BTreeMap<crate::pipeline::TargetKey, CodegenOutput>>,
    }

    impl LoweringFrontend {
        /// The HIR this frontend pretends to have parsed.
        ///
        /// Built in [`TARGET_NAME`]'s namespace, which is what makes the root module codegen
        /// produces land on the namespace [`lowering_manifest`] declares for the target — the
        /// assembler assembles nothing otherwise.
        fn hir(cx: &TargetContext<'_>) -> MidenComponent {
            crate::pipeline::testing::component_in_namespace(
                &cx.context(),
                TARGET_NAME,
                Some(RODATA),
                Some(METADATA.to_vec()),
            )
        }
    }

    impl crate::pipeline::Frontend for LoweringFrontend {
        fn compile(
            &self,
            cx: &TargetContext<'_>,
        ) -> CompilerResult<Flow<miden_assembly::ProjectSourceInputs>> {
            let lowered = match hir_to_masm(cx, Self::hir(cx))? {
                Flow::Continue(lowered) => lowered,
                Flow::Break(stopped) => return Ok(Flow::Break(stopped)),
            };
            // Destructured rather than moved whole, so that anything assembly needs and this
            // frontend fails to keep is a compile error here rather than a missing section
            // in the package.
            let LoweredTarget {
                sources,
                component,
                account_component_metadata_bytes,
                component_wit_bytes,
                source_provenance,
            } = lowered;
            self.lowered.borrow_mut().insert(
                cx.target_key(),
                CodegenOutput {
                    component,
                    account_component_metadata_bytes,
                    component_wit_bytes,
                    source_provenance,
                },
            );
            Ok(Flow::Continue(sources))
        }

        fn post_process(
            &self,
            package: &mut miden_mast_package::Package,
            cx: &TargetContext<'_>,
        ) -> CompilerResult<()> {
            let lowered = self.lowered.borrow();
            let found = lowered.get(&cx.target_key()).ok_or_else(|| {
                Report::msg(alloc::format!(
                    "internal error: nothing was lowered for target '{}'",
                    cx.target_key().name()
                ))
            })?;
            crate::pipeline::assembly::post_process_package(
                package,
                &found.component,
                found.account_component_metadata_bytes.as_deref(),
                found.component_wit_bytes.as_deref(),
                cx.assembly().target,
                cx.assembly().package_registry,
            )
        }

        fn provenance(
            &self,
            cx: &TargetContext<'_>,
        ) -> CompilerResult<miden_assembly::ProjectSourceProvenanceInputs> {
            use miden_assembly::{ProjectSourceProvenanceInputs, SourceFileProvenance};

            Ok(ProjectSourceProvenanceInputs {
                root: SourceFileProvenance {
                    path: cx.assembly().resolved_target_root.clone(),
                    content: alloc::string::String::from("fixture").into_boxed_str(),
                },
                support: Vec::new(),
            })
        }
    }

    fn make_lowering(
        _session: alloc::rc::Rc<midenc_session::Session>,
    ) -> alloc::rc::Rc<dyn crate::pipeline::Frontend> {
        alloc::rc::Rc::new(LoweringFrontend::default())
    }

    /// Nothing on this route is rendered; these tests never emit.
    fn unrendered(
        _artifact: &crate::pipeline::Artifact,
        _session: &midenc_session::Session,
    ) -> CompilerResult<()> {
        Ok(())
    }

    /// The route a frontend that delegates to the shared backend runs.
    const LOWERING_FRONTEND: crate::pipeline::FrontendRegistration =
        crate::pipeline::FrontendRegistration::new(
            crate::pipeline::FrontendId::new("lowering-fixture"),
            &[LOWERING],
            &[
                CheckpointId::HIR_ANALYZED,
                CheckpointId::HIR_TRANSFORMED,
                CheckpointId::MASM_LOWERED,
                CheckpointId::PACKAGE_ASSEMBLED,
            ],
            &[
                ("analyze", CheckpointId::HIR_ANALYZED),
                ("rewrite", CheckpointId::HIR_TRANSFORMED),
                ("lower", CheckpointId::MASM_LOWERED),
                ("assemble", CheckpointId::PACKAGE_ASSEMBLED),
            ],
            &[
                crate::pipeline::ArtifactDecl {
                    checkpoint: CheckpointId::HIR_ANALYZED,
                    id: ArtifactId::HIR,
                    render: unrendered,
                },
                crate::pipeline::ArtifactDecl {
                    checkpoint: CheckpointId::HIR_TRANSFORMED,
                    id: ArtifactId::HIR,
                    render: unrendered,
                },
                crate::pipeline::ArtifactDecl {
                    checkpoint: CheckpointId::MASM_LOWERED,
                    id: ArtifactId::MASM,
                    render: unrendered,
                },
                crate::pipeline::ArtifactDecl {
                    checkpoint: CheckpointId::PACKAGE_ASSEMBLED,
                    id: ArtifactId::PACKAGE,
                    render: unrendered,
                },
            ],
            make_lowering,
        );

    /// Materialize a fixture project rooted at a `.hirfixture` file, returning its manifest.
    ///
    /// The root file's *contents* are never read: the fixture frontend builds its HIR itself.
    /// It only has to exist, with the extension dispatch keys on — hence the root's name is
    /// built from [`LOWERING`] rather than spelled out, so that renaming the extension cannot
    /// leave a project whose root file no longer dispatches to this route.
    fn lowering_manifest(dir: &str) -> std::path::PathBuf {
        let namespace = crate::pipeline::testing::component_target_namespace(TARGET_NAME);
        let root = alloc::format!("lib.{LOWERING}");
        crate::pipeline::testing::fixture_source(dir, &root, "fixture");
        crate::pipeline::testing::fixture_source(
            dir,
            "miden-project.toml",
            &alloc::format!(
                r#"
[package]
name = "{TARGET_NAME}"
version = "0.1.0"

[lib]
namespace = "{namespace}"
path = "{root}"
"#
            ),
        )
    }

    /// Compile the fixture project in `dir` with the lowering frontend, returning its package.
    fn assemble_through_the_backend(dir: &str) -> alloc::sync::Arc<miden_mast_package::Package> {
        use alloc::boxed::Box;

        use midenc_session::{InputFile, Options, Session, diagnostics::DefaultSourceManager};

        let manifest = lowering_manifest(dir);
        let input = InputFile::from_path(&manifest).expect("a manifest is a valid input");
        let options = Box::new(Options::default()).with_output_types(Default::default(), None);
        let source_manager = alloc::sync::Arc::new(DefaultSourceManager::default());
        let session = alloc::rc::Rc::new(
            Session::new(input.clone(), options, None, source_manager)
                .expect("the fixture project should open a session"),
        );

        let mut registry = crate::pipeline::FrontendRegistry::new();
        registry
            .register(LOWERING_FRONTEND)
            .expect("the fixture route must be consistent");
        let request = crate::pipeline::CompilationRequest::new(session, input);
        let outcome = crate::pipeline::Pipeline::new(registry)
            .compile(request, &mut miden_package_registry::NoPackageStore)
            .expect("the fixture project should build");
        outcome.into_package().expect("a full build must yield a package")
    }

    /// Everything codegen produced reaches the package, through the frontend's own state.
    ///
    /// The two assertions are of a kind but not of a severity: a missing metadata section is
    /// a package that lacks a section, while a missing rodata advice map is a package whose
    /// code pushes a commitment the advice provider has never heard of — a silent miscompile.
    #[test]
    fn a_frontend_that_lowers_through_the_backend_assembles_a_complete_package() {
        use midenc_codegen_masm::Rodata;

        let package = assemble_through_the_backend("backend_post_process");

        let advice_map = package.mast_forest().advice_map();
        assert_eq!(
            advice_map.len(),
            1,
            "the component's one rodata segment must reach the package's advice map"
        );
        let (_, value) = advice_map.iter().next().expect("the advice map has an entry");
        assert_eq!(
            value.as_ref(),
            Rodata::bytes_to_elements(RODATA).as_slice(),
            "the advice map must hold the rodata itself, keyed by its commitment"
        );

        let section = package
            .sections
            .iter()
            .find(|section| section.id == miden_mast_package::SectionId::ACCOUNT_COMPONENT_METADATA)
            .expect("the account-component metadata must reach the package as a section");
        assert_eq!(
            section.data.as_ref(),
            METADATA,
            "and it must be the metadata codegen produced, byte for byte"
        );
    }

    /// Calling [`apply_rewrites`] directly writes the same document a whole run of the service
    /// writes, byte for byte.
    ///
    /// This began life comparing the pipeline against the legacy `Stage` chain, which was the
    /// property that mattered while both existed. The chain is gone, but the *two callers* are
    /// not: [`hir_to_masm`] reaches the rewrites through [`apply_rewrites`], and so does any
    /// caller with no assembler that runs the phase directly. So the same claim still has two
    /// sides to compare, and it would still break the moment either side grew an emission of its
    /// own at a different point in the phase.
    #[test]
    fn both_callers_write_the_same_post_rewrite_document() {
        let service_dir = fixture_dir("backend_same_document_service");
        let context = hir_emitting_context(&service_dir);
        let component = minimal_component(&context);
        let _ = run_to_completion("backend_same_document", context, component)
            .expect("the backend should compile");

        let direct_dir = fixture_dir("backend_same_document_direct");
        let context = hir_emitting_context(&direct_dir);
        let component = minimal_component(&context);
        apply_rewrites(component.world.as_operation_ref(), context)
            .expect("the rewrites should succeed");

        let service = hir_documents(&service_dir);
        let direct = hir_documents(&direct_dir);
        assert_eq!(service.len(), 1, "the service path wrote {service:?}");
        assert_eq!(direct.len(), 1, "the direct path wrote {direct:?}");
        assert_eq!(
            service[0].1, direct[0].1,
            "both callers must write the very same post-rewrite HIR"
        );
    }
}
