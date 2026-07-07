//! MASM-to-HIR disassembler.

mod events;
mod infer;
mod lift;
pub mod project;
mod semantics;
mod stack;
#[cfg(test)]
mod tests;

use std::{collections::BTreeMap, path::Path, rc::Rc, sync::Arc};

use miden_assembly::{ProjectSourceInputs, ast::ModuleKind};
use miden_assembly_syntax::{
    ast::{self, Module},
    debuginfo::{SourceLanguage, SourceManager, SourceSpan, Uri},
    parser::read_modules_from_root,
};
use midenc_hir::{Context, FunctionType, Report, Type, dialects::builtin};

use self::project::ExternalMetadata;

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
    let source_manager = context.source_manager();
    let warnings_as_errors = context.session().options.diagnostics.warnings.warnings_as_errors();
    let (root, support) =
        read_modules_from_root(path, None, None, source_manager, warnings_as_errors)?;

    let target =
        project::ProjectTargetInput::new(ProjectSourceInputs { root, support }, Default::default());
    lift::lift_project_target(target, &lift::LiftConfig::strict(config), context)
}

/// Disassemble a MASM file for linting, skipping procedures that cannot be lifted.
pub fn disassemble_file_for_lint(
    path: impl AsRef<Path>,
    config: &DisassemblerConfig,
    context: Rc<Context>,
) -> Result<DisassembledWorld> {
    let path = path.as_ref();
    let source_manager = context.source_manager();
    let warnings_as_errors = context.session().options.diagnostics.warnings.warnings_as_errors();
    let (root, support) =
        read_modules_from_root(path, None, None, source_manager, warnings_as_errors)?;
    let target =
        project::ProjectTargetInput::new(ProjectSourceInputs { root, support }, Default::default());
    lift::lift_project_target(target, &lift::LiftConfig::lint(config), context)
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
    let source_manager = context.source_manager();
    let warnings_as_errors = context.session().options.diagnostics.warnings.warnings_as_errors();
    let (root, support) =
        read_modules_from_root(path, None, None, source_manager, warnings_as_errors)?;
    let target = project::ProjectTargetInput::new(
        ProjectSourceInputs { root, support },
        ExternalMetadata {
            signatures: external_signatures.clone(),
            ..Default::default()
        },
    );
    lift::lift_project_target(target, &lift::LiftConfig::strict(config), context)
}

/// Disassemble a MASM source string into an HIR world.
pub fn disassemble_source(
    source: impl Into<String>,
    module_path: impl AsRef<miden_assembly_syntax::Path>,
    config: &DisassemblerConfig,
    context: Rc<Context>,
) -> Result<DisassembledWorld> {
    let root = parse_source_with_module_path(source, module_path, context.clone())?;
    let target = project::ProjectTargetInput::new(
        ProjectSourceInputs {
            root,
            support: Default::default(),
        },
        ExternalMetadata::default(),
    );
    lift::lift_project_target(target, &lift::LiftConfig::strict(config), context)
}

/// Disassemble a MASM source string for linting, skipping procedures that cannot be lifted.
pub fn disassemble_source_for_lint(
    source: impl Into<String>,
    module_path: impl AsRef<miden_assembly_syntax::Path>,
    config: &DisassemblerConfig,
    context: Rc<Context>,
) -> Result<DisassembledWorld> {
    let root = parse_source_with_module_path(source, module_path, context.clone())?;
    let target = project::ProjectTargetInput::new(
        ProjectSourceInputs {
            root,
            support: Default::default(),
        },
        ExternalMetadata::default(),
    );
    lift::lift_project_target(target, &lift::LiftConfig::lint(config), context)
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
    let root = parse_source_with_module_path(source, module_path, context.clone())?;
    let external_signatures: ExternalSignatureMap = external_signatures
        .iter()
        .map(|(path, ty)| (path.to_absolute().unwrap().into(), ty.clone()))
        .collect();
    let mut target = project::ProjectTargetInput::new(
        ProjectSourceInputs {
            root,
            support: Default::default(),
        },
        ExternalMetadata {
            signatures: external_signatures.clone(),
            ..Default::default()
        },
    );
    let mut modules = BTreeMap::<Arc<ast::Path>, Box<Module>>::default();
    for (path, sig) in external_signatures {
        let name = path.procedure_name().unwrap().expect("invalid procedure path");
        let (_, module_path) = path.split_last().unwrap();
        let module_path: Arc<ast::Path> = module_path.to_path_buf().into_boxed_path().into();
        let kind = if module_path.is_in_kernel() {
            ModuleKind::Kernel
        } else {
            ModuleKind::Library
        };
        let module = modules
            .entry(module_path.clone())
            .or_insert_with(|| Box::new(Module::new(kind, module_path.clone())));
        let procedure = if module_path.is_kernel_path() {
            ast::Procedure::new_syscall(
                miden_assembly_syntax::debuginfo::SourceSpan::UNKNOWN,
                name,
                0,
                ast::Block::new(miden_assembly_syntax::debuginfo::SourceSpan::UNKNOWN, vec![]),
            )
        } else {
            ast::Procedure::new(
                miden_assembly_syntax::debuginfo::SourceSpan::UNKNOWN,
                ast::Visibility::Public,
                name,
                0,
                ast::Block::new(miden_assembly_syntax::debuginfo::SourceSpan::UNKNOWN, vec![]),
            )
        };
        let sig = ast::FunctionType::new(
            sig.abi,
            sig.params().iter().cloned().map(ast::TypeExpr::from).collect(),
            sig.results().iter().cloned().map(ast::TypeExpr::from).collect(),
        );
        module.define_procedure(procedure.with_signature(sig), context.source_manager())?;
    }
    let kernel = match modules.remove(ast::Path::KERNEL) {
        None => None,
        Some(root) => {
            let support = modules
                .extract_if(.., |_, m| m.is_in_kernel())
                .map(|(_, m)| m)
                .collect::<Vec<_>>();
            let kernel = miden_assembly::Assembler::new(context.source_manager())
                .assemble_kernel("kernel", root, support)
                .map(Arc::from)?;
            Some(kernel)
        }
    };
    target.kernel = kernel;
    target.dependency_modules.extend(modules.into_values());
    lift::lift_project_target(target, &lift::LiftConfig::strict(config), context)
}

/// Disassemble already-parsed `sources`, discovering external procedure signatures from
/// `dependency_graph`.
///
/// For a caller that has the target's sources in hand — the compiler's Miden Assembly frontend,
/// which is handed them by the assembler — and so needs nothing resolved from a manifest but the
/// signatures of the procedures those sources call into.
///
/// The graph is the assembler's own resolved one, which is why this takes it rather than a
/// [`Project`](miden_project::Project): it names every dependency in the closure with its
/// provenance already resolved, where a project's `[dependencies]` table names only the direct
/// ones. Registry dependencies contribute nothing, which is what iterating that table gave them
/// too — the caller's assembler has already linked those packages, and the disassembler's
/// signature inference covers whatever the linked package does not answer.
pub fn disassemble_project_target_with_sources(
    sources: ProjectSourceInputs,
    dependency_graph: &miden_project::ProjectDependencyGraph,
    config: &DisassemblerConfig,
    context: Rc<Context>,
) -> Result<DisassembledWorld> {
    let external_metadata = project::collect_dependency_graph_metadata(
        dependency_graph,
        project::RegistryNodes::Skip,
        &context,
    )?;
    let inputs = project::ProjectTargetInput::new(sources, external_metadata);
    lift::lift_project_target(inputs, &lift::LiftConfig::strict(config), context)
}

/// Disassemble a project target for linting, skipping procedures that cannot be lifted.
pub fn disassemble_project_target_for_lint(
    project: &miden_project::Project,
    target: Option<&str>,
    sources: Option<ProjectSourceInputs>,
    config: &DisassemblerConfig,
    context: Rc<Context>,
) -> Result<DisassembledWorld> {
    let inputs = if let Some(sources) = sources {
        let metadata = project::collect_dependency_metadata(project, &context)?;
        project::ProjectTargetInput::new(sources, metadata)
    } else {
        project::resolve_project_target(project, target, &context)?
    };
    lift::lift_project_target(inputs, &lift::LiftConfig::lint(config), context)
}

/// Disassemble pre-resolved project target inputs for linting.
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
    root: Box<Module>,
    config: &DisassemblerConfig,
    context: Rc<Context>,
) -> Result<DisassembledWorld> {
    let target = project::ProjectTargetInput::new(
        ProjectSourceInputs {
            root,
            support: Default::default(),
        },
        ExternalMetadata::default(),
    );
    lift::lift_project_target(target, &lift::LiftConfig::strict(config), context)
}

/// Disassemble a parsed MASM AST module for linting, skipping procedures that cannot be lifted.
pub fn disassemble_module_for_lint(
    root: Box<Module>,
    config: &DisassemblerConfig,
    context: Rc<Context>,
) -> Result<DisassembledWorld> {
    let target = project::ProjectTargetInput::new(
        ProjectSourceInputs {
            root,
            support: Default::default(),
        },
        ExternalMetadata::default(),
    );
    lift::lift_project_target(target, &lift::LiftConfig::lint(config), context)
}

fn parse_source_with_module_path(
    source: impl Into<String>,
    module_path: impl AsRef<miden_assembly_syntax::Path>,
    context: Rc<Context>,
) -> Result<Box<Module>> {
    let source_manager = context.session().source_manager.clone();
    let uri = Uri::from(module_path.as_ref().as_str().to_string().into_boxed_str());
    let source_file = source_manager.load(SourceLanguage::Masm, uri, source.into());
    miden_assembly_syntax::ModuleParser::new(None).parse(
        Some(module_path.as_ref()),
        source_file,
        source_manager,
    )
}
