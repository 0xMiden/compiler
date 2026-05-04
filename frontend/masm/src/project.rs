use std::{
    fs,
    path::{Path, PathBuf},
};

use miden_core::serde::Deserializable;
use miden_mast_package::{Package as MastPackage, PackageExport};
use miden_project::{DependencyVersionScheme, Project};
use midenc_hir::Context;

use crate::{ExternalSignatureMap, Result, error};

pub(crate) struct ProjectTargetInput {
    pub source_path: PathBuf,
    pub external_signatures: ExternalSignatureMap,
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

    let external_signatures = collect_preassembled_dependency_signatures(&project)?;

    Ok(ProjectTargetInput {
        source_path,
        external_signatures,
    })
}

fn collect_preassembled_dependency_signatures(project: &Project) -> Result<ExternalSignatureMap> {
    let mut signatures = ExternalSignatureMap::new();
    let package = project.package();
    for dependency in package.dependencies() {
        let Some(path) = resolve_preassembled_dependency_path(project, dependency.scheme())? else {
            continue;
        };
        if path.extension().and_then(|ext| ext.to_str()) != Some(MastPackage::EXTENSION) {
            continue;
        }
        let package = load_mast_package(&path)?;
        for export in package.manifest.exports() {
            let PackageExport::Procedure(export) = export else {
                continue;
            };
            let Some(signature) = &export.signature else {
                continue;
            };
            let path = export.path.to_absolute().to_string();
            if let Some(existing) = signatures.insert(path.clone(), signature.clone())
                && existing != *signature
            {
                return Err(error::error(format!(
                    "conflicting package metadata signatures for external procedure '{path}'"
                )));
            }
        }
    }
    Ok(signatures)
}

fn resolve_preassembled_dependency_path(
    project: &Project,
    scheme: &DependencyVersionScheme,
) -> Result<Option<PathBuf>> {
    match scheme {
        DependencyVersionScheme::Path { path, .. } => {
            let package = project.package();
            let base_dir = package.manifest_path().and_then(Path::parent).ok_or_else(|| {
                error::error("project package does not have a filesystem manifest path")
            })?;
            Ok(Some(resolve_uri_path(base_dir, path.inner().path())))
        }
        DependencyVersionScheme::WorkspacePath { path, .. } => {
            let Some(base_dir) = workspace_base_dir(project) else {
                return Ok(None);
            };
            Ok(Some(resolve_uri_path(base_dir, path.inner().path())))
        }
        DependencyVersionScheme::Registry(_)
        | DependencyVersionScheme::Workspace { .. }
        | DependencyVersionScheme::Git { .. } => Ok(None),
    }
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
