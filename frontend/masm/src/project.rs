use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

use miden_assembly_syntax::{
    Parse, ParseOptions, PathBuf as MasmPathBuf,
    ast::{Export, Module, ModuleKind},
    debuginfo::SourceManager,
};
use miden_core::serde::Deserializable;
use miden_mast_package::{Package as MastPackage, PackageExport};
use miden_project::{
    DependencyVersionScheme, Package as ProjectPackage, Project, ProjectDependencyGraph,
    ProjectDependencyNode, ProjectDependencyNodeProvenance, ProjectSource, Target,
};
use midenc_hir::{Context, Type};

use crate::{ExternalSignatureMap, ExternalTypeMap, Result, error, signatures};

pub(crate) struct ProjectTargetInput {
    pub source_path: PathBuf,
    pub module_path: MasmPathBuf,
    pub external_signatures: ExternalSignatureMap,
    pub external_types: ExternalTypeMap,
}

#[derive(Default)]
struct ExternalMetadata {
    signatures: ExternalSignatureMap,
    types: ExternalTypeMap,
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
            Some(name) => error::error(format!("project has no target named '{name}'")),
            None => error::error("project has no build targets"),
        })?;

    let target_path = target.path.as_ref().ok_or_else(|| {
        error::error(format!(
            "target '{}' does not specify a MASM source path",
            target.name.inner()
        ))
    })?;

    let target_path = Path::new(target_path.inner().path());
    if target_path.extension().and_then(|ext| ext.to_str()) != Some("masm") {
        return Err(error::error(format!(
            "target '{}' path '{}' is not a .masm file",
            target.name.inner(),
            target_path.display()
        )));
    }

    let base_dir = package
        .manifest_path()
        .and_then(Path::parent)
        .ok_or_else(|| error::error("project package does not have a filesystem manifest path"))?;

    let source_path = if target_path.is_absolute() {
        target_path.to_path_buf()
    } else {
        base_dir.join(target_path)
    };

    let module_path = target.namespace.inner().as_ref().to_path_buf();
    let external_metadata = collect_dependency_metadata(&project, context)?;

    Ok(project_target_input(source_path, module_path, external_metadata))
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
        return Err(error::error(format!(
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
            Some(name) => error::error(format!("project has no target named '{name}'")),
            None => error::error("project has no build targets"),
        })?;

    let target_path = target.path.as_ref().ok_or_else(|| {
        error::error(format!(
            "target '{}' does not specify a MASM source path",
            target.name.inner()
        ))
    })?;

    let target_path = Path::new(target_path.inner().path());
    if target_path.extension().and_then(|ext| ext.to_str()) != Some("masm") {
        return Err(error::error(format!(
            "target '{}' path '{}' is not a .masm file",
            target.name.inner(),
            target_path.display()
        )));
    }

    let base_dir = package_base_dir(package.as_ref())?;
    let source_path = resolve_uri_path(
        base_dir,
        target_path.to_str().ok_or_else(|| {
            error::error(format!("target path '{}' is not valid UTF-8", target_path.display()))
        })?,
    );
    let module_path = target.namespace.inner().as_ref().to_path_buf();
    let external_metadata = collect_dependency_graph_metadata(dependency_graph, context)?;

    Ok(project_target_input(source_path, module_path, external_metadata))
}

fn project_target_input(
    source_path: PathBuf,
    module_path: MasmPathBuf,
    external_metadata: ExternalMetadata,
) -> ProjectTargetInput {
    ProjectTargetInput {
        source_path,
        module_path,
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
            context,
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
    let mut source_modules = Vec::<Box<Module>>::new();
    let source_manager = context.session().source_manager.clone();

    for (package, node) in dependency_graph.nodes() {
        if package == dependency_graph.root() {
            continue;
        }
        collect_dependency_graph_node_metadata(
            &mut metadata,
            &mut source_modules,
            node,
            source_manager.clone(),
        )?;
    }

    collect_source_modules_types(&mut metadata.types, context, &source_modules)?;
    for module in &source_modules {
        collect_source_module_signatures(
            &mut metadata.signatures,
            context,
            module,
            &metadata.types,
        )?;
    }

    Ok(metadata)
}

fn collect_dependency_graph_node_metadata(
    metadata: &mut ExternalMetadata,
    source_modules: &mut Vec<Box<Module>>,
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
            if let Some(module) = parse_source_package_module(package.as_ref(), source_manager)? {
                source_modules.push(module);
            }
            Ok(())
        }
        ProjectDependencyNodeProvenance::Source(ProjectSource::Real {
            library_path: None, ..
        })
        | ProjectDependencyNodeProvenance::Source(ProjectSource::Virtual { .. }) => Ok(()),
        ProjectDependencyNodeProvenance::Registry { selected, .. } => Err(error::error(format!(
            "dependency graph node '{}' resolved to registry package '{}', but registry package \
             artifacts are not available from the dependency graph",
            node.name, selected
        ))),
    }
}

fn collect_dependency_metadata_for_scheme(
    metadata: &mut ExternalMetadata,
    project: &Project,
    context: &Context,
    dependency_name: &str,
    scheme: &DependencyVersionScheme,
    source_manager: Arc<dyn SourceManager>,
) -> Result<()> {
    match scheme {
        DependencyVersionScheme::Path { path, .. } => {
            let package = project.package();
            let path = resolve_uri_path(package_base_dir(package.as_ref())?, path.inner().path());
            collect_path_dependency_metadata(
                metadata,
                context,
                dependency_name,
                &path,
                source_manager,
            )
        }
        DependencyVersionScheme::WorkspacePath { path, .. } => {
            let Some(base_dir) = workspace_base_dir(project) else {
                return Ok(());
            };
            let path = resolve_uri_path(base_dir, path.inner().path());
            collect_path_dependency_metadata(
                metadata,
                context,
                dependency_name,
                &path,
                source_manager,
            )
        }
        DependencyVersionScheme::Workspace { member, .. } => {
            let Project::WorkspacePackage { workspace, .. } = project else {
                return Ok(());
            };
            let Some(package) = workspace.get_member_by_relative_path(member.inner().path()) else {
                return Err(error::error(format!(
                    "workspace dependency '{dependency_name}' refers to missing member '{}'",
                    member.inner().path()
                )));
            };
            collect_source_package_metadata(metadata, context, package.as_ref(), source_manager)
        }
        DependencyVersionScheme::Registry(_) | DependencyVersionScheme::Git { .. } => Ok(()),
    }
}

fn collect_path_dependency_metadata(
    metadata: &mut ExternalMetadata,
    context: &Context,
    dependency_name: &str,
    path: &Path,
    source_manager: Arc<dyn SourceManager>,
) -> Result<()> {
    if path.extension().and_then(|ext| ext.to_str()) == Some(MastPackage::EXTENSION) {
        return collect_mast_package_metadata(metadata, path);
    }

    let project = Project::load_project_reference(dependency_name, path, source_manager.as_ref())?;
    let package = project.package();
    collect_source_package_metadata(metadata, context, package.as_ref(), source_manager)
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
                    export.path.to_absolute().to_string(),
                    signature.clone(),
                )?;
            }
            PackageExport::Type(export) => {
                insert_external_type(
                    &mut metadata.types,
                    export.path.to_absolute().to_string(),
                    export.ty.clone(),
                )?;
            }
            PackageExport::Constant(_) => {}
        }
    }
    Ok(())
}

fn collect_source_package_metadata(
    metadata: &mut ExternalMetadata,
    context: &Context,
    package: &ProjectPackage,
    source_manager: Arc<dyn SourceManager>,
) -> Result<()> {
    let Some(module) = parse_source_package_module(package, source_manager)? else {
        return Ok(());
    };
    collect_source_module_types(&mut metadata.types, context, &module)?;
    collect_source_module_signatures(&mut metadata.signatures, context, &module, &metadata.types)
}

fn parse_source_package_module(
    package: &ProjectPackage,
    source_manager: Arc<dyn SourceManager>,
) -> Result<Option<Box<Module>>> {
    let Some(target) = package.library_target() else {
        return Ok(None);
    };
    let target = target.inner();
    let Some(source_path) = resolve_target_source_path(package, target)? else {
        return Ok(None);
    };
    if source_path.extension().and_then(|ext| ext.to_str()) != Some("masm") {
        return Err(error::error(format!(
            "library target '{}' path '{}' is not a .masm file",
            target.name.inner(),
            source_path.display()
        )));
    }

    let module_path = target.namespace.inner().as_ref().to_path_buf();
    let module = source_path
        .as_path()
        .parse_with_options(source_manager, ParseOptions::new(ModuleKind::Library, module_path))?;
    Ok(Some(module))
}

fn collect_source_module_types(
    types: &mut ExternalTypeMap,
    context: &Context,
    module: &Module,
) -> Result<()> {
    for (index, path) in module.exported() {
        let Export::Type(decl) = &module[index] else {
            continue;
        };
        let ty =
            signatures::convert_type_expr_with_external_types(context, module, &decl.ty(), types)?;
        insert_external_type(types, path.as_path().to_absolute().to_string(), ty)?;
    }

    Ok(())
}

fn collect_source_modules_types(
    types: &mut ExternalTypeMap,
    context: &Context,
    modules: &[Box<Module>],
) -> Result<()> {
    let mut pending = Vec::new();
    for (module_index, module) in modules.iter().enumerate() {
        for (index, path) in module.exported() {
            if matches!(&module[index], Export::Type(_)) {
                pending.push((module_index, index, path.as_path().to_absolute().to_string()));
            }
        }
    }

    while !pending.is_empty() {
        let mut progress = false;
        let mut next = Vec::new();
        let mut last_unresolved = None;

        for (module_index, index, path) in pending {
            let module = &modules[module_index];
            let Export::Type(decl) = &module[index] else {
                continue;
            };
            match signatures::convert_type_expr_with_external_types(
                context,
                module,
                &decl.ty(),
                types,
            ) {
                Ok(ty) => {
                    insert_external_type(types, path, ty)?;
                    progress = true;
                }
                Err(err) if is_unresolved_external_type_metadata(&err) => {
                    last_unresolved = Some(err);
                    next.push((module_index, index, path));
                }
                Err(err) => return Err(err),
            }
        }

        if !progress {
            return Err(last_unresolved.unwrap_or_else(|| {
                error::error("failed to resolve source dependency type metadata")
            }));
        }
        pending = next;
    }

    Ok(())
}

fn is_unresolved_external_type_metadata(err: &miden_assembly_syntax::diagnostics::Report) -> bool {
    err.to_string().contains("external type metadata")
}

fn collect_source_module_signatures(
    signatures: &mut ExternalSignatureMap,
    context: &Context,
    module: &Module,
    external_types: &ExternalTypeMap,
) -> Result<()> {
    for (index, path) in module.exported() {
        let Some(signature) = module.procedure_signature(index) else {
            continue;
        };
        let signature = if external_types.is_empty() {
            signatures::convert_ast_function_type(context, module, signature)?
        } else {
            signatures::convert_ast_function_type_with_external_types(
                context,
                module,
                signature,
                external_types,
            )?
        };
        insert_external_signature(signatures, path.as_path().to_absolute().to_string(), signature)?;
    }

    Ok(())
}

fn resolve_target_source_path(
    package: &ProjectPackage,
    target: &Target,
) -> Result<Option<PathBuf>> {
    let Some(path) = &target.path else {
        return Ok(None);
    };
    Ok(Some(resolve_uri_path(package_base_dir(package)?, path.inner().path())))
}

fn package_base_dir(package: &ProjectPackage) -> Result<&Path> {
    package
        .manifest_path()
        .and_then(Path::parent)
        .ok_or_else(|| error::error("project package does not have a filesystem manifest path"))
}

fn insert_external_signature(
    signatures: &mut ExternalSignatureMap,
    path: String,
    signature: midenc_hir::FunctionType,
) -> Result<()> {
    if let Some(existing) = signatures.insert(path.clone(), signature.clone())
        && existing != signature
    {
        return Err(error::error(format!(
            "conflicting package metadata signatures for external procedure '{path}'"
        )));
    }
    Ok(())
}

fn insert_external_type(types: &mut ExternalTypeMap, path: String, ty: Type) -> Result<()> {
    if let Some(existing) = types.insert(path.clone(), ty.clone())
        && existing != ty
    {
        return Err(error::error(format!(
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

fn load_mast_package(path: &Path) -> Result<MastPackage> {
    let bytes = fs::read(path).map_err(|err| {
        error::error(format!("failed to read Miden package dependency '{}': {err}", path.display()))
    })?;
    MastPackage::read_from_bytes(&bytes).map_err(|err| {
        error::error(format!(
            "failed to decode Miden package dependency '{}': {err}",
            path.display()
        ))
    })
}
