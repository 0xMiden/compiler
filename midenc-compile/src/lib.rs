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
pub mod rust;
mod stage;
pub mod stages;

use alloc::rc::Rc;

pub use midenc_hir::Context;
use midenc_hir::Op;
use midenc_session::{
    OutputMode,
    diagnostics::{Diagnostic, Report, WrapErr, miette},
};

use self::stages::*;
pub use self::{
    compiler::Compiler,
    stage::Stage,
    stages::{CodegenOutput, MidenComponent},
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
        Artifact::Assembled(ref package) => {
            log::info!(
                "succesfully assembled mast package '{}' with digest {}",
                package.name,
                DisplayHex::new(&package.digest().as_bytes())
            );
            session
                .emit(OutputMode::Text, package)
                .map_err(Report::msg)
                .wrap_err("failed to pretty print 'mast' artifact")?;
            session
                .emit(OutputMode::Binary, package)
                .map_err(Report::msg)
                .wrap_err("failed to serialize 'mast' artifact")
        }
        Artifact::Lowered(_) => {
            log::debug!("no outputs requested by user: pipeline stopped before assembly");
            Ok(())
        }
    }
}

/// Same as `compile`, but return compiled artifacts to the caller
pub fn compile_to_memory(context: Rc<Context>) -> CompilerResult<Artifact> {
    let session = context.session_rc();
    stages::run_default_pipeline(session.input.clone(), context)
}

/// Same as `compile_to_memory`, but allows registering a callback which will be used as an extra
/// compiler stage immediately after code generation and prior to assembly, if the linker was run.
pub fn compile_to_memory_with_pre_assembly_stage<F>(
    context: Rc<Context>,
    pre_assembly_stage: &mut F,
) -> CompilerResult<Artifact>
where
    F: FnMut(CodegenOutput, Rc<Context>) -> CompilerResult<CodegenOutput>,
{
    let mut stages = ParseComponentStage
        .map(stages::apply_rewrites_to_miden_component)
        .next(CodegenStage)
        .next(
            pre_assembly_stage
                as &mut (
                         dyn FnMut(CodegenOutput, Rc<Context>) -> CompilerResult<CodegenOutput> + '_
                     ),
        )
        .next(AssembleStage);

    let session = context.session_rc();
    let input = session.input.clone().ok_or_else(|| Report::msg("no inputs"))?;
    stages.run(input, context)
}

/// Compile the current inputs without lowering to Miden Assembly.
///
/// Returns the translated pre-link outputs of the compiler's link stage.
pub fn compile_to_optimized_hir(context: Rc<Context>) -> CompilerResult<MidenComponent> {
    let mut stages = ParseComponentStage.map(stages::apply_rewrites_to_miden_component);

    let session = context.session_rc();
    let input = session.input.clone().ok_or_else(|| Report::msg("no inputs"))?;
    stages.run(input, context)
}

/// Compile the current inputs without lowering to Miden Assembly and without any IR transformations.
///
/// Returns the translated pre-link outputs of the compiler's link stage.
pub fn compile_to_unoptimized_hir(context: Rc<Context>) -> CompilerResult<MidenComponent> {
    let mut stages = ParseComponentStage;

    let session = context.session_rc();
    let input = session.input.clone().ok_or_else(|| Report::msg("no inputs"))?;
    stages.run(input, context)
}

/// Lowers previously-generated pre-link outputs of the compiler to Miden Assembly/MAST.
///
/// Returns the compiled artifact, just like `compile_to_memory` would.
pub fn compile_link_output_to_masm(link_output: MidenComponent) -> CompilerResult<Artifact> {
    let mut stages = CodegenStage.next(AssembleStage);

    let context = link_output.world.borrow().as_operation().context_rc();
    stages.run(link_output, context)
}

/// Lowers previously-generated pre-link outputs of the compiler to Miden Assembly/MAST, but with
/// an provided callback that will be used as an extra compiler stage just prior to assembly.
///
/// Returns the compiled artifact, just like `compile_to_memory` would.
pub fn compile_link_output_to_masm_with_pre_assembly_stage<F>(
    link_output: MidenComponent,
    pre_assembly_stage: &mut F,
) -> CompilerResult<Artifact>
where
    F: FnMut(CodegenOutput, Rc<Context>) -> CompilerResult<CodegenOutput>,
{
    let mut stages = CodegenStage
        .next(
            pre_assembly_stage
                as &mut (
                         dyn FnMut(CodegenOutput, Rc<Context>) -> CompilerResult<CodegenOutput> + '_
                     ),
        )
        .next(AssembleStage);

    let context = link_output.world.borrow().as_operation().context_rc();
    stages.run(link_output, context)
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
