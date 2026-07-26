//! Construction helpers for compiling without a project manifest on disk.
//!
//! Used by tests today, and by input preparation for standalone inputs from increment 3.

use alloc::{format, sync::Arc};
use std::path::Path as FsPath;

use miden_assembly::TargetAssemblyContext;
use miden_package_registry::NoPackageStore;
use midenc_session::{
    diagnostics::{DefaultSourceManager, Report, SourceManager},
    miden_project::{
        Package as ProjectPackage, Profile, ProjectDependencyGraph, ProjectDependencyGraphBuilder,
        Target, TargetType, Uri,
    },
};

use crate::CompilerResult;

/// Write `contents` to `<temp>/midenc-pipeline-fixtures/<dir>/<file>` and return its path.
///
/// [`VirtualProject`] needs a target root that exists on disk, because the dependency graph
/// resolves and reads it. This is the one place that materializes one, so the tests across
/// `pipeline` agree on where fixtures live and on the failure messages when they cannot be
/// written.
///
/// `dir` must be unique per fixture: the directory is not cleaned up between runs, and
/// tests within a crate run concurrently, so two tests sharing a `dir` would race on the
/// same file.
///
/// Test-only, so that shipped builds of this module keep to `std::path` and do not reach
/// for `std::fs`. `tempfile` would be the obvious alternative, but it is not a
/// dev-dependency of this crate and this does not warrant adding one.
#[cfg(test)]
pub(crate) fn fixture_source(dir: &str, file: &str, contents: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join("midenc-pipeline-fixtures").join(dir);
    std::fs::create_dir_all(&dir).expect("should create fixture dir");
    let path = dir.join(file);
    std::fs::write(&path, contents).expect("should write fixture source");
    path
}

/// A [`fixture_source`] holding the smallest valid WebAssembly module.
///
/// The wasm frontend is not run over these; the module only has to be a plausible target
/// root with a `.wat` extension, which is what dispatch keys on.
#[cfg(test)]
pub(crate) fn wat_fixture(dir: &str, file: &str) -> std::path::PathBuf {
    fixture_source(dir, file, "(module)")
}

/// A synthesized project with no manifest on disk, wrapping a single target.
pub struct VirtualProject {
    package: Arc<ProjectPackage>,
    target: Target,
    profile: Profile,
    dependency_graph: ProjectDependencyGraph,
    store: NoPackageStore,
    source_manager: Arc<dyn SourceManager>,
}

impl VirtualProject {
    /// Synthesize a project named `name` with a single target rooted at `target_root`.
    ///
    /// Executable targets use [`Target::executable`], whose namespace is `$exec`. This
    /// must match the root module path the backend produces, or assembly fails the
    /// namespace check in `load_target_sources`.
    pub fn new(name: &str, target_root: &FsPath, target_type: TargetType) -> CompilerResult<Self> {
        let uri = Uri::from(target_root);
        let target = if target_type.is_executable() {
            Target::executable(name, uri)
        } else {
            let namespace = miden_assembly_syntax::Path::new(name)
                .to_absolute()
                .map(|path| Arc::from(path.into_owned()))
                .map_err(|err| Report::msg(format!("invalid namespace '{name}': {err}")))?;
            Target::library(namespace, uri)
        };

        let package: Arc<ProjectPackage> = Arc::from(ProjectPackage::new(name, target.clone()));
        let source_manager: Arc<dyn SourceManager> = Arc::new(DefaultSourceManager::default());
        let store = NoPackageStore;
        let dependency_graph = ProjectDependencyGraphBuilder::new(&store)
            .with_source_manager(source_manager.clone())
            .build(package.clone())?;

        Ok(Self {
            package,
            target,
            profile: Profile::default(),
            dependency_graph,
            store,
            source_manager,
        })
    }

    /// Build the assembler-facing context for this project's target.
    pub fn assembly_context(&self) -> CompilerResult<TargetAssemblyContext<'_>> {
        TargetAssemblyContext::new_virtual(
            self.package.clone(),
            &self.target,
            &self.profile,
            &self.dependency_graph,
            &self.store,
            self.source_manager.clone(),
        )
    }

    /// The (empty) dependency graph.
    pub fn dependency_graph(&self) -> &ProjectDependencyGraph {
        &self.dependency_graph
    }

    /// The synthesized package.
    pub fn package(&self) -> Arc<ProjectPackage> {
        self.package.clone()
    }

    /// The build profile, `dev` by default.
    pub fn profile(&self) -> &Profile {
        &self.profile
    }

    /// The canonical source manager.
    pub fn source_manager(&self) -> Arc<dyn SourceManager> {
        self.source_manager.clone()
    }

    /// The single target of this project.
    pub fn target(&self) -> &Target {
        &self.target
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn virtual_project_has_no_manifest_path() {
        let root = wat_fixture("no_manifest", "lib.wat");
        let project = VirtualProject::new("fixture", &root, TargetType::Library)
            .expect("should build virtual project");
        assert!(
            project.package().manifest_path().is_none(),
            "a virtual project must have no manifest path, or the dependency graph classifies it \
             as a real source and starts computing provenance"
        );
    }

    #[test]
    fn assembly_context_resolves_the_target_root_and_uses_the_empty_sentinel() {
        let root = wat_fixture("ctx", "lib.wat");
        let project = VirtualProject::new("fixture", &root, TargetType::Library)
            .expect("should build virtual project");
        let cx = project.assembly_context().expect("should build assembly context");
        assert_eq!(cx.resolved_target_root.extension().and_then(|e| e.to_str()), Some("wat"));
        assert!(
            cx.manifest_path.as_os_str().is_empty(),
            "new_virtual uses an empty path sentinel for manifest_path"
        );
    }

    #[test]
    fn executable_virtual_targets_use_the_exec_namespace() {
        let root = wat_fixture("exe", "main.wat");
        let project = VirtualProject::new("fixture_exe", &root, TargetType::Executable)
            .expect("should build virtual project");
        assert_eq!(
            project.target().namespace.inner().as_ref(),
            miden_assembly_syntax::Path::exec_path(),
            "executable targets must use $exec, matching Module::new_executable"
        );
    }
}
