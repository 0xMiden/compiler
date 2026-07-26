//! Construction helpers for compiling without a project manifest on disk.
//!
//! Used by tests today, and by input preparation for standalone inputs from increment 4.
//! Standalone inputs still run the legacy [`Stage`](crate::Stage) chains, so nothing outside
//! tests synthesizes a project yet.

use alloc::{format, sync::Arc, vec, vec::Vec};
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

/// An **empty** directory at `<temp>/midenc-pipeline-fixtures/<dir>`, for a test to emit into.
///
/// Emptied on each call, unlike [`fixture_source`]'s directories: a test asserting on *what a
/// renderer wrote* — and especially one asserting that nothing was written — would otherwise
/// be satisfied, or defeated, by a leftover from an earlier run. `dir` must therefore be
/// unique per test, and must not name a directory holding fixture sources.
#[cfg(test)]
pub(crate) fn fixture_dir(dir: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join("midenc-pipeline-fixtures").join(dir);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("should create fixture output dir");
    dir
}

/// A [`fixture_source`] holding the smallest valid WebAssembly module.
///
/// The wasm frontend is not run over these; the module only has to be a plausible target
/// root with a `.wat` extension, which is what dispatch keys on.
#[cfg(test)]
pub(crate) fn wat_fixture(dir: &str, file: &str) -> std::path::PathBuf {
    fixture_source(dir, file, "(module)")
}

/// Construct the target named `name`, rooted at `target_root`, of type `target_type`.
///
/// Executable targets use [`Target::executable`], whose namespace is `$exec`. This must
/// match the root module path the backend produces, or assembly fails the namespace check
/// in `load_target_sources`. Library targets use [`Target::library`], which derives the
/// target's name from the absolutized namespace, so a library named `foo` has the target
/// name `::foo` — distinct from the `foo` an executable of the same package gets, which is
/// what lets one package hold both.
fn synthesize_target(
    name: &str,
    target_root: &FsPath,
    target_type: TargetType,
) -> CompilerResult<Target> {
    let uri = Uri::from(target_root);
    if target_type.is_executable() {
        return Ok(Target::executable(name, uri));
    }
    let namespace = miden_assembly_syntax::Path::new(name)
        .to_absolute()
        .map(|path| Arc::from(path.into_owned()))
        .map_err(|err| Report::msg(format!("invalid namespace '{name}': {err}")))?;
    Ok(Target::library(namespace, uri))
}

/// A synthesized project with no manifest on disk.
pub struct VirtualProject {
    package: Arc<ProjectPackage>,
    /// This project's targets, the selected one first.
    ///
    /// Never empty: every constructor supplies the package's default target as the first
    /// element, which is what [`VirtualProject::target`] returns.
    targets: Vec<Target>,
    profile: Profile,
    dependency_graph: ProjectDependencyGraph,
    store: NoPackageStore,
    source_manager: Arc<dyn SourceManager>,
}

impl VirtualProject {
    /// Synthesize a project named `name` with a single target rooted at `target_root`.
    ///
    /// See `synthesize_target` for how the target's namespace is derived.
    pub fn new(name: &str, target_root: &FsPath, target_type: TargetType) -> CompilerResult<Self> {
        let target = synthesize_target(name, target_root, target_type)?;
        let package = ProjectPackage::new(name, target.clone());
        Self::assemble(Arc::from(package), vec![target])
    }

    /// Synthesize a project named `name` with *both* an executable and a library target.
    ///
    /// The executable is the package's default target and comes first; the library is the
    /// implicit one an executable of the same package links against. Both belong to a
    /// single `Arc<ProjectPackage>`, which is the point: the assembler hands the root and
    /// required-library callbacks clones of that same `Arc`, so a role derivation can only
    /// tell them apart by their target.
    pub fn executable_and_library(
        name: &str,
        executable_root: &FsPath,
        library_root: &FsPath,
    ) -> CompilerResult<Self> {
        let executable = synthesize_target(name, executable_root, TargetType::Executable)?;
        let library = synthesize_target(name, library_root, TargetType::Library)?;
        let package = ProjectPackage::new(name, executable.clone()).with_targets([library.clone()]);
        Self::assemble(Arc::from(package), vec![executable, library])
    }

    /// Resolve `package`'s dependency graph and wrap it up with its `targets`.
    fn assemble(package: Arc<ProjectPackage>, targets: Vec<Target>) -> CompilerResult<Self> {
        let source_manager: Arc<dyn SourceManager> = Arc::new(DefaultSourceManager::default());
        let store = NoPackageStore;
        let dependency_graph = ProjectDependencyGraphBuilder::new(&store)
            .with_source_manager(source_manager.clone())
            .build(package.clone())?;

        Ok(Self {
            package,
            targets,
            profile: Profile::default(),
            dependency_graph,
            store,
            source_manager,
        })
    }

    /// Build the assembler-facing context for this project's selected target.
    pub fn assembly_context(&self) -> CompilerResult<TargetAssemblyContext<'_>> {
        self.assembly_context_for(self.target())
    }

    /// Build the assembler-facing context for `target`, which must be one of this project's.
    ///
    /// The context carries `Arc::clone` of this project's package, exactly as the assembler
    /// does for the root and required-library callbacks of one project.
    pub fn assembly_context_for<'a>(
        &'a self,
        target: &'a Target,
    ) -> CompilerResult<TargetAssemblyContext<'a>> {
        TargetAssemblyContext::new_virtual(
            self.package.clone(),
            target,
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

    /// The selected target of this project.
    pub fn target(&self) -> &Target {
        self.targets.first().expect("a virtual project always has at least one target")
    }

    /// Every target of this project, the selected one first.
    pub fn targets(&self) -> &[Target] {
        &self.targets
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
    fn one_package_can_hold_both_an_executable_and_a_library_target() {
        let exe_root = wat_fixture("both", "main.wat");
        let lib_root = wat_fixture("both", "lib.wat");
        let project = VirtualProject::executable_and_library("fixture", &exe_root, &lib_root)
            .expect("should build virtual project");

        let [executable, library] = project.targets() else {
            panic!("expected exactly two targets, got {}", project.targets().len());
        };
        assert!(executable.is_executable(), "the selected target comes first");
        assert!(library.is_library());
        assert_eq!(
            executable.namespace.inner().as_ref(),
            miden_assembly_syntax::Path::exec_path(),
            "executable targets must use $exec"
        );
        assert_eq!(
            library.namespace.inner().as_str(),
            "::fixture",
            "library targets use the absolutized package namespace"
        );
        assert_ne!(
            executable.name.inner(),
            library.name.inner(),
            "the two targets must have distinct names, or the package could not hold both"
        );

        // The assembler hands the root and required-library callbacks `Arc::clone` of one
        // package, so package identity alone cannot distinguish the two roles. Pin that the
        // fixture reproduces it, since the role derivation's use of `Arc::ptr_eq` rests on it.
        let exe_cx = project.assembly_context_for(executable).expect("executable context");
        let lib_cx = project.assembly_context_for(library).expect("library context");
        assert!(
            Arc::ptr_eq(&exe_cx.package, &lib_cx.package),
            "both targets' contexts must carry the very same package allocation"
        );
        assert_eq!(
            exe_cx.resolved_target_root.file_name().and_then(|n| n.to_str()),
            Some("main.wat")
        );
        assert_eq!(
            lib_cx.resolved_target_root.file_name().and_then(|n| n.to_str()),
            Some("lib.wat")
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
