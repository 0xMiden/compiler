//! MASM-to-HIR disassembler.

mod events;
mod infer;
mod lift;
pub mod project;
mod semantics;
mod signatures;
mod stack;
#[cfg(test)]
mod tests;

use std::{collections::BTreeMap, path::Path, rc::Rc, sync::Arc};

use miden_assembly::ProjectSourceInputs;
use miden_assembly_syntax::{
    Parse, ParseOptions,
    ast::{self, Module, ModuleKind},
    debuginfo::{SourceLanguage, SourceManager, SourceManagerExt, SourceSpan, Uri},
};
use miden_project::Project;
use midenc_hir::{Context, FunctionType, Report, Type, dialects::builtin};

/// Result type used by the MASM disassembler.
pub type Result<T> = core::result::Result<T, Report>;

/// External procedure signatures keyed by absolute MASM procedure path.
///
/// These entries are used for calls to procedures outside the module being disassembled. Project
/// disassembly can populate this from package metadata; tests or embedding tools can provide it
/// directly when they already know the callee contracts.
pub type ExternalSignatureMap = BTreeMap<Arc<ast::Path>, FunctionType>;

/// External type definitions keyed by absolute MASM type path.
///
/// These entries are used when MASM procedure signatures refer to imported types. Project
/// disassembly populates this from dependency package metadata/source exports so signatures can be
/// lowered without requiring the MASM AST resolver to load external modules.
pub type ExternalTypeMap = BTreeMap<Arc<ast::Path>, Type>;

/// Configuration for MASM disassembly.
#[derive(Default, Debug, Clone, Copy)]
pub struct DisassemblerConfig {
    /// Infer signatures for procedures whose MASM AST/package metadata does not provide one.
    ///
    /// When enabled, missing signatures are inferred from stack underflow and final stack shape.
    pub infer_missing_signatures: bool,
}

/// Result of disassembling MASM into HIR.
pub struct DisassembledWorld {
    /// The HIR context which owns the lifted world and all nested IR entities.
    pub context: Rc<Context>,
    /// The lifted HIR world.
    pub world: builtin::WorldRef,
    /// The root lifted HIR module for the selected target.
    ///
    /// This is retained as a convenience for single-module callers and existing analyses which
    /// operate on a module root. Multi-module callers should prefer walking `world`.
    pub module: builtin::ModuleRef,
    /// Procedures omitted from the HIR world in lint mode.
    pub skipped_procedures: Vec<SkippedProcedure>,
}

/// A MASM procedure skipped while building a lintable HIR world.
#[derive(Debug, Clone)]
pub struct SkippedProcedure {
    pub path: Arc<ast::Path>,
    pub span: SourceSpan,
    pub reason: String,
}

/// Disassemble a MASM file into an HIR world.
pub fn disassemble_file(
    path: impl AsRef<Path>,
    config: &DisassemblerConfig,
    context: Rc<Context>,
) -> Result<DisassembledWorld> {
    let path = path.as_ref();
    let module_path = masm_module_path_from_file(path)?;
    let module = parse_file_with_module_path(path, &module_path, context.clone())?;
    lift::lift_single_module(&module, &lift::LiftConfig::strict(config), context)
}

/// Disassemble a MASM file for linting, skipping procedures that cannot be lifted.
pub fn disassemble_file_for_lint(
    path: impl AsRef<Path>,
    config: &DisassemblerConfig,
    context: Rc<Context>,
) -> Result<DisassembledWorld> {
    let path = path.as_ref();
    let module_path = masm_module_path_from_file(path)?;
    let module = parse_file_with_module_path(path, &module_path, context.clone())?;
    lift::lift_single_module(&module, &lift::LiftConfig::lint(config), context)
}

/// Disassemble a MASM file into an HIR world, using externally-provided procedure signatures for
/// path-based invoke targets.
pub fn disassemble_file_with_external_signatures(
    path: impl AsRef<Path>,
    config: &DisassemblerConfig,
    external_signatures: &ExternalSignatureMap,
    context: Rc<Context>,
) -> Result<DisassembledWorld> {
    let path = path.as_ref();
    let module_path = masm_module_path_from_file(path)?;
    disassemble_file_with_module_path_and_external_signatures(
        path,
        module_path,
        config,
        external_signatures,
        &ExternalTypeMap::new(),
        context,
    )
}

fn disassemble_file_with_module_path_and_external_signatures(
    path: impl AsRef<Path>,
    module_path: impl AsRef<miden_assembly_syntax::Path>,
    config: &DisassemblerConfig,
    external_signatures: &ExternalSignatureMap,
    external_types: &ExternalTypeMap,
    context: Rc<Context>,
) -> Result<DisassembledWorld> {
    let path = path.as_ref();
    let module = parse_file_with_module_path(path, module_path, context.clone())?;
    lift::lift_module(
        &module,
        &lift::LiftConfig::strict(config),
        external_signatures,
        external_types,
        context,
    )
}

/// Disassemble a MASM source string into an HIR world.
pub fn disassemble_source(
    source: impl Into<String>,
    module_path: impl AsRef<miden_assembly_syntax::Path>,
    config: &DisassemblerConfig,
    context: Rc<Context>,
) -> Result<DisassembledWorld> {
    let module = parse_source_with_module_path(source, module_path, context.clone())?;
    lift::lift_single_module(&module, &lift::LiftConfig::strict(config), context)
}

/// Disassemble a MASM source string for linting, skipping procedures that cannot be lifted.
pub fn disassemble_source_for_lint(
    source: impl Into<String>,
    module_path: impl AsRef<miden_assembly_syntax::Path>,
    config: &DisassemblerConfig,
    context: Rc<Context>,
) -> Result<DisassembledWorld> {
    let module = parse_source_with_module_path(source, module_path, context.clone())?;
    lift::lift_single_module(&module, &lift::LiftConfig::lint(config), context)
}

/// Disassemble a MASM source string into an HIR world, using externally-provided procedure
/// signatures for path-based invoke targets.
pub fn disassemble_source_with_external_signatures(
    source: impl Into<String>,
    module_path: impl AsRef<miden_assembly_syntax::Path>,
    config: &DisassemblerConfig,
    external_signatures: &ExternalSignatureMap,
    context: Rc<Context>,
) -> Result<DisassembledWorld> {
    let module = parse_source_with_module_path(source, module_path, context.clone())?;
    lift::lift_module(
        &module,
        &lift::LiftConfig::strict(config),
        external_signatures,
        &ExternalTypeMap::new(),
        context,
    )
}

/// Disassemble a target from a `miden-project.toml` package manifest.
pub fn disassemble_project_target(
    project: &Project,
    target: Option<&str>,
    sources: Option<ProjectSourceInputs>,
    config: &DisassemblerConfig,
    context: Rc<Context>,
) -> Result<DisassembledWorld> {
    let inputs = if let Some(sources) = sources {
        let external_metadata = project::collect_dependency_metadata(project, &context)?;
        project::ProjectTargetInput::from_source_inputs(sources, external_metadata)
    } else {
        project::resolve_project_target(project, target, &context)?
    };
    lift::lift_project_target(inputs, &lift::LiftConfig::strict(config), context)
}

/// Disassemble a project target for linting, skipping procedures that cannot be lifted.
pub fn disassemble_project_target_for_lint(
    project: &Project,
    target: Option<&str>,
    sources: Option<ProjectSourceInputs>,
    config: &DisassemblerConfig,
    context: Rc<Context>,
) -> Result<DisassembledWorld> {
    let inputs = if let Some(sources) = sources {
        let external_metadata = project::collect_dependency_metadata(project, &context)?;
        project::ProjectTargetInput::from_source_inputs(sources, external_metadata)
    } else {
        project::resolve_project_target(project, target, &context)?
    };
    lift::lift_project_target(inputs, &lift::LiftConfig::lint(config), context)
}

/// Disassemble pre-resolved project target inputs for linting, skipping procedures that cannot be
/// lifted.
pub fn disassemble_project_target_input_for_lint(
    inputs: project::ProjectTargetInput,
    config: &DisassemblerConfig,
    context: Rc<Context>,
) -> Result<DisassembledWorld> {
    lift::lift_project_target(inputs, &lift::LiftConfig::lint(config), context)
}

/// Disassemble a target from a `miden-project.toml` package manifest.
pub fn disassemble_project_target_from_path(
    manifest_path: impl AsRef<Path>,
    target: Option<&str>,
    config: &DisassemblerConfig,
    context: Rc<Context>,
) -> Result<DisassembledWorld> {
    let target = project::resolve_project_target_from_manifest_path(
        manifest_path.as_ref(),
        target,
        &context,
    )?;
    lift::lift_project_target(target, &lift::LiftConfig::strict(config), context)
}

/// Disassemble a target from a `miden-project.toml` package manifest for linting.
pub fn disassemble_project_target_from_path_for_lint(
    manifest_path: impl AsRef<Path>,
    target: Option<&str>,
    config: &DisassemblerConfig,
    context: Rc<Context>,
) -> Result<DisassembledWorld> {
    let target = project::resolve_project_target_from_manifest_path(
        manifest_path.as_ref(),
        target,
        &context,
    )?;
    lift::lift_project_target(target, &lift::LiftConfig::lint(config), context)
}

/// Disassemble a target from a `miden-project.toml` package manifest, using a precomputed
/// dependency graph to discover external procedure signatures.
pub fn disassemble_project_target_with_dependency_graph(
    manifest_path: impl AsRef<Path>,
    target: Option<&str>,
    dependency_graph: &miden_project::ProjectDependencyGraph,
    config: &DisassemblerConfig,
    context: Rc<Context>,
) -> Result<DisassembledWorld> {
    let target = project::resolve_project_target_from_manifest_path_with_dependency_graph(
        manifest_path.as_ref(),
        target,
        dependency_graph,
        &context,
    )?;
    lift::lift_project_target(target, &lift::LiftConfig::strict(config), context)
}

/// Disassemble a parsed MASM AST module into HIR.
pub fn disassemble_module(
    module: &Module,
    config: &DisassemblerConfig,
    context: Rc<Context>,
) -> Result<DisassembledWorld> {
    lift::lift_single_module(module, &lift::LiftConfig::strict(config), context)
}

/// Disassemble a parsed MASM AST module for linting, skipping procedures that cannot be lifted.
pub fn disassemble_module_for_lint(
    module: &Module,
    config: &DisassemblerConfig,
    context: Rc<Context>,
) -> Result<DisassembledWorld> {
    lift::lift_single_module(module, &lift::LiftConfig::lint(config), context)
}

fn parse_file_with_module_path(
    path: &Path,
    module_path: impl AsRef<miden_assembly_syntax::Path>,
    context: Rc<Context>,
) -> Result<Box<Module>> {
    let source_manager = context.session().source_manager.clone();
    let source_file = source_manager.load_file(path).map_err(|err| {
        Report::msg(format!("failed to load MASM source '{}': {err}", path.display()))
    })?;
    source_file
        .parse_with_options(source_manager, ParseOptions::new(ModuleKind::Library, module_path))
}

fn parse_source_with_module_path(
    source: impl Into<String>,
    module_path: impl AsRef<miden_assembly_syntax::Path>,
    context: Rc<Context>,
) -> Result<Box<Module>> {
    let source_manager = context.session().source_manager.clone();
    let uri = Uri::from(module_path.as_ref().as_str().to_string().into_boxed_str());
    let source_file = source_manager.load(SourceLanguage::Masm, uri, source.into());
    source_file
        .parse_with_options(source_manager, ParseOptions::new(ModuleKind::Library, module_path))
}

fn masm_module_path_from_file(path: &Path) -> Result<miden_assembly_syntax::PathBuf> {
    let stem = path.file_stem().and_then(|stem| stem.to_str()).ok_or_else(|| {
        Report::msg(format!("failed to derive MASM module name from '{}'", path.display()))
    })?;
    stem.parse::<miden_assembly_syntax::PathBuf>()
        .map_err(|err| Report::msg(format!("invalid MASM module path '{}': {err}", stem)))
}
