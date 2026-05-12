#![allow(clippy::vec_box)]

use std::{
    collections::BTreeSet,
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

use miden_assembly_syntax::{
    ModuleParser, Path as MasmPath,
    ast::{self, Module, ModuleKind},
    debuginfo::SourceManager,
};
use miden_core::serde::Deserializable;
use miden_mast_package::{Package as MastPackage, PackageExport, TargetType};
use miden_project::{
    DependencyVersionScheme, Package as ProjectPackage, Project, ProjectDependencyGraph,
    ProjectDependencyNode, ProjectDependencyNodeProvenance, ProjectSource, Target,
};
use midenc_hir::{Context, Report, Type};

use crate::{ExternalSignatureMap, ExternalTypeMap, Result};

pub(crate) struct ProjectTargetInput {
    pub root: Box<Module>,
    pub support: Vec<Box<Module>>,
    pub dependency_modules: Vec<Box<Module>>,
    pub external_signatures: ExternalSignatureMap,
    pub external_types: ExternalTypeMap,
}

#[derive(Default)]
struct ExternalMetadata {
    signatures: ExternalSignatureMap,
    types: ExternalTypeMap,
    source_modules: Vec<Box<Module>>,
}

pub(crate) fn resolve_project_target(
    manifest_path: &Path,
    target_name: Option<&str>,
    context: &Context,
) -> Result<ProjectTargetInput> {
    let source_manager = context.session().source_manager.clone();
    let project = Project::load(manifest_path, source_manager.as_ref())?;
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

    let (root, support) = load_target_modules(package.as_ref(), target.inner(), source_manager)?;
    let external_metadata = collect_dependency_metadata(&project, context)?;

    Ok(project_target_input(root, support, external_metadata))
}

pub(crate) fn resolve_project_target_with_dependency_graph(
    manifest_path: &Path,
    target_name: Option<&str>,
    dependency_graph: &ProjectDependencyGraph,
    context: &Context,
) -> Result<ProjectTargetInput> {
    let source_manager = context.session().source_manager.clone();
    let project = Project::load(manifest_path, source_manager.as_ref())?;
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

    let (root, support) = load_target_modules(package.as_ref(), target.inner(), source_manager)?;
    let external_metadata = collect_dependency_graph_metadata(dependency_graph, context)?;

    Ok(project_target_input(root, support, external_metadata))
}

fn project_target_input(
    root: Box<Module>,
    support: Vec<Box<Module>>,
    external_metadata: ExternalMetadata,
) -> ProjectTargetInput {
    ProjectTargetInput {
        root,
        support,
        dependency_modules: external_metadata.source_modules,
        external_signatures: external_metadata.signatures,
        external_types: external_metadata.types,
    }
}

fn collect_dependency_metadata(project: &Project, context: &Context) -> Result<ExternalMetadata> {
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
    let (root, support) = load_target_modules(package, target.inner(), source_manager)?;
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

fn load_target_modules(
    package: &ProjectPackage,
    target: &Target,
    source_manager: Arc<dyn SourceManager>,
) -> Result<(Box<Module>, Vec<Box<Module>>)> {
    let target_path = target.path.as_ref().ok_or_else(|| {
        Report::msg(format!("target '{}' does not specify a MASM source path", target.name.inner()))
    })?;
    let target_path = resolve_uri_path(package_base_dir(package)?, target_path.inner().path());
    if target_path.extension().and_then(|ext| ext.to_str()) != Some(Module::FILE_EXTENSION) {
        return Err(Report::msg(format!(
            "target '{}' path '{}' is not a .masm file",
            target.name.inner(),
            target_path.display()
        )));
    }

    let root_path = target_path.canonicalize().map_err(|error| {
        Report::msg(format!("failed to resolve target source '{}': {error}", target_path.display()))
    })?;
    let root_dir = root_path.parent().map(Path::to_path_buf).ok_or_else(|| {
        Report::msg(format!("target source '{}' has no parent directory", root_path.display()))
    })?;
    let root = parse_module_file(
        &root_path,
        target_root_module_kind(target.ty),
        target.namespace.inner().as_ref(),
        source_manager.clone(),
    )?;

    let mut excluded = excluded_target_roots(package, target, &root_path)?;
    excluded.insert(root_path);
    let support_paths = read_support_module_paths(&root_dir, target.namespace.inner(), &excluded)?;
    let support = support_paths
        .iter()
        .map(|path| {
            let relative = path.strip_prefix(&root_dir).map_err(|error| {
                Report::msg(format!(
                    "failed to derive module path for '{}': {error}",
                    path.display()
                ))
            })?;
            let module_path = module_path_from_relative(target.namespace.inner(), relative)?;
            parse_module_file(
                path,
                ModuleKind::Library,
                module_path.as_ref(),
                source_manager.clone(),
            )
        })
        .collect::<Result<Vec<_>>>()?;

    Ok((root, support))
}

fn parse_module_file(
    path: &Path,
    kind: ModuleKind,
    module_path: &MasmPath,
    source_manager: Arc<dyn SourceManager>,
) -> Result<Box<Module>> {
    let mut parser = ModuleParser::new(kind);
    parser.parse_file(module_path, path, source_manager)
}

fn target_root_module_kind(ty: TargetType) -> ModuleKind {
    match ty {
        TargetType::Executable => ModuleKind::Executable,
        TargetType::Kernel => ModuleKind::Kernel,
        _ => ModuleKind::Library,
    }
}

fn excluded_target_roots(
    package: &ProjectPackage,
    target: &Target,
    current_root: &Path,
) -> Result<BTreeSet<PathBuf>> {
    let base_dir = package_base_dir(package)?;
    let mut excluded = BTreeSet::new();

    if !target.ty.is_executable()
        && let Some(library_target) = package.library_target()
        && let Some(path) = library_target.path.as_ref()
    {
        insert_excluded_target_root(&mut excluded, base_dir, path.inner().path(), current_root)?;
    }

    for executable in package.executable_targets() {
        let Some(path) = executable.path.as_ref() else {
            continue;
        };
        insert_excluded_target_root(&mut excluded, base_dir, path.inner().path(), current_root)?;
    }

    Ok(excluded)
}

fn insert_excluded_target_root(
    excluded: &mut BTreeSet<PathBuf>,
    base_dir: &Path,
    path: &str,
    current_root: &Path,
) -> Result<()> {
    let path = resolve_uri_path(base_dir, path);
    if let Ok(path) = path.canonicalize()
        && path != current_root
    {
        excluded.insert(path);
    }
    Ok(())
}

fn read_support_module_paths(
    root_dir: &Path,
    namespace: &MasmPath,
    excluded: &BTreeSet<PathBuf>,
) -> Result<Vec<PathBuf>> {
    let mut paths = Vec::new();
    collect_module_files(root_dir, &mut paths)?;
    paths.sort();

    let mut modules = Vec::new();
    for path in paths {
        let canonical = path.canonicalize().map_err(|error| {
            Report::msg(format!("failed to resolve '{}': {error}", path.display()))
        })?;
        if excluded.contains(&canonical) {
            continue;
        }

        let relative = canonical.strip_prefix(root_dir).map_err(|error| {
            Report::msg(format!(
                "failed to derive module path for '{}': {error}",
                canonical.display()
            ))
        })?;
        module_path_from_relative(namespace, relative)?;
        modules.push(canonical);
    }

    Ok(modules)
}

fn collect_module_files(dir: &Path, paths: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(dir).map_err(|error| {
        Report::msg(format!("failed to read module directory '{}': {error}", dir.display()))
    })? {
        let entry = entry.map_err(|error| {
            Report::msg(format!("failed to read directory entry in '{}': {error}", dir.display()))
        })?;
        let path = entry.path();
        let file_type = entry.file_type().map_err(|error| {
            Report::msg(format!("failed to read file type for '{}': {error}", path.display()))
        })?;

        if file_type.is_dir() {
            collect_module_files(&path, paths)?;
            continue;
        }

        if path.extension() == Some(AsRef::<OsStr>::as_ref(Module::FILE_EXTENSION)) {
            paths.push(path);
        }
    }

    Ok(())
}

fn module_path_from_relative(namespace: &MasmPath, relative: &Path) -> Result<Arc<MasmPath>> {
    let mut module_path = namespace.to_path_buf();
    let stem = relative.with_extension("");
    let mut components = stem
        .iter()
        .map(|component| {
            component.to_str().ok_or_else(|| {
                Report::msg(format!("module path '{}' contains invalid UTF-8", relative.display()))
            })
        })
        .collect::<Result<Vec<_>>>()?;

    if components.last().is_some_and(|component| *component == Module::ROOT) {
        components.pop();
    }

    for component in components {
        MasmPath::validate(component).map_err(|error| Report::msg(error.to_string()))?;
        module_path.push_component(component);
    }

    Ok(module_path.into())
}

fn load_mast_package(path: &Path) -> Result<MastPackage> {
    let bytes = fs::read(path).map_err(|err| {
        Report::msg(format!("failed to read Miden package dependency '{}': {err}", path.display()))
    })?;
    MastPackage::read_from_bytes(&bytes).map_err(|err| {
        Report::msg(format!(
            "failed to decode Miden package dependency '{}': {err}",
            path.display()
        ))
    })
}
