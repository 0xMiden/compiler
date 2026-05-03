use std::path::{Path, PathBuf};

use miden_project::Project;
use midenc_hir::Context;

use crate::{Result, error};

pub(crate) fn resolve_project_target_path(
    manifest_path: &Path,
    target_name: Option<&str>,
    context: &Context,
) -> Result<PathBuf> {
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

    Ok(if target_path.is_absolute() {
        target_path.to_path_buf()
    } else {
        base_dir.join(target_path)
    })
}
