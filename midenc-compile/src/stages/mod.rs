use alloc::{format, rc::Rc};

use miden_assembly::ProjectSourceInputs;
use midenc_hir::{Context, dialects::builtin};
use midenc_session::{
    FileType, OutputMode, Session,
    diagnostics::{IntoDiagnostic, Report},
};

use super::Stage;
use crate::{CompilerResult, CompilerStopped};

mod analyze;
pub(crate) mod assemble;
mod cargo;
mod codegen;
mod parse;
mod rewrite;

pub use self::{
    analyze::{ComponentAnalysisStage, MasmAnalysisStage},
    assemble::{Artifact, AssembleProjectStage, AssembleStage},
    cargo::CargoBuildStage,
    codegen::{CodegenOutput, CodegenStage},
    parse::{
        MasmSources, MidenComponent, ParseComponentStage, ParseHirStage, ParseMasmStage,
        ParseRustStage, ParseWasmStage,
    },
    rewrite::ApplyRewritesStage,
};

pub fn run_default_pipeline(
    input: Option<midenc_session::InputFile>,
    context: Rc<Context>,
) -> CompilerResult<Artifact> {
    use midenc_session::FileType;

    let Some(input) = input else {
        return masm_project_pipeline(None, context);
    };

    match input.file_type() {
        FileType::Hir => hir_pipeline(input, context),
        FileType::Masm => masm_source_pipeline(input, context),
        FileType::Masp => Err(Report::msg("unsupported input file type '.masp'")),
        FileType::Rust => rust_pipeline(input, context),
        FileType::Toml => project_assembly_pipeline(input, context),
        FileType::Wasm | FileType::Wat => wasm_pipeline(input, context),
    }
}

/// Assemble the project `input` locates, through the frontend-neutral pipeline.
///
/// Dispatch is by the selected target's root extension, so a Rust-rooted and a
/// Miden-Assembly-rooted manifest arrive here alike; there is no special case for either.
pub(crate) fn project_assembly_pipeline(
    input: midenc_session::InputFile,
    context: Rc<Context>,
) -> CompilerResult<Artifact> {
    use alloc::vec::Vec;

    use miden_assembly_syntax::DisplayHex;

    use crate::pipeline::{CompilationRequest, OutputRequest, Pipeline};

    let session = context.session_rc();
    let mut registry = session.package_registry()?;

    // No outputs are requested and no observers attached here. `--emit` is *not* forwarded
    // into the request: `Options::output_types` cannot distinguish an explicit `--emit` from
    // the implicit `masp` that `Options::with_output_types` inserts on every invocation, and
    // forwarding explicit specs would turn today's silent no-op into a usage error — the Rust
    // route holds a single checkpoint, so any named output short of the package would fail to
    // resolve against it. Validating explicit `--emit` specs against a route belongs with the
    // CLI, which still holds its raw `Vec<OutputTypeSpec>`.
    //
    // What is emitted is nonetheless decided by the session: `Pipeline::compile` attaches an
    // observer that renders the selected target's artifacts through the route's own
    // declarations, and `Session::emit` writes only the output types the session asked for.
    let request =
        CompilationRequest::new(session, input).with_outputs(OutputRequest::new(Vec::new()));

    let outcome = Pipeline::with_default_frontends()?.compile(request, registry.as_mut())?;
    let package = outcome.into_package()?;

    log::debug!(
        "successfully assembled package with digest {}",
        DisplayHex::new(&package.digest().as_bytes())
    );

    Ok(Artifact::Assembled(package))
}

#[cfg(false)]
pub(crate) fn cargo_project_pipeline(
    input: midenc_session::InputFile,
    context: Rc<Context>,
) -> CompilerResult<Artifact> {
    let mut build_project_stage = CargoBuildStage;
    let wasm = build_project_stage.run(input, context.clone())?;
    wasm_pipeline(wasm, context)
}

pub(crate) fn cargo_project_codegen_pipeline(
    input: midenc_session::InputFile,
    filesystem_cache_dir: Option<&std::path::Path>,
    context: Rc<Context>,
) -> CompilerResult<CodegenOutput> {
    let mut build_project_stage = CargoBuildStage::new(filesystem_cache_dir);
    let wasm = build_project_stage.run(input, context.clone())?;
    wasm_codegen_pipeline(wasm, context)
}

fn hir_pipeline(
    input: midenc_session::InputFile,
    context: Rc<Context>,
) -> CompilerResult<Artifact> {
    let mut stages = ParseHirStage
        .next_optional(ApplyRewritesStage)
        .map(extract_miden_component_or_bail)
        .next(CodegenStage)
        .next(AssembleStage);

    stages.run(input, context)
}

fn wasm_pipeline(
    input: midenc_session::InputFile,
    context: Rc<Context>,
) -> CompilerResult<Artifact> {
    let mut stages = ParseWasmStage
        .next(ComponentAnalysisStage)
        .map(apply_rewrites_to_miden_component)
        .next(CodegenStage)
        .next(AssembleStage);

    stages.run(input, context)
}

fn wasm_codegen_pipeline(
    input: midenc_session::InputFile,
    context: Rc<Context>,
) -> CompilerResult<CodegenOutput> {
    let mut stages = ParseWasmStage
        .next(ComponentAnalysisStage)
        .map(apply_rewrites_to_miden_component)
        .next(CodegenStage);

    stages.run(input, context)
}

fn masm_source_pipeline(
    input: midenc_session::InputFile,
    context: Rc<Context>,
) -> CompilerResult<Artifact> {
    let mut stages = ParseMasmStage
        .map(|input, _| Ok(Some(input)))
        .next(MasmAnalysisStage)
        .next(AssembleProjectStage);

    stages.run(input, context)
}

fn masm_project_pipeline(
    input: Option<midenc_session::InputFile>,
    context: Rc<Context>,
) -> CompilerResult<Artifact> {
    use alloc::boxed::Box;
    let maybe_parse_masm_stage =
        Box::new(|input: Option<midenc_session::InputFile>, context| match input {
            Some(input) if input.file_type() == FileType::Masm => {
                let mut parse = ParseMasmStage;
                parse.run(input, context).map(Some)
            }
            _ => Ok(None),
        })
            as Box<
                dyn FnMut(
                    Option<midenc_session::InputFile>,
                    Rc<Context>,
                ) -> CompilerResult<Option<MasmSources>>,
            >;
    let mut stages = maybe_parse_masm_stage.next(MasmAnalysisStage).next(AssembleProjectStage);

    stages.run(input, context)
}

fn rust_pipeline(
    input: midenc_session::InputFile,
    context: Rc<Context>,
) -> CompilerResult<Artifact> {
    let mut parse_rust = ParseRustStage;
    let output = parse_rust.run(input, context.clone())?;
    wasm_pipeline(output, context)
}

fn ensure_world_for_operation(
    op: midenc_hir::OperationRef,
    context: Rc<Context>,
) -> CompilerResult<builtin::WorldRef> {
    use midenc_hir::{
        BuilderExt, Op, Rewriter, SourceSpan,
        patterns::{NoopRewriterListener, RewriterImpl},
    };
    if let Some(parent) = op.parent_op() {
        parent.try_downcast_op::<builtin::World>().map_err(|op| {
            Report::msg(
                format!("cannot compile a component nested under '{}'", op.borrow().name(),),
            )
        })
    } else {
        let mut builder = RewriterImpl::<NoopRewriterListener>::new(context);
        let world: builtin::WorldRef = {
            let world_builder = builder.create::<builtin::World, ()>(SourceSpan::default());
            world_builder()?
        };
        let body = world.borrow().as_operation().region(0).entry().as_block_ref();
        builder.move_op_to_end(op, body);
        Ok(world)
    }
}

pub fn apply_rewrites_to_miden_component(
    component: MidenComponent,
    context: Rc<Context>,
) -> CompilerResult<MidenComponent> {
    if context.session().parse_only() {
        log::debug!(target: "driver", "stopping compiler early (parse-only=true)");
        return Err(CompilerStopped("parse-only=true").into());
    }
    let mut rewrites = ApplyRewritesStage;
    rewrites.run(component.world.as_operation_ref(), context)?;

    Ok(component)
}

/// This function can be used as a compiler stage following [ParseHirStage] to convert the generic
/// HIR operation to a [MidenComponent], so long as it is a world, component, or module with valid
/// structure.
///
/// This stage will return an error if the HIR structure is not supported for further compilation
pub fn extract_miden_component_or_bail(
    op: midenc_hir::OperationRef,
    context: Rc<Context>,
) -> CompilerResult<MidenComponent> {
    #[cfg(feature = "std")]
    use miden_assembly::{ProjectSourceProvenanceInputs, SourceFileProvenance};

    #[cfg(feature = "std")]
    let source_provenance = ProjectSourceProvenanceInputs {
        root: SourceFileProvenance {
            path: std::path::PathBuf::from("mod.hir").into_boxed_path(),
            content: alloc::string::String::new().into_boxed_str(),
        },
        support: Default::default(),
    };

    if let Ok(world) = op.try_downcast_op::<builtin::World>() {
        Ok(MidenComponent {
            world,
            component: None,
            account_component_metadata_bytes: None,
            #[cfg(feature = "std")]
            source_provenance,
        })
    } else if let Ok(component) = op.try_downcast_op::<builtin::Component>() {
        let world = ensure_world_for_operation(op, context.clone())?;
        Ok(MidenComponent {
            world,
            component: Some(component),
            account_component_metadata_bytes: None,
            #[cfg(feature = "std")]
            source_provenance,
        })
    } else if let Ok(module) = op.try_downcast_op::<builtin::Module>() {
        if let Some(parent) = op.parent_op() {
            if let Ok(component) = parent.try_downcast_op::<builtin::Component>() {
                let world = ensure_world_for_operation(parent, context.clone())?;
                Ok(MidenComponent {
                    world,
                    component: Some(component),
                    account_component_metadata_bytes: None,
                    #[cfg(feature = "std")]
                    source_provenance,
                })
            } else if let Ok(world) = parent.try_downcast_op::<builtin::World>() {
                Ok(MidenComponent {
                    world,
                    component: None,
                    account_component_metadata_bytes: None,
                    #[cfg(feature = "std")]
                    source_provenance,
                })
            } else {
                Err(Report::msg(format!(
                    "cannot compile a module nested under '{}'",
                    parent.borrow().name(),
                )))
            }
        } else {
            let world = ensure_world_for_operation(module.as_operation_ref(), context.clone())?;
            Ok(MidenComponent {
                world,
                component: None,
                account_component_metadata_bytes: None,
                #[cfg(feature = "std")]
                source_provenance,
            })
        }
    } else {
        Err(Report::msg(format!("cannot compile a '{}' alone", op.borrow().name())))
    }
}
