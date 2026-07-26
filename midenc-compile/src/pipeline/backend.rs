//! The shared HIR backend, as a service frontends call.
//!
//! Frontends that produce HIR hand it here; the service runs analysis, rewrites, and
//! codegen, publishing a checkpoint after each. Frontends that produce Miden Assembly
//! directly do not call it.

use alloc::format;

use miden_assembly::ProjectSourceInputs;
use midenc_session::diagnostics::Report;

use super::{ArtifactId, CheckpointId, Flow, TargetContext};
use crate::{
    CompilerResult, MidenComponent, Stage,
    stages::{CodegenStage, ComponentAnalysisStage},
};

/// Run the shared backend over `hir`, producing assembly-ready Miden Assembly.
///
/// Publishes [`CheckpointId::HIR_ANALYZED`], [`CheckpointId::HIR_TRANSFORMED`], and
/// [`CheckpointId::MASM_LOWERED`], returning early if any of them is the goal.
pub fn hir_to_masm(
    cx: &TargetContext<'_>,
    hir: MidenComponent,
) -> CompilerResult<Flow<ProjectSourceInputs>> {
    use midenc_hir::Op;

    let context = hir.world.borrow().as_operation().context_rc();

    let hir = stage("analysis", ComponentAnalysisStage.run(hir, context.clone()))?;
    let hir = match cx.checkpoint(CheckpointId::HIR_ANALYZED, ArtifactId::HIR, hir)? {
        Flow::Continue(hir) => hir,
        Flow::Stop(stopped) => return Ok(Flow::Stop(stopped)),
    };

    let hir = stage(
        "rewrites",
        crate::stages::apply_rewrites_to_miden_component(hir, context.clone()),
    )?;
    let hir = match cx.checkpoint(CheckpointId::HIR_TRANSFORMED, ArtifactId::HIR, hir)? {
        Flow::Continue(hir) => hir,
        Flow::Stop(stopped) => return Ok(Flow::Stop(stopped)),
    };

    let codegen = stage("codegen", CodegenStage.run(hir, context))?;
    // TODO(increment-3): only `codegen.component` survives this call. `CodegenOutput` also carries
    // `account_component_metadata_bytes` and `source_provenance`, both threaded from parse through
    // `MidenComponent`; assembly consumes them to attach the account-component metadata section to
    // the package (see `attach_account_component_metadata` in `stages/assemble.rs`). Dropping them
    // here means a frontend delegating to this service would assemble a package missing that
    // metadata, and since `hir_to_masm` is a free function with no per-target state, the frontend
    // could not recover them in `Frontend::post_process` either. There is no non-test caller yet,
    // so nothing is broken today; a later increment must route both values through per-target
    // state rather than widening this function's `Flow<ProjectSourceInputs>` contract.
    let session = cx.session();
    let sources = codegen.component.source_inputs(cx.assembly().target, &session)?;
    cx.checkpoint(CheckpointId::MASM_LOWERED, ArtifactId::MASM, sources)
}

/// Convert a legacy stage's early-stop sentinel into an explicit error.
///
/// The legacy stages decide for themselves when to stop: parsing on `parse_only`, analysis
/// on `analyze_only` *or* accumulated diagnostics, rewrites on `rewrite_only()` (derived
/// from the requested output types, not from any named flag), and codegen on `link_only`.
/// Each returns `CompilerStopped` carrying its own reason. Stop policy is the backend
/// service's concern, not theirs, so for the four *flag*-driven conditions a sentinel
/// reaching here is a bug rather than a legitimate outcome — but which bug varies by stage,
/// so the sentinel's reason is preserved verbatim instead of being replaced with a guess.
///
/// One wrapped condition is *not* an internal bug: `ComponentAnalysisStage::run` (see
/// `stages/analyze.rs`) returns the same `CompilerStopped("either errors were raised, or
/// analyze-only is set")` when `-Zlint` accumulated real user diagnostics. That is a
/// user-facing failure, and this wrapper currently misreports it as
/// `internal error: legacy analysis stage stopped early: ...` — an ICE for a legitimate
/// user error. Nothing calls [`hir_to_masm`] outside tests yet, so no user can hit it today.
///
/// TODO(increment-3): wiring this service into a real driver must first route the analysis
/// stage's diagnostic-driven stop to the user as a normal compilation failure (the stage's
/// own `has_errors()` branch), distinct from the flag-driven sentinels. The stage bodies
/// then move into this module and the flag checks are deleted, at which point this wrapper
/// goes away.
fn stage<T>(name: &str, result: CompilerResult<T>) -> CompilerResult<T> {
    result.map_err(|report| {
        if report.downcast_ref::<crate::CompilerStopped>().is_some() {
            Report::msg(format!("internal error: legacy {name} stage stopped early: {report}"))
        } else {
            report
        }
    })
}

#[cfg(test)]
mod tests {
    use alloc::{rc::Rc, vec::Vec};
    use core::cell::RefCell;

    use midenc_session::miden_project::TargetType;

    use super::*;
    use crate::pipeline::{
        ArtifactId, CaptureSlot, Goal, Outcome, RecordingObserver, TargetContext, TargetRole,
        testing::{VirtualProject, wat_fixture},
    };

    fn library(name: &str) -> VirtualProject {
        let root = wat_fixture(name, "lib.wat");
        VirtualProject::new(name, &root, TargetType::Library).expect("should build")
    }

    /// Adapted from the passing test in `midenc-compile/tests/codegen_legalization.rs`.
    ///
    /// A single public `main` whose body is just `ret` — the smallest component that
    /// codegen accepts.
    fn build_component() -> (MidenComponent, Rc<midenc_hir::Context>) {
        use std::path::Path;

        use miden_assembly::{ProjectSourceProvenanceInputs, SourceFileProvenance};
        use midenc_hir::{
            BuilderExt, Context, Ident, OpBuilder, SourceSpan, Visibility,
            dialects::builtin::{
                self, BuiltinOpBuilder, ComponentBuilder, FunctionBuilder, ModuleBuilder,
                WorldBuilder, attributes::Signature,
            },
            version::Version,
        };

        let context = Rc::new(Context::default());
        let mut builder = OpBuilder::new(context.clone());
        let world = builder.create::<builtin::World, ()>(SourceSpan::UNKNOWN)().unwrap();
        let mut world_builder = WorldBuilder::new(world);
        let component = world_builder
            .define_component(
                Ident::with_empty_span("test_ns".into()),
                Ident::with_empty_span("test".into()),
                Version::new(1, 0, 0),
            )
            .unwrap();

        let mut component_builder = ComponentBuilder::new(component);
        let module =
            component_builder.define_module(Ident::with_empty_span("test".into())).unwrap();
        let signature = Signature::new(&context, [], []);
        let mut module_builder = ModuleBuilder::new(module);
        let function = module_builder
            .define_function(Ident::with_empty_span("main".into()), Visibility::Public, signature)
            .unwrap();

        let mut builder = OpBuilder::new(context.clone());
        let mut function_builder = FunctionBuilder::new(function, &mut builder);
        function_builder.ret(None, SourceSpan::UNKNOWN).unwrap();

        let component = MidenComponent {
            world,
            component: Some(component),
            account_component_metadata_bytes: None,
            source_provenance: ProjectSourceProvenanceInputs {
                root: SourceFileProvenance {
                    path: Path::new(file!()).to_path_buf().into_boxed_path(),
                    content: alloc::string::String::new().into_boxed_str(),
                },
                support: Default::default(),
            },
        };
        (component, context)
    }

    /// Run the backend to `goal`, returning the observed checkpoint trace and whatever the
    /// target captured when it stopped.
    fn run(name: &str, goal: CheckpointId) -> (Vec<CheckpointId>, Option<Outcome>) {
        let (component, context) = build_component();
        let project = library(name);
        let assembly = project.assembly_context().expect("assembly context");
        let observer = Rc::new(RefCell::new(RecordingObserver::default()));
        let capture = Rc::new(RefCell::new(CaptureSlot::default()));

        let cx = TargetContext::for_testing(
            &assembly,
            context,
            TargetRole::Root,
            Goal::at(goal),
            observer.clone(),
            capture.clone(),
        );

        let flow = hir_to_masm(&cx, component).expect("backend should succeed");
        assert!(flow.is_stop(), "the goal is on the backend's route, so it must stop there");
        let trace = observer.borrow().records().iter().map(|(c, _)| *c).collect();
        let captured = capture.borrow_mut().take();
        (trace, captured)
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

    #[test]
    fn stage_reframes_a_sentinel_and_keeps_its_reason() {
        use alloc::string::ToString;

        let sentinel: Report = crate::CompilerStopped("link-only=true").into();
        let err = stage::<()>("codegen", Err(sentinel)).expect_err("the sentinel must not pass");

        let msg = err.to_string();
        assert!(msg.contains("internal error"), "the framing must survive: {msg}");
        assert!(msg.contains("codegen"), "the stage name must be named: {msg}");
        assert!(
            msg.contains("link-only=true"),
            "the sentinel's own reason is the only accurate diagnosis, so it must survive: {msg}"
        );
    }

    #[test]
    fn stage_passes_a_genuine_error_through_unchanged() {
        use alloc::string::ToString;

        let err = stage::<()>("rewrites", Err(Report::msg("undefined symbol `foo`")))
            .expect_err("a real error must still be an error");

        let msg = err.to_string();
        assert_eq!(
            msg, "undefined symbol `foo`",
            "a non-sentinel error is not the backend's to rewrite"
        );
    }
}
