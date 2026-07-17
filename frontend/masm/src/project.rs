#![allow(clippy::vec_box)]

use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

use miden_assembly::ProjectSourceInputs;
use miden_assembly_syntax::{
    ast::{self, Module, ModuleKind},
    debuginfo::SourceManager,
    parser::read_modules_from_root,
};
use miden_mast_package::{Package as MastPackage, PackageExport};
use miden_project::{
    DependencyVersionScheme, Package as ProjectPackage, Project, ProjectDependencyGraph,
    ProjectDependencyNode, ProjectDependencyNodeProvenance, ProjectSource, Target,
};
use midenc_hir::{Context, Report, Type};

use crate::{ExternalSignatureMap, ExternalTypeMap, Result};

pub struct ProjectTargetInput {
    pub sources: ProjectSourceInputs,
    pub dependency_modules: Vec<Box<Module>>,
    pub external_signatures: ExternalSignatureMap,
    pub external_types: ExternalTypeMap,
}

impl ProjectTargetInput {
    pub fn new(sources: ProjectSourceInputs, external_metadata: ExternalMetadata) -> Self {
        ProjectTargetInput {
            sources,
            dependency_modules: external_metadata.source_modules,
            external_signatures: external_metadata.signatures,
            external_types: external_metadata.types,
        }
    }
}

#[derive(Default)]
pub struct ExternalMetadata {
    pub signatures: ExternalSignatureMap,
    pub types: ExternalTypeMap,
    pub source_modules: Vec<Box<Module>>,
}

/// Resolve disassembler inputs for `target_name` of `project`
pub fn resolve_project_target(
    project: &Project,
    target_name: Option<&str>,
    context: &Context,
) -> Result<ProjectTargetInput> {
    let package = project.package();

    let target = package
        .library_target()
        .into_iter()
        .chain(package.executable_targets().iter())
        .find(|target| target_name.is_none_or(|name| target.name.as_ref().inner().as_ref() == name))
        .ok_or_else(|| match target_name {
            Some(name) => Report::msg(format!("project has no target named '{name}'")),
            None => Report::msg("project has no build targets"),
        })?;

    let source_manager = context.session().source_manager.clone();
    let sources = load_target_modules(package.as_ref(), target.inner(), source_manager)?;
    let external_metadata = collect_dependency_metadata(project, context)?;

    Ok(ProjectTargetInput::new(sources, external_metadata))
}

/// Resolve disassembler inputs for `target_name` of the project at `manifest_path`
pub fn resolve_project_target_from_manifest_path(
    manifest_path: &Path,
    target_name: Option<&str>,
    context: &Context,
) -> Result<ProjectTargetInput> {
    let project = Project::load(manifest_path, &context.session().source_manager)?;

    resolve_project_target(&project, target_name, context)
}

/// Resolve disassembler inputs for `target_name` of the project at `manifest_path`, using an
/// already-resolved `dependency_graph`.
pub fn resolve_project_target_from_manifest_path_with_dependency_graph(
    manifest_path: &Path,
    target_name: Option<&str>,
    dependency_graph: &ProjectDependencyGraph,
    context: &Context,
) -> Result<ProjectTargetInput> {
    let project = Project::load(manifest_path, &context.session().source_manager)?;

    resolve_project_target_with_dependency_graph(&project, target_name, dependency_graph, context)
}

/// Resolve disassembler inputs for `target_name` of `project` , using an already-resolved
/// `dependency_graph`.
pub fn resolve_project_target_with_dependency_graph(
    project: &Project,
    target_name: Option<&str>,
    dependency_graph: &ProjectDependencyGraph,
    context: &Context,
) -> Result<ProjectTargetInput> {
    let package = project.package();
    let package_name = package.name();
    if dependency_graph.root() != package_name.inner() {
        return Err(Report::msg(format!(
            "dependency graph root '{}' does not match project package '{}'",
            dependency_graph.root(),
            package_name.inner()
        )));
    }

    let target = package
        .library_target()
        .into_iter()
        .chain(package.executable_targets().iter())
        .find(|target| target_name.is_none_or(|name| target.name.as_ref().inner().as_ref() == name))
        .ok_or_else(|| match target_name {
            Some(name) => Report::msg(format!("project has no target named '{name}'")),
            None => Report::msg("project has no build targets"),
        })?;

    let source_manager = context.session().source_manager.clone();
    let sources = load_target_modules(package.as_ref(), target.inner(), source_manager)?;
    let external_metadata = collect_dependency_graph_metadata(dependency_graph, context)?;

    Ok(ProjectTargetInput::new(sources, external_metadata))
}

pub fn collect_dependency_metadata(
    project: &Project,
    context: &Context,
) -> Result<ExternalMetadata> {
    let mut metadata = ExternalMetadata::default();
    let package = project.package();
    let source_manager = context.session().source_manager.clone();
    for dependency in package.dependencies() {
        collect_dependency_metadata_for_scheme(
            &mut metadata,
            project,
            dependency.name().as_ref(),
            dependency.scheme(),
            source_manager.clone(),
        )?;
    }
    Ok(metadata)
}

fn collect_dependency_graph_metadata(
    dependency_graph: &ProjectDependencyGraph,
    context: &Context,
) -> Result<ExternalMetadata> {
    let mut metadata = ExternalMetadata::default();
    let source_manager = context.session().source_manager.clone();

    for (package, node) in dependency_graph.nodes() {
        if package == dependency_graph.root() {
            continue;
        }
        collect_dependency_graph_node_metadata(&mut metadata, node, source_manager.clone())?;
    }

    Ok(metadata)
}

fn collect_dependency_graph_node_metadata(
    metadata: &mut ExternalMetadata,
    node: &ProjectDependencyNode,
    source_manager: Arc<dyn SourceManager>,
) -> Result<()> {
    match &node.provenance {
        ProjectDependencyNodeProvenance::Preassembled { path, .. } => {
            collect_mast_package_metadata(metadata, path)
        }
        ProjectDependencyNodeProvenance::Source(ProjectSource::Real {
            manifest_path,
            library_path: Some(_),
            ..
        }) => {
            let project = Project::load_project_reference(
                node.name.as_ref(),
                manifest_path,
                source_manager.as_ref(),
            )?;
            let package = project.package();
            metadata
                .source_modules
                .extend(parse_source_package_modules(package.as_ref(), source_manager)?);
            Ok(())
        }
        ProjectDependencyNodeProvenance::Source(ProjectSource::Real {
            library_path: None, ..
        })
        | ProjectDependencyNodeProvenance::Source(ProjectSource::Virtual { .. }) => Ok(()),
        ProjectDependencyNodeProvenance::Registry { selected, .. } => Err(Report::msg(format!(
            "dependency graph node '{}' resolved to registry package '{}', but registry package \
             artifacts are not available from the dependency graph",
            node.name, selected
        ))),
    }
}

fn collect_dependency_metadata_for_scheme(
    metadata: &mut ExternalMetadata,
    project: &Project,
    dependency_name: &str,
    scheme: &DependencyVersionScheme,
    source_manager: Arc<dyn SourceManager>,
) -> Result<()> {
    match scheme {
        DependencyVersionScheme::Path { path, .. } => {
            let package = project.package();
            let path = resolve_uri_path(package_base_dir(package.as_ref())?, path.inner().path());
            collect_path_dependency_metadata(metadata, dependency_name, &path, source_manager)
        }
        DependencyVersionScheme::WorkspacePath { path, .. } => {
            let Some(base_dir) = workspace_base_dir(project) else {
                return Ok(());
            };
            let path = resolve_uri_path(base_dir, path.inner().path());
            collect_path_dependency_metadata(metadata, dependency_name, &path, source_manager)
        }
        DependencyVersionScheme::Workspace { member, .. } => {
            let Project::WorkspacePackage { workspace, .. } = project else {
                return Ok(());
            };
            let Some(package) = workspace.get_member_by_relative_path(member.inner().path()) else {
                return Err(Report::msg(format!(
                    "workspace dependency '{dependency_name}' refers to missing member '{}'",
                    member.inner().path()
                )));
            };
            collect_source_package_metadata(metadata, package.as_ref(), source_manager)
        }
        DependencyVersionScheme::Registry(_) | DependencyVersionScheme::Git { .. } => Ok(()),
    }
}

fn collect_path_dependency_metadata(
    metadata: &mut ExternalMetadata,
    dependency_name: &str,
    path: &Path,
    source_manager: Arc<dyn SourceManager>,
) -> Result<()> {
    if path.extension().and_then(|ext| ext.to_str()) == Some(MastPackage::EXTENSION) {
        return collect_mast_package_metadata(metadata, path);
    }

    let project = Project::load_project_reference(dependency_name, path, source_manager.as_ref())?;
    let package = project.package();
    collect_source_package_metadata(metadata, package.as_ref(), source_manager)
}

fn collect_mast_package_metadata(metadata: &mut ExternalMetadata, path: &Path) -> Result<()> {
    let package = load_mast_package(path)?;
    for export in package.manifest.exports() {
        match export {
            PackageExport::Procedure(export) => {
                let Some(signature) = &export.signature else {
                    continue;
                };
                insert_external_signature(
                    &mut metadata.signatures,
                    export.path.clone(),
                    signature.clone(),
                )?;
            }
            PackageExport::Type(export) => {
                insert_external_type(&mut metadata.types, export.path.clone(), export.ty.clone())?;
            }
            PackageExport::Constant(_) => {}
        }
    }
    Ok(())
}

fn collect_source_package_metadata(
    metadata: &mut ExternalMetadata,
    package: &ProjectPackage,
    source_manager: Arc<dyn SourceManager>,
) -> Result<()> {
    let modules = parse_source_package_modules(package, source_manager)?;
    metadata.source_modules.extend(modules);
    Ok(())
}

fn parse_source_package_modules(
    package: &ProjectPackage,
    source_manager: Arc<dyn SourceManager>,
) -> Result<Vec<Box<Module>>> {
    let Some(target) = package.library_target() else {
        return Ok(Vec::new());
    };
    let ProjectSourceInputs { root, support } =
        load_target_modules(package, target.inner(), source_manager)?;
    Ok(core::iter::once(root).chain(support).collect())
}

fn package_base_dir(package: &ProjectPackage) -> Result<&Path> {
    package
        .manifest_path()
        .and_then(Path::parent)
        .ok_or_else(|| Report::msg("project package does not have a filesystem manifest path"))
}

fn insert_external_signature(
    signatures: &mut ExternalSignatureMap,
    path: Arc<ast::Path>,
    signature: midenc_hir::FunctionType,
) -> Result<()> {
    if let Some(existing) = signatures.insert(path.clone(), signature.clone())
        && existing != signature
    {
        return Err(Report::msg(format!(
            "conflicting package metadata signatures for external procedure '{path}'"
        )));
    }
    Ok(())
}

fn insert_external_type(types: &mut ExternalTypeMap, path: Arc<ast::Path>, ty: Type) -> Result<()> {
    if let Some(existing) = types.insert(path.clone(), ty.clone())
        && existing != ty
    {
        return Err(Report::msg(format!(
            "conflicting package metadata types for external type '{path}'"
        )));
    }
    Ok(())
}

fn workspace_base_dir(project: &Project) -> Option<&Path> {
    match project {
        Project::WorkspacePackage { workspace, .. } => {
            workspace.manifest_path().and_then(Path::parent)
        }
        Project::Package(_) => None,
    }
}

fn resolve_uri_path(base_dir: &Path, path: &str) -> PathBuf {
    let path = Path::new(path);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        base_dir.join(path)
    }
}

pub fn load_target_modules(
    package: &ProjectPackage,
    target: &Target,
    source_manager: Arc<dyn SourceManager>,
) -> Result<ProjectSourceInputs> {
    let target_path = &target.path;
    let target_path = resolve_uri_path(package_base_dir(package)?, target_path.inner().path());
    if target_path.extension().and_then(|ext| ext.to_str()) != Some(Module::FILE_EXTENSION) {
        return Err(Report::msg(format!(
            "target '{}' path '{}' is not a .masm file",
            target.name.inner(),
            target_path.display()
        )));
    }

    let kind = if target.is_executable() {
        ModuleKind::Executable
    } else if target.is_kernel() {
        ModuleKind::Kernel
    } else {
        ModuleKind::Library
    };
    let (root, support) = read_modules_from_root(
        &target_path,
        Some(target.namespace.inner().clone()),
        Some(kind),
        source_manager,
        false,
    )?;

    Ok(ProjectSourceInputs { root, support })
}

fn load_mast_package(path: &Path) -> Result<MastPackage> {
    let bytes = fs::read(path).map_err(|err| {
        Report::msg(format!("failed to read Miden package dependency '{}': {err}", path.display()))
    })?;
    MastPackage::read_from_bytes_trusted(&bytes).map_err(|err| {
        Report::msg(format!(
            "failed to decode Miden package dependency '{}': {err}",
            path.display()
        ))
    })
}
