#![no_std]
#![deny(warnings)]

#[macro_use]
extern crate alloc;
#[cfg(feature = "std")]
extern crate std;

#[cfg(feature = "std")]
pub mod cargo;
mod compiler;
#[cfg(feature = "std")]
pub mod pipeline;
#[cfg(feature = "std")]
pub mod rust;

use alloc::rc::Rc;

pub use midenc_hir::Context;
use midenc_hir::Op;
use midenc_session::{
    Emit, OutputMode, OutputType,
    diagnostics::{Diagnostic, Report, WrapErr, miette},
};

pub use self::{
    compiler::Compiler,
    pipeline::artifacts::{CodegenOutput, CompiledArtifact, MidenComponent},
};

pub type CompilerResult<T> = Result<T, Report>;

/// The compilation pipeline was stopped early
#[derive(Debug, thiserror::Error, Diagnostic)]
#[error("compilation was canceled by user: {0}")]
#[diagnostic()]
pub struct CompilerStopped(&'static str);

/// Run the compiler using the provided [midenc_session::Session]
pub fn compile(context: Rc<Context>) -> CompilerResult<()> {
    use midenc_hir::formatter::DisplayHex;

    log::info!(target: "driver", "starting compilation session");

    let session = context.session();
    match compile_to_memory(context.clone())? {
        CompiledArtifact::Assembled(ref package) => {
            log::info!(
                "succesfully assembled mast package '{}' with digest {}",
                package.name,
                DisplayHex::new(&package.digest().as_bytes())
            );
            session
                .emit(OutputMode::Text, package)
                .map_err(Report::msg)
                .wrap_err("failed to pretty print 'mast' artifact")?;
            let output_path = session.emit_to(OutputType::Masp, package.name());
            session
                .emit(OutputMode::Binary, package)
                .map_err(Report::msg)
                .wrap_err("failed to serialize 'mast' artifact")?;
            if let Some(output_path) = output_path {
                session.diagnostics.info(format!("Compiled {}", output_path.display()));
            }
            Ok(())
        }
        CompiledArtifact::Lowered(_) => {
            log::debug!("no outputs requested by user: pipeline stopped before assembly");
            Ok(())
        }
    }
}

/// Same as `compile`, but return compiled artifacts to the caller
pub fn compile_to_memory(context: Rc<Context>) -> CompilerResult<CompiledArtifact> {
    let session = context.session_rc();
    let input = session.input.clone().ok_or_else(|| Report::msg("no inputs"))?;
    artifact_from_outcome(
        run_pipeline(session.clone(), input, pipeline::Start::Input, None)?,
        pipeline::stop_flag(&session.options)?,
    )
}

/// Lowers previously-generated pre-link outputs of the compiler to Miden Assembly/MAST, running
/// `pre_assembly_stage` against the lowered Miden Assembly just before it is assembled.
///
/// Returns the compiled artifact, just like `compile_to_memory` would.
///
/// # What the callback may and may not do
///
/// It *observes*. The legacy chain took an `FnMut(CodegenOutput, ..) -> CodegenOutput` slotted
/// between two stages, so in principle it could substitute what got assembled; no caller ever
/// did, and the pipeline has no such slot — the lowered target travels inside the assembler's
/// own callback. What it does have is a hook at that point, so the callback is handed a
/// [`LoweredTarget`](pipeline::backend::LoweredTarget) by reference and returns only success or
/// failure. A failure fails the compilation, as returning `Err` from the old stage did.
///
/// The `'static` bound comes with it, for the reason
/// [`PreAssemblyHook`](pipeline::PreAssemblyHook) sets out: the hook reaches the frontend
/// through a boxed source provider. A caller that needs to get something out of the callback
/// shares an `Rc<RefCell<_>>` with it rather than borrowing a local.
///
/// # The caller's HIR context must outlive this call
///
/// `link_output` is HIR, and HIR operations hold only a raw pointer to the context they were
/// allocated in. The context is recovered from the component here and held for the duration of
/// the request, so a caller need only keep its own handle alive *up to* this call — but it must
/// do that much. See [`pipeline::Start::At`].
pub fn compile_link_output_to_masm_with_pre_assembly_stage<F>(
    link_output: MidenComponent,
    pre_assembly_stage: F,
) -> CompilerResult<CompiledArtifact>
where
    F: FnMut(&pipeline::backend::LoweredTarget) -> CompilerResult<()> + 'static,
{
    use alloc::rc::Rc as AllocRc;
    use core::cell::RefCell;

    let context = link_output.world.borrow().as_operation().context_rc();
    let session = context.session_rc();
    let input = session.input.clone().ok_or_else(|| Report::msg("no inputs"))?;
    // `hir.transformed`, not `hir.initial`: this entry point *lowers* HIR the caller already
    // holds, and its callers hand over HIR that has either been rewritten already
    // by the caller, or is built by hand and must not be (`tests/support`'s `eval.rs` builds
    // components programmatically). Seeding at `hir.initial` would re-run analysis and the
    // rewrites over both,
    // which is not a wasted pass but a miscompile — the greedy pipeline is not idempotent, and a
    // second run produced `NoSolution` operand-scheduling failures in five 128-bit arithmetic
    // fixtures and two differential proptests. It is also what the legacy
    // `CodegenStage → AssembleStage` chain did: codegen and nothing before it.
    let start = pipeline::Start::At {
        checkpoint: pipeline::CheckpointId::HIR_TRANSFORMED,
        artifact: pipeline::Artifact::new(pipeline::ArtifactId::HIR, link_output),
    };
    let hook = AllocRc::new(RefCell::new(pre_assembly_stage)) as pipeline::PreAssemblyHook;

    let outcome = run_pipeline(session.clone(), input, start, Some(hook))?;
    // The context is dropped only now: the seed provider takes an owning handle of its own, but
    // holding this one until the request is over makes the requirement unmissable rather than
    // merely satisfied.
    drop(context);
    artifact_from_outcome(outcome, pipeline::stop_flag(&session.options)?)
}

/// Run one compilation of `input` through the pipeline, in `session`.
///
/// The single place the crate's entry points build a [`pipeline::CompilationRequest`]. No
/// outputs are requested and no observers are attached: `--emit` is *not* forwarded into the
/// request, because `Options::output_types` cannot distinguish an explicit `--emit` from the
/// implicit `masp` that `Options::with_output_types` inserts on every invocation, and
/// forwarding explicit specs would turn today's silent no-op into a usage error. What is
/// emitted is nonetheless decided by the session: `Pipeline::compile` attaches an observer that
/// renders the selected target's artifacts through the route's own declarations, and
/// `Session::emit` writes only the output types the session asked for.
fn run_pipeline(
    session: Rc<midenc_session::Session>,
    input: midenc_session::InputFile,
    start: pipeline::Start,
    pre_assembly: Option<pipeline::PreAssemblyHook>,
) -> CompilerResult<pipeline::Outcome> {
    use alloc::vec::Vec;

    let mut registry = session.package_registry()?;
    let mut request = pipeline::CompilationRequest::new(session, input)
        .with_outputs(pipeline::OutputRequest::new(Vec::new()))
        .with_start(start);
    if let Some(hook) = pre_assembly {
        request = request.with_pre_assembly(hook);
    }
    pipeline::Pipeline::with_default_frontends()?.compile(request, registry.as_mut())
}

/// Map a compilation's [`pipeline::Outcome`] onto the [`CompiledArtifact`] the entry points
/// hand back.
///
/// [`CompiledArtifact`] has only two shapes, neither of which can carry an artifact captured
/// partway along a route: `Assembled` needs the finished package and `Lowered` needs a
/// [`CodegenOutput`], while a run stopped at, say, `masm.lowered` holds `ProjectSourceInputs`.
/// So every stop short of the package is reported the way it always has been — as a
/// [`CompilerStopped`], which `midenc-driver` downcasts into exit 0 — and the captured artifact
/// is discarded. Callers that want it use the pipeline directly.
///
/// `CompiledArtifact::Lowered` is therefore unreachable from here. It survives because it is
/// part of a public enum with an external consumer, and narrowing that is a separate change.
fn artifact_from_outcome(
    outcome: pipeline::Outcome,
    stop: Option<pipeline::StopFlag>,
) -> CompilerResult<CompiledArtifact> {
    use miden_assembly_syntax::DisplayHex;

    if outcome.checkpoint() != pipeline::CheckpointId::PACKAGE_ASSEMBLED {
        // A stop with no flag behind it came from `--stop-after`, or from a caller that set the
        // stop point on the request itself; the generic reason names the flag either way.
        let reason = stop.map_or("stop-after", pipeline::StopFlag::reason);
        log::debug!("stopping compiler early at '{}' ({reason})", outcome.checkpoint());
        return Err(CompilerStopped(reason).into());
    }

    let package = outcome.into_package()?;
    log::debug!(
        "successfully assembled package with digest {}",
        DisplayHex::new(&package.digest().as_bytes())
    );
    Ok(CompiledArtifact::Assembled(package))
}

pub(crate) fn emit_hir_if_requested(
    op: &midenc_hir::Operation,
    context: Rc<Context>,
) -> CompilerResult<()> {
    use alloc::string::ToString;

    use midenc_hir::{
        OpPrintingFlags,
        diagnostics::IntoDiagnostic,
        print::{AsmPrinter, OpPrinter},
    };
    use midenc_session::OutputType;

    let session = context.session();
    if session.should_emit(OutputType::Hir) {
        let flags = OpPrintingFlags::from(context.session().options.as_ref());
        let mut printer = AsmPrinter::new(context.clone(), &flags);
        op.print(&mut printer);
        let hir_str = printer.finish().to_string();
        session.emit(OutputMode::Text, &hir_str).into_diagnostic()?;
    }

    Ok(())
}
