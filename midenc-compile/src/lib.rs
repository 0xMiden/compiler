#![no_std]

extern crate alloc;
#[cfg(feature = "std")]
extern crate std;

mod compiler;
mod stage;
mod stages;

use alloc::{rc::Rc, vec::Vec};

pub use midenc_hir::Context;
use midenc_hir::Op;
use midenc_session::{
    diagnostics::{miette, Diagnostic, Report, WrapErr},
    OutputMode,
};

pub use self::{
    compiler::Compiler,
    stages::{CodegenOutput, LinkOutput},
};
use self::{stage::Stage, stages::*};

pub type CompilerResult<T> = Result<T, Report>;

/// The compilation pipeline was stopped early
#[derive(Debug, thiserror::Error, Diagnostic)]
#[error("compilation was canceled by user")]
#[diagnostic()]
pub struct CompilerStopped;

/// Run the compiler using the provided [Session]
pub fn compile(context: Rc<Context>) -> CompilerResult<()> {
    use midenc_hir::formatter::DisplayHex;

    log::info!("starting compilation session");

    midenc_codegen_masm::register_dialect_hooks(&context);

    let session = context.session();
    match compile_inputs(session.inputs.clone(), context.clone())? {
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
    let inputs = context.session().inputs.clone();
    compile_inputs(inputs, context)
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
    let mut stages = ParseStage
        .collect(LinkStage)
        .next_optional(ApplyRewritesStage)
        .next(CodegenStage)
        .next(
            pre_assembly_stage
                as &mut (dyn FnMut(CodegenOutput, Rc<Context>) -> CompilerResult<CodegenOutput>
                          + '_),
        )
        .next(AssembleStage);

    let inputs = context.session().inputs.clone();
    stages.run(inputs, context)
}

/// Compile the current inputs without lowering to Miden Assembly.
///
/// Returns the translated pre-link outputs of the compiler's link stage.
pub fn compile_to_optimized_hir(context: Rc<Context>) -> CompilerResult<LinkOutput> {
    let mut stages = ParseStage.collect(LinkStage).next_optional(ApplyRewritesStage);

    let inputs = context.session().inputs.clone();
    stages.run(inputs, context)
}

/// Lowers previously-generated pre-link outputs of the compiler to Miden Assembly/MAST.
///
/// Returns the compiled artifact, just like `compile_to_memory` would.
pub fn compile_link_output_to_masm(link_output: LinkOutput) -> CompilerResult<Artifact> {
    let mut stages = CodegenStage.next(AssembleStage);

    let context = link_output.component.borrow().as_operation().context_rc();
    stages.run(link_output, context)
}

/// Lowers previously-generated pre-link outputs of the compiler to Miden Assembly/MAST, but with
/// an provided callback that will be used as an extra compiler stage just prior to assembly.
///
/// Returns the compiled artifact, just like `compile_to_memory` would.
pub fn compile_link_output_to_masm_with_pre_assembly_stage<F>(
    link_output: LinkOutput,
    pre_assembly_stage: &mut F,
) -> CompilerResult<Artifact>
where
    F: FnMut(CodegenOutput, Rc<Context>) -> CompilerResult<CodegenOutput>,
{
    let mut stages = CodegenStage
        .next(
            pre_assembly_stage
                as &mut (dyn FnMut(CodegenOutput, Rc<Context>) -> CompilerResult<CodegenOutput>
                          + '_),
        )
        .next(AssembleStage);

    let context = link_output.component.borrow().as_operation().context_rc();
    stages.run(link_output, context)
}

fn compile_inputs(
    inputs: Vec<midenc_session::InputFile>,
    context: Rc<Context>,
) -> CompilerResult<Artifact> {
    let mut stages = ParseStage
        .collect(LinkStage)
        .next_optional(ApplyRewritesStage)
        .next(CodegenStage)
        .next(AssembleStage);

    stages.run(inputs, context)
}
