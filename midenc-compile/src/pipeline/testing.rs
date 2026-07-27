//! Construction helpers for compiling without a project manifest on disk.
//!
//! `synthesize_target` is shared with
//! [`prepare_standalone`](crate::pipeline::prepare_standalone), which builds the project a
//! real standalone request compiles. Everything else here is for tests: [`VirtualProject`]
//! materializes a manifest and an assembly context for one, which is what lets a frontend or
//! the shared backend be driven without a project on disk.

use alloc::{format, sync::Arc, vec, vec::Vec};
use std::path::Path as FsPath;

use miden_assembly::TargetAssemblyContext;
use miden_assembly_syntax::Path as MasmPath;
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

/// A context whose session writes `--emit=hir` into `out_dir`.
#[cfg(test)]
pub(crate) fn hir_emitting_context(out_dir: &FsPath) -> alloc::rc::Rc<midenc_hir::Context> {
    use midenc_session::{OutputFile, OutputType, OutputTypeSpec, OutputTypes};

    let output_types = OutputTypes::new([OutputTypeSpec::Typed {
        output_type: OutputType::Hir,
        path: Some(OutputFile::Directory(out_dir.to_path_buf())),
    }])
    .expect("hir is a valid output type");
    context_emitting(output_types, |_| {})
}

/// Every `.hir` document in `dir`, as `(file stem, contents)`, sorted by stem.
///
/// Counting *documents on disk* is what makes the emission testable at all, and it has a
/// limit worth stating where the helper lives: [`Emit`](midenc_session::Emit) for a `String`
/// reports no name, so every HIR emission in one session resolves to the same destination and
/// `write_to_file` truncates it. A document written twice therefore looks exactly like one
/// written once. This helper proves an emission happened; it cannot prove it happened only
/// once. `this_route_names_the_hir_emission_once`, in each frontend's tests, covers that half
/// by counting the emission's call sites in the source instead.
#[cfg(test)]
pub(crate) fn hir_documents(dir: &FsPath) -> Vec<(alloc::string::String, alloc::string::String)> {
    use alloc::string::ToString;

    let mut documents = std::fs::read_dir(dir)
        .expect("the output directory should exist")
        .map(|entry| entry.expect("should read a directory entry").path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("hir"))
        .map(|path| {
            let stem = path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .expect("a written document has a name")
                .to_string();
            (stem, std::fs::read_to_string(&path).expect("should read the document"))
        })
        .collect::<Vec<_>>();
    documents.sort();
    documents
}

/// A context whose session emits `output_types` and was otherwise configured by `configure`.
///
/// Silent, so that a test which raises diagnostics does not write them into the test output.
/// Silencing does not hide them from `has_errors()`: the error counter is bumped before the
/// emitter is consulted.
#[cfg(test)]
pub(crate) fn context_emitting(
    output_types: midenc_session::OutputTypes,
    configure: impl FnOnce(&mut midenc_session::Options),
) -> alloc::rc::Rc<midenc_hir::Context> {
    use alloc::{boxed::Box, rc::Rc};

    use midenc_session::{InputFile, Options, Session, Verbosity};

    let mut options = Box::new(Options::default());
    configure(&mut options);
    let options = options.with_verbosity(Verbosity::Silent).with_output_types(output_types, None);
    let source_manager = Arc::new(DefaultSourceManager::default());
    let session = Session::new(InputFile::empty(), options, None, source_manager)
        .expect("should build a session");
    Rc::new(midenc_hir::Context::new(Rc::new(session)))
}

/// The smallest [`MidenComponent`] the backend accepts, built in `context`.
///
/// A single public `main` whose body is just `ret`. Adapted from the passing test in
/// `midenc-compile/tests/codegen_legalization.rs`, and shared because the backend's own tests
/// and the seed's alike need a component that survives analysis, rewrites and codegen without
/// tripping any of them.
///
/// `context` is taken rather than created so that a caller can supply a session — the lint
/// flag, the requested output types — and have the component belong to it.
#[cfg(test)]
pub(crate) fn minimal_component(
    context: &alloc::rc::Rc<midenc_hir::Context>,
) -> crate::MidenComponent {
    component_in_namespace(context, "test_ns", None, None)
}

/// The component name [`component_in_namespace`] builds under, and its version.
///
/// Constants rather than literals because they are half of the component's id, and a target
/// that assembles what this module builds has to name that whole id — see
/// [`component_target_namespace`].
#[cfg(test)]
const COMPONENT_NAME: &str = "test";
#[cfg(test)]
const COMPONENT_VERSION: (u64, u64, u64) = (1, 0, 0);

/// The target namespace a project must declare to assemble what
/// [`component_in_namespace`]`(_, namespace, ..)` produces.
///
/// Codegen roots a component's Miden Assembly at its *id* — namespace, name and version
/// together, as one quoted path component — and the assembler refuses to assemble a target
/// whose root module sits anywhere but the target's own namespace. So a manifest for such a
/// target reads `namespace = "test_ns:test@1.0.0"`, not `namespace = "test_ns"`. Real projects
/// look the same: `tests/fixtures/components/cross-ctx-account` declares
/// `namespace = "miden:cross-ctx-account/foo@1.0.0"`.
#[cfg(test)]
pub(crate) fn component_target_namespace(namespace: &str) -> alloc::string::String {
    let (major, minor, patch) = COMPONENT_VERSION;
    alloc::format!("{namespace}:{COMPONENT_NAME}@{major}.{minor}.{patch}")
}

/// [`minimal_component`], in `namespace`, carrying `rodata` and `metadata` if given.
///
/// The namespace is a parameter because a component assembled as part of a project must be
/// lowered into the *target's* namespace: codegen derives the root Miden Assembly module path
/// from the component's id, and the assembler rejects a root module that does not sit exactly
/// at the target's own namespace. The id is not the namespace alone, so a caller that assembles
/// what this builds must declare its target's namespace as
/// [`component_target_namespace(namespace)`](component_target_namespace) rather than
/// `namespace`.
///
/// `rodata`, when given, is planted as a read-only [`builtin::Segment`](midenc_hir::dialects::builtin::Segment)
/// at offset 0 of linear memory. That is what makes codegen produce a
/// [`Rodata`](midenc_codegen_masm::Rodata) segment, and therefore what makes the rodata advice
/// map assembly attaches to the package non-empty. `metadata` stands in for the serialized
/// account-component metadata a real account-component target carries; nothing validates it,
/// so arbitrary bytes suffice to observe whether the section survives to the package.
#[cfg(test)]
pub(crate) fn component_in_namespace(
    context: &alloc::rc::Rc<midenc_hir::Context>,
    namespace: &str,
    rodata: Option<&[u8]>,
    metadata: Option<Vec<u8>>,
) -> crate::MidenComponent {
    use miden_assembly::{ProjectSourceProvenanceInputs, SourceFileProvenance};
    use midenc_hir::{
        BuilderExt, Ident, OpBuilder, SourceSpan, Visibility,
        dialects::builtin::{
            self, BuiltinOpBuilder, ComponentBuilder, FunctionBuilder, ModuleBuilder, WorldBuilder,
            attributes::Signature,
        },
        version::Version,
    };

    let mut builder = OpBuilder::new(context.clone());
    let world = builder.create::<builtin::World, ()>(SourceSpan::UNKNOWN)().unwrap();
    let mut world_builder = WorldBuilder::new(world);
    let component = world_builder
        .define_component(
            Ident::with_empty_span(midenc_hir::interner::Symbol::intern(namespace)),
            Ident::with_empty_span(COMPONENT_NAME.into()),
            Version::new(COMPONENT_VERSION.0, COMPONENT_VERSION.1, COMPONENT_VERSION.2),
        )
        .unwrap();

    let mut component_builder = ComponentBuilder::new(component);
    let module = component_builder.define_module(Ident::with_empty_span("test".into())).unwrap();
    let signature = Signature::new(context, [], []);
    let mut module_builder = ModuleBuilder::new(module);
    if let Some(rodata) = rodata {
        module_builder
            .define_data_segment(0, rodata.to_vec(), true, SourceSpan::UNKNOWN)
            .expect("should define the data segment");
    }
    let function = module_builder
        .define_function(Ident::with_empty_span("main".into()), Visibility::Public, signature)
        .unwrap();

    let mut builder = OpBuilder::new(context.clone());
    let mut function_builder = FunctionBuilder::new(function, &mut builder);
    function_builder.ret(None, SourceSpan::UNKNOWN).unwrap();

    crate::MidenComponent {
        world,
        component: Some(component),
        account_component_metadata_bytes: metadata,
        source_provenance: ProjectSourceProvenanceInputs {
            root: SourceFileProvenance {
                path: FsPath::new(file!()).to_path_buf().into_boxed_path(),
                content: alloc::string::String::new().into_boxed_str(),
            },
            support: Default::default(),
        },
    }
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
/// Shared with [`prepare_standalone`](crate::pipeline::prepare_standalone), which synthesizes
/// the target of a real standalone build, so everything below is a correctness requirement
/// rather than a fixture detail.
///
/// # The namespace is decided by the target type
///
/// Two of the six types are rooted at a reserved namespace, and each is reserved because
/// something addresses it by that name:
///
/// - **Executables** are rooted at `$exec`, the path `MasmComponent::source_inputs` gives the
///   module it builds with `Module::new_executable`. `load_target_sources` rejects a root
///   module that does not sit exactly at its target's namespace, so a name-derived namespace
///   fails every executable.
/// - **Kernels** are rooted at `$kernel`, mirroring the manifest, which maps
///   `TargetType::Kernel` to `Path::kernel_path()` unconditionally and never derives a kernel's
///   namespace from a name. It has to: semantic analysis rewrites every `syscall.foo` to
///   `$kernel::foo`, so a kernel assembled elsewhere exports procedures no `syscall` can
///   address, and the linker's `link_with_kernel` asserts that a kernel package contains a
///   module whose path `is_kernel_path()`.
///
/// The remaining four — library, account component, note and transaction script — take the
/// absolutized `name`, and their target *name* is derived from that namespace, so a library
/// named `foo` has the target name `::foo`. That is what distinguishes it from the `foo` an
/// executable of the same package gets, and therefore what lets one package hold both. The two
/// reserved-namespace types keep the caller's `name` instead: their namespace is a sentinel
/// shared by every target of that type and could not identify one.
///
/// # Why not `Target::executable` and `Target::library`
///
/// Both are thin wrappers over [`Target::new`] that hardcode a target type, and `Target::library`
/// hardcodes `TargetType::Library` — so routing the four library-like types through it silently
/// discards the requested type. `assemble_source_package` asserts the assembled package's kind
/// equals its target's, so that would emit a `Library`-kind package for an account component.
/// The wrappers' *shapes* are reproduced exactly; only the type is carried through.
pub(crate) fn synthesize_target(
    name: &str,
    target_root: &FsPath,
    target_type: TargetType,
) -> CompilerResult<Target> {
    let uri = Uri::from(target_root);
    match target_type {
        TargetType::Executable => Ok(Target::new(target_type, name, MasmPath::exec_path(), uri)),
        TargetType::Kernel => Ok(Target::new(target_type, name, MasmPath::kernel_path(), uri)),
        _ => {
            let namespace: Arc<MasmPath> = MasmPath::new(name)
                .to_absolute()
                .map(|path| Arc::from(path.into_owned()))
                .map_err(|err| Report::msg(format!("invalid namespace '{name}': {err}")))?;
            let derived_name: Arc<str> = namespace.as_str().into();
            Ok(Target::new(target_type, derived_name, namespace, uri))
        }
    }
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

    /// The target a standalone preparation synthesized, ready to be served by a frontend.
    ///
    /// This is what lets a test hold preparation and a frontend to the *same* namespace —
    /// the invariant `load_target_sources` enforces — without either side restating it.
    ///
    /// Preparation's package is not reused verbatim: it carries the `miden-core` registry
    /// dependency every standalone build links against, and [`NoPackageStore`] cannot resolve
    /// it, so building its dependency graph fails outright. A target's namespace has nothing
    /// to do with its dependency closure, so the target is lifted into a package of its own
    /// under the same name.
    #[cfg(test)]
    pub(crate) fn for_prepared_target(
        prepared: &crate::pipeline::PreparedProject,
    ) -> CompilerResult<Self> {
        let target = prepared.target.clone();
        let package = ProjectPackage::new(prepared.package.name().inner().as_ref(), target.clone());
        Self::assemble(Arc::from(package), vec![target])
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
            MasmPath::exec_path(),
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
            MasmPath::exec_path(),
            "executable targets must use $exec, matching Module::new_executable"
        );
    }
}
