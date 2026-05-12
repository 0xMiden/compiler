//! MASM-to-HIR disassembler.

mod events;
mod infer;
mod lift;
mod project;
mod semantics;
mod signatures;
mod stack;
#[cfg(test)]
mod tests;

use std::{collections::BTreeMap, path::Path, rc::Rc, sync::Arc};

use miden_assembly_syntax::{
    Parse, ParseOptions,
    ast::{self, Module, ModuleKind},
    debuginfo::{SourceLanguage, SourceManager, SourceManagerExt, Uri},
};
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

/// Result of disassembling a MASM module.
pub struct DisassembledModule {
    /// The HIR context which owns the lifted module and all nested IR entities.
    pub context: Rc<Context>,
    /// The lifted HIR module.
    pub module: builtin::ModuleRef,
}

/// Disassemble a MASM file into an HIR module.
pub fn disassemble_file(
    path: impl AsRef<Path>,
    config: &DisassemblerConfig,
    context: Rc<Context>,
) -> Result<DisassembledModule> {
    disassemble_file_with_external_signatures(path, config, &ExternalSignatureMap::new(), context)
}

/// Disassemble a MASM file into an HIR module, using externally-provided procedure signatures for
/// path-based invoke targets.
pub fn disassemble_file_with_external_signatures(
    path: impl AsRef<Path>,
    config: &DisassemblerConfig,
    external_signatures: &ExternalSignatureMap,
    context: Rc<Context>,
) -> Result<DisassembledModule> {
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
) -> Result<DisassembledModule> {
    let path = path.as_ref();
    let source_manager = context.session().source_manager.clone();
    let source_file = source_manager.load_file(path).map_err(|err| {
        Report::msg(format!("failed to load MASM source '{}': {err}", path.display()))
    })?;
    let module = source_file
        .parse_with_options(source_manager, ParseOptions::new(ModuleKind::Library, module_path))?;
    lift::lift_module(&module, config, external_signatures, external_types, context)
}

/// Disassemble a MASM source string into an HIR module.
pub fn disassemble_source(
    source: impl Into<String>,
    module_path: impl AsRef<miden_assembly_syntax::Path>,
    config: &DisassemblerConfig,
    context: Rc<Context>,
) -> Result<DisassembledModule> {
    disassemble_source_with_external_signatures(
        source,
        module_path,
        config,
        &ExternalSignatureMap::new(),
        context,
    )
}

/// Disassemble a MASM source string into an HIR module, using externally-provided procedure
/// signatures for path-based invoke targets.
pub fn disassemble_source_with_external_signatures(
    source: impl Into<String>,
    module_path: impl AsRef<miden_assembly_syntax::Path>,
    config: &DisassemblerConfig,
    external_signatures: &ExternalSignatureMap,
    context: Rc<Context>,
) -> Result<DisassembledModule> {
    let source_manager = context.session().source_manager.clone();
    let uri = Uri::from(module_path.as_ref().as_str().to_string().into_boxed_str());
    let source_file = source_manager.load(SourceLanguage::Masm, uri, source.into());
    let module = source_file
        .parse_with_options(source_manager, ParseOptions::new(ModuleKind::Library, module_path))?;
    lift::lift_module(&module, config, external_signatures, &ExternalTypeMap::new(), context)
}

/// Disassemble a target from a `miden-project.toml` package manifest.
pub fn disassemble_project_target(
    manifest_path: impl AsRef<Path>,
    target: Option<&str>,
    config: &DisassemblerConfig,
    context: Rc<Context>,
) -> Result<DisassembledModule> {
    let target = project::resolve_project_target(manifest_path.as_ref(), target, &context)?;
    disassemble_file_with_module_path_and_external_signatures(
        target.source_path,
        target.module_path,
        config,
        &target.external_signatures,
        &target.external_types,
        context,
    )
}

/// Disassemble a target from a `miden-project.toml` package manifest, using a precomputed
/// dependency graph to discover external procedure signatures.
pub fn disassemble_project_target_with_dependency_graph(
    manifest_path: impl AsRef<Path>,
    target: Option<&str>,
    dependency_graph: &miden_project::ProjectDependencyGraph,
    config: &DisassemblerConfig,
    context: Rc<Context>,
) -> Result<DisassembledModule> {
    let target = project::resolve_project_target_with_dependency_graph(
        manifest_path.as_ref(),
        target,
        dependency_graph,
        &context,
    )?;
    disassemble_file_with_module_path_and_external_signatures(
        target.source_path,
        target.module_path,
        config,
        &target.external_signatures,
        &target.external_types,
        context,
    )
}

/// Disassemble a parsed MASM AST module into HIR.
pub fn disassemble_module(
    module: &Module,
    config: &DisassemblerConfig,
    context: Rc<Context>,
) -> Result<DisassembledModule> {
    lift::lift_module(
        module,
        config,
        &ExternalSignatureMap::new(),
        &ExternalTypeMap::new(),
        context,
    )
}

fn masm_module_path_from_file(path: &Path) -> Result<miden_assembly_syntax::PathBuf> {
    let stem = path.file_stem().and_then(|stem| stem.to_str()).ok_or_else(|| {
        Report::msg(format!("failed to derive MASM module name from '{}'", path.display()))
    })?;
    stem.parse::<miden_assembly_syntax::PathBuf>()
        .map_err(|err| Report::msg(format!("invalid MASM module path '{}': {err}", stem)))
}
