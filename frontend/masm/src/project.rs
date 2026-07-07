#![allow(clippy::vec_box)]

use std::{
    collections::BTreeSet,
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

use miden_assembly::ProjectSourceInputs;
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

pub struct ProjectTargetInput {
    pub sources: ProjectSourceInputs,
    pub dependency_modules: Vec<Box<Module>>,
    pub relative_module_paths: BTreeSet<Arc<MasmPath>>,
    pub external_signatures: ExternalSignatureMap,
    pub external_types: ExternalTypeMap,
}

impl ProjectTargetInput {
    pub fn new(sources: LoadedProjectSources, external_metadata: ExternalMetadata) -> Self {
        Self::with_relative_module_paths(
            sources.sources,
            sources.relative_module_paths,
            external_metadata,
        )
    }

    pub fn from_source_inputs(
        sources: ProjectSourceInputs,
        external_metadata: ExternalMetadata,
    ) -> Self {
        ProjectTargetInput {
            sources,
            dependency_modules: external_metadata.source_modules,
            relative_module_paths: external_metadata.relative_module_paths,
            external_signatures: external_metadata.signatures,
            external_types: external_metadata.types,
        }
    }

    fn with_relative_module_paths(
        sources: ProjectSourceInputs,
        relative_module_paths: BTreeSet<Arc<MasmPath>>,
        external_metadata: ExternalMetadata,
    ) -> Self {
        let mut relative_module_paths = relative_module_paths;
        relative_module_paths.extend(external_metadata.relative_module_paths);
        ProjectTargetInput {
            sources,
            dependency_modules: external_metadata.source_modules,
            relative_module_paths,
            external_signatures: external_metadata.signatures,
            external_types: external_metadata.types,
        }
    }
}

pub struct LoadedProjectSources {
    pub sources: ProjectSourceInputs,
    pub relative_module_paths: BTreeSet<Arc<MasmPath>>,
}

#[derive(Default)]
pub struct ExternalMetadata {
    pub signatures: ExternalSignatureMap,
    pub types: ExternalTypeMap,
    pub source_modules: Vec<Box<Module>>,
    pub relative_module_paths: BTreeSet<Arc<MasmPath>>,
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
            collect_source_package_metadata(metadata, package.as_ref(), source_manager)
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
    let Some(LoadedProjectSources {
        sources,
        relative_module_paths,
    }) = parse_source_package_sources(package, source_manager)?
    else {
        return Ok(());
    };
    metadata.relative_module_paths.extend(relative_module_paths);
    metadata
        .source_modules
        .extend(core::iter::once(sources.root).chain(sources.support));
    Ok(())
}

fn parse_source_package_sources(
    package: &ProjectPackage,
    source_manager: Arc<dyn SourceManager>,
) -> Result<Option<LoadedProjectSources>> {
    let Some(target) = package.library_target() else {
        return Ok(None);
    };
    load_target_modules(package, target.inner(), source_manager).map(Some)
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
) -> Result<LoadedProjectSources> {
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
    let mut relative_module_paths =
        collect_module_declarations(&root_path, target.namespace.inner().as_ref())?;
    let root = parse_module_file(
        &root_path,
        target_root_module_kind(target.ty),
        target.namespace.inner().as_ref(),
        source_manager.clone(),
    )?;

    let mut excluded = excluded_target_roots(package, target, &root_path)?;
    excluded.insert(root_path);
    let support_paths = read_support_module_paths(&root_dir, target.namespace.inner(), &excluded)?;
    let mut support = Vec::new();
    let mut support_modules = Vec::new();
    for path in &support_paths {
        let relative = path.strip_prefix(&root_dir).map_err(|error| {
            Report::msg(format!("failed to derive module path for '{}': {error}", path.display()))
        })?;
        let module_path = module_path_from_relative(target.namespace.inner(), relative)?;
        support.push(parse_module_file(
            path,
            ModuleKind::Library,
            module_path.as_ref(),
            source_manager.clone(),
        )?);
        support_modules.push((path, module_path));
    }

    let mut changed = true;
    while changed {
        changed = false;
        for (path, module_path) in &support_modules {
            if !relative_module_paths.contains(module_path) {
                continue;
            }
            for child in collect_module_declarations(path, module_path.as_ref())? {
                changed |= relative_module_paths.insert(child);
            }
        }
    }

    Ok(LoadedProjectSources {
        sources: ProjectSourceInputs { root, support },
        relative_module_paths,
    })
}

fn parse_module_file(
    path: &Path,
    kind: ModuleKind,
    module_path: &MasmPath,
    source_manager: Arc<dyn SourceManager>,
) -> Result<Box<Module>> {
    if let Some(source) = read_project_module_source(path, module_path)? {
        if source.lines().all(is_ignorable_module_line) {
            return Ok(Box::new(Module::new(kind, module_path)));
        }
        let mut parser = ModuleParser::new(kind);
        return parser.parse_str(module_path, source, source_manager);
    }

    let mut parser = ModuleParser::new(kind);
    parser.parse_file(module_path, path, source_manager)
}

fn read_project_module_source(path: &Path, module_path: &MasmPath) -> Result<Option<String>> {
    if path.file_name().and_then(|name| name.to_str()) != Some(Module::ROOT_FILENAME) {
        return Ok(None);
    }

    let source = fs::read_to_string(path).map_err(|error| {
        Report::msg(format!("failed to read MASM module index '{}': {error}", path.display()))
    })?;
    let source = source
        .lines()
        .map(|line| {
            if is_module_declaration(line) {
                String::new()
            } else if let Some(line) = rewrite_grouped_public_use_declaration(line, module_path) {
                line
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    Ok(Some(source))
}

fn collect_module_declarations(
    path: &Path,
    module_path: &MasmPath,
) -> Result<BTreeSet<Arc<MasmPath>>> {
    if path.file_name().and_then(|name| name.to_str()) != Some(Module::ROOT_FILENAME) {
        return Ok(BTreeSet::new());
    }

    let source = fs::read_to_string(path).map_err(|error| {
        Report::msg(format!("failed to read MASM module index '{}': {error}", path.display()))
    })?;
    let mut modules = BTreeSet::new();
    for line in source.lines() {
        let line = line.split_once('#').map_or(line, |(code, _)| code).trim();
        let module = line.strip_prefix("pub ").unwrap_or(line);
        let Some(module) = module.strip_prefix("mod ") else {
            continue;
        };
        let module = module.trim();
        if !is_valid_module_declaration_path(module) {
            continue;
        }
        let path = if module.starts_with("::") {
            module.to_string()
        } else {
            format!("::{}::{module}", module_path.as_str().trim_start_matches("::"))
        };
        modules.insert(Arc::from(MasmPath::new(&path)));
    }
    Ok(modules)
}

fn is_module_declaration(line: &str) -> bool {
    let line = line.split_once('#').map_or(line, |(code, _)| code).trim();
    let line = line.strip_prefix("pub ").unwrap_or(line);
    let Some(module) = line.strip_prefix("mod ") else {
        return false;
    };
    let module = module.trim();
    is_valid_module_declaration_path(module)
}

fn is_valid_module_declaration_path(module: &str) -> bool {
    MasmPath::validate(module).is_ok()
}

fn rewrite_grouped_public_use_declaration(line: &str, module_path: &MasmPath) -> Option<String> {
    let code_len = line.split_once('#').map_or(line.len(), |(code, _)| code.len());
    let (code, comment) = line.split_at(code_len);
    let pub_start = code.find(|c: char| !c.is_whitespace())?;
    if !code[pub_start..].starts_with("pub use ") {
        return None;
    }

    let indent = &code[..pub_start];
    let use_body = code[pub_start + "pub use ".len()..].trim();
    if let Some(imports) = expand_public_use_from(indent, use_body, comment, module_path) {
        return Some(imports);
    }

    None
}

fn expand_public_use_from(
    indent: &str,
    use_body: &str,
    comment: &str,
    current_module: &MasmPath,
) -> Option<String> {
    let use_body = use_body.strip_prefix('{')?;
    let (names, module_path) = use_body.split_once("} from ")?;
    let module_path = module_path.trim();
    let module_path = if let Some(relative) = module_path.strip_prefix("self::") {
        format!("{}::{relative}", current_module.as_str().trim_start_matches("::"))
    } else {
        module_path.to_string()
    };
    if module_path.is_empty() {
        return None;
    }

    let mut imports = Vec::new();
    for name in names.split(',').map(str::trim).filter(|name| !name.is_empty()) {
        let mut import = format!("{indent}pub use {module_path}::{name}->{name}");
        if imports.is_empty() {
            import.push_str(comment);
        }
        imports.push(import);
    }
    if imports.is_empty() {
        None
    } else {
        Some(imports.join("\n"))
    }
}

fn is_ignorable_module_line(line: &str) -> bool {
    let line = line.trim();
    line.is_empty() || line.starts_with('#')
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
