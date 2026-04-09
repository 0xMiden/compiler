use alloc::{collections::BTreeMap, sync::Arc};
use core::fmt;

use miden_assembly::{
    Library, Path,
    ast::InvocationTarget,
    library::{LibraryExport, ProcedureExport},
};
use miden_core::Word;
use miden_mast_package::{Package, Section, SectionId};
use midenc_hir::{constants::ConstantData, dialects::builtin, interner::Symbol};
use midenc_session::{
    Emit, OutputMode, OutputType, Session, Writer,
    diagnostics::{IntoDiagnostic, Report, SourceSpan, Span, WrapErr},
};

use crate::{TraceEvent, lower::NativePtr, masm};

pub struct MasmComponent {
    pub id: builtin::ComponentId,
    /// The symbol name of the component initializer function
    ///
    /// This function is responsible for initializing global variables and writing data segments
    /// into memory at program startup, and at cross-context call boundaries (in callee prologue).
    pub init: Option<masm::InvocationTarget>,
    /// The symbol name of the program entrypoint, if this component is executable.
    ///
    /// If unset, it indicates that the component is a library, even if it could be made executable.
    pub entrypoint: Option<masm::InvocationTarget>,
    /// The kernel library to link against
    pub kernel: Option<masm::KernelLibrary>,
    /// The rodata segments of this component keyed by the offset of the segment
    pub rodata: Vec<Rodata>,
    /// The address of the start of the global heap
    pub heap_base: u32,
    /// The address of the `__stack_pointer` global, if such a global has been defined
    pub stack_pointer: Option<u32>,
    /// The set of modules in this component
    pub modules: Vec<Arc<masm::Module>>,
}

impl Emit for MasmComponent {
    fn name(&self) -> Option<Symbol> {
        None
    }

    fn output_type(&self, _mode: OutputMode) -> OutputType {
        OutputType::Masm
    }

    fn write_to<W: Writer>(
        &self,
        mut writer: W,
        mode: OutputMode,
        _session: &Session,
    ) -> anyhow::Result<()> {
        if mode != OutputMode::Text {
            anyhow::bail!("masm emission does not support binary mode");
        }
        writer.write_fmt(core::format_args!("{self}"))?;
        Ok(())
    }
}

/// Represents a read-only data segment, combined with its content digest
#[derive(Clone, PartialEq, Eq)]
pub struct Rodata {
    /// The component to which this read-only data segment belongs
    pub component: builtin::ComponentId,
    /// The content digest computed for `data`
    pub digest: Word,
    /// The address at which the data for this segment begins
    pub start: NativePtr,
    /// The raw binary data for this segment
    pub data: Arc<ConstantData>,
}
impl fmt::Debug for Rodata {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Rodata")
            .field("digest", &format_args!("{}", &self.digest))
            .field("start", &self.start)
            .field_with("data", |f| {
                f.debug_struct("ConstantData")
                    .field("len", &self.data.len())
                    .finish_non_exhaustive()
            })
            .finish()
    }
}
impl Rodata {
    pub fn size_in_bytes(&self) -> usize {
        self.data.len()
    }

    pub fn size_in_felts(&self) -> usize {
        self.data.len().next_multiple_of(4) / 4
    }

    pub fn size_in_words(&self) -> usize {
        self.size_in_felts().next_multiple_of(4) / 4
    }

    /// Attempt to convert this rodata object to its equivalent representation in felts
    ///
    /// See [Self::bytes_to_elements] for more details.
    pub fn to_elements(&self) -> Vec<miden_processor::Felt> {
        Self::bytes_to_elements(self.data.as_slice())
    }

    /// Attempt to convert the given bytes to their equivalent representation in felts
    ///
    /// The resulting felts will be in padded out to the nearest number of words, i.e. if the data
    /// only takes up 3 felts worth of bytes, then the resulting `Vec` will contain 4 felts, so that
    /// the total size is a valid number of words.
    pub fn bytes_to_elements(bytes: &[u8]) -> Vec<miden_processor::Felt> {
        use miden_processor::Felt;

        let mut felts = Vec::with_capacity(bytes.len() / 4);
        let mut iter = bytes.iter().copied().array_chunks::<4>();
        felts.extend(iter.by_ref().map(|chunk| Felt::new(u32::from_le_bytes(chunk) as u64)));
        let remainder = iter.into_remainder();
        if remainder.len() > 0 {
            let mut chunk = [0u8; 4];
            for (i, byte) in remainder.enumerate() {
                chunk[i] = byte;
            }
            felts.push(Felt::new(u32::from_le_bytes(chunk) as u64));
        }

        let size_in_felts = bytes.len().next_multiple_of(4) / 4;
        let size_in_words = size_in_felts.next_multiple_of(4) / 4;
        let padding = (size_in_words * 4).abs_diff(felts.len());
        felts.resize(felts.len() + padding, Felt::ZERO);
        debug_assert_eq!(felts.len() % 4, 0, "expected to be a valid number of words");
        felts
    }
}

inventory::submit! {
    midenc_session::CompileFlag::new("test_harness")
        .long("test-harness")
        .action(midenc_session::FlagAction::SetTrue)
        .help("If present, causes the code generator to emit extra code for the VM test harness")
        .help_heading("Testing")
}

impl fmt::Display for MasmComponent {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        use crate::intrinsics::INTRINSICS_MODULE_NAMES;

        for module in self.modules.iter() {
            // Skip printing the standard library modules and intrinsics modules to focus on the
            // user-defined modules and avoid the
            // stack overflow error when printing large programs
            // https://github.com/0xMiden/miden-formatting/issues/4
            let module_name = module.path().as_str();
            let module_name_trimmed = module_name.trim_start_matches("::");
            if INTRINSICS_MODULE_NAMES.contains(&module_name) {
                continue;
            }
            if module.is_in_namespace(Path::new("std"))
                || module_name_trimmed.starts_with("miden::core")
                || module_name_trimmed.starts_with("miden::protocol")
            {
                continue;
            } else {
                writeln!(f, "# mod {}\n", &module_name)?;
                writeln!(f, "{module}")?;
            }
        }
        Ok(())
    }
}

impl MasmComponent {
    /// Assemble this component into a Miden package.
    pub fn assemble(
        &self,
        link_libraries: &[Arc<Library>],
        link_packages: &BTreeMap<Symbol, Arc<Package>>,
        account_component_metadata_bytes: Option<&[u8]>,
        session: &Session,
    ) -> Result<Package, Report> {
        project_support::assemble(
            self,
            link_libraries,
            link_packages,
            account_component_metadata_bytes,
            session,
        )
    }

    /// Generate an executable module which when run expects the raw data segment data to be
    /// provided on the advice stack in the same order as initialization, and the operands of
    /// the entrypoint function on the operand stack.
    fn generate_main(
        &self,
        entrypoint: &InvocationTarget,
        emit_test_harness: bool,
        source_manager: Arc<dyn midenc_session::SourceManager + Send + Sync>,
    ) -> Result<Arc<masm::Module>, Report> {
        use masm::{Instruction as Inst, Op};

        let mut exe = Box::new(masm::Module::new_executable());
        let span = SourceSpan::default();
        let body = {
            let mut block = masm::Block::new(span, Vec::with_capacity(64));
            // Invoke component initializer, if present
            if let Some(init) = self.init.as_ref() {
                block.push(Op::Inst(Span::new(span, Inst::Exec(init.clone()))));
            }

            // Initialize test harness, if requested
            if emit_test_harness {
                self.emit_test_harness(&mut block);
            }

            // Invoke the program entrypoint
            block.push(Op::Inst(Span::new(
                span,
                Inst::Trace(TraceEvent::FrameStart.as_u32().into()),
            )));
            block.push(Op::Inst(Span::new(span, Inst::Exec(entrypoint.clone()))));
            block
                .push(Op::Inst(Span::new(span, Inst::Trace(TraceEvent::FrameEnd.as_u32().into()))));

            // Truncate the stack to 16 elements on exit
            let truncate_stack = {
                let name = masm::ProcedureName::new("truncate_stack").unwrap();
                let module = masm::LibraryPath::new("::miden::core::sys").unwrap();
                let qualified = masm::QualifiedProcedureName::new(module.as_path(), name);
                InvocationTarget::Path(Span::new(span, qualified.into_inner()))
            };
            block.push(Op::Inst(Span::new(span, Inst::Exec(truncate_stack))));
            block
        };
        let start = masm::Procedure::new(
            span,
            masm::Visibility::Public,
            masm::ProcedureName::main(),
            0,
            body,
        );
        exe.define_procedure(start, source_manager)
            .into_diagnostic()
            .wrap_err("failed to define executable `main` procedure")?;
        Ok(Arc::from(exe))
    }

    fn emit_test_harness(&self, block: &mut masm::Block) {
        use masm::{Instruction as Inst, IntValue, Op, PushValue};
        use miden_core::Felt;

        let span = SourceSpan::default();

        let pipe_words_to_memory = {
            let name = masm::ProcedureName::new("pipe_words_to_memory").unwrap();
            let module = masm::LibraryPath::new("::miden::core::mem").unwrap();
            let qualified = masm::QualifiedProcedureName::new(module.as_path(), name);
            InvocationTarget::Path(Span::new(span, qualified.into_inner()))
        };

        // Step 1: Get the number of initializers to run
        // => [inits] on operand stack
        block.push(Op::Inst(Span::new(span, Inst::AdvPush(1.into()))));

        // Step 2: Evaluate the initial state of the loop condition `inits > 0`
        // => [inits, inits]
        block.push(Op::Inst(Span::new(span, Inst::Dup0)));
        // => [inits > 0, inits]
        block.push(Op::Inst(Span::new(span, Inst::Push(PushValue::Int(IntValue::U8(0)).into()))));
        block.push(Op::Inst(Span::new(span, Inst::Gt)));

        // Step 3: Loop until `inits == 0`
        let mut loop_body = Vec::with_capacity(16);

        // State of operand stack on entry to `loop_body`: [inits]
        // State of advice stack on entry to `loop_body`: [dest_ptr, num_words, ...]
        //
        // Step 3a: Compute next value of `inits`, i.e. `inits'`
        // => [inits - 1]
        loop_body.push(Op::Inst(Span::new(span, Inst::SubImm(Felt::ONE.into()))));

        // Step 3b: Copy initializer data to memory
        // => [num_words, dest_ptr, inits']
        loop_body.push(Op::Inst(Span::new(span, Inst::AdvPush(2.into()))));
        // => [C, B, A, dest_ptr, inits'] on operand stack
        loop_body
            .push(Op::Inst(Span::new(span, Inst::Trace(TraceEvent::FrameStart.as_u32().into()))));
        loop_body.push(Op::Inst(Span::new(span, Inst::Exec(pipe_words_to_memory))));
        loop_body
            .push(Op::Inst(Span::new(span, Inst::Trace(TraceEvent::FrameEnd.as_u32().into()))));
        // Drop C, B, A
        loop_body.push(Op::Inst(Span::new(span, Inst::DropW)));
        loop_body.push(Op::Inst(Span::new(span, Inst::DropW)));
        loop_body.push(Op::Inst(Span::new(span, Inst::DropW)));
        // => [inits']
        loop_body.push(Op::Inst(Span::new(span, Inst::Drop)));

        // Step 3c: Evaluate loop condition `inits' > 0`
        // => [inits', inits']
        loop_body.push(Op::Inst(Span::new(span, Inst::Dup0)));
        // => [inits' > 0, inits']
        loop_body
            .push(Op::Inst(Span::new(span, Inst::Push(PushValue::Int(IntValue::U8(0)).into()))));
        loop_body.push(Op::Inst(Span::new(span, Inst::Gt)));

        // Step 4: Enter (or skip) loop
        block.push(Op::While {
            span,
            body: masm::Block::new(span, loop_body),
        });

        // Step 5: Drop `inits` after loop is evaluated
        block.push(Op::Inst(Span::new(span, Inst::Drop)));
    }
}

/// Attach serialized account component metadata to the assembled package.
fn attach_account_component_metadata(
    package: &mut Package,
    account_component_metadata_bytes: Option<&[u8]>,
) {
    if let Some(bytes) = account_component_metadata_bytes {
        package
            .sections
            .push(Section::new(SectionId::ACCOUNT_COMPONENT_METADATA, bytes.to_vec()));
    }
}

/// Rewrite library exports to preserve Wasm component-model interface names.
fn normalize_library_exports(package: &mut Package) -> Result<(), Report> {
    if !package.kind.is_library() {
        return Ok(());
    }

    let exports = recover_wasm_cm_interfaces(package.mast.as_ref());
    package.mast = Arc::new(Library::new(package.mast.mast_forest().clone(), exports)?);
    Ok(())
}

/// Extend the package advice map with the component's rodata segments.
fn extend_rodata_advice_map(package: &mut Package, rodata: &[Rodata]) {
    if rodata.is_empty() {
        return;
    }

    let advice_map = rodata.iter().map(|segment| (segment.digest, segment.to_elements())).collect();
    Arc::make_mut(&mut package.mast).extend_advice_map(advice_map);
}

// TODO: extract into a separate file
mod project_support {
    use alloc::{
        collections::{BTreeMap, BTreeSet},
        string::ToString,
        sync::Arc,
        vec::Vec,
    };

    use miden_assembly::{Assembler, ProjectSourceInputs, ProjectTargetSelector};
    use miden_mast_package::{Package as MastPackage, TargetType, Version};
    use miden_package_registry::{
        PackageProvider, PackageRecord, PackageRegistry, PackageStore, PackageVersions,
        Version as RegistryVersion, VersionRequirement,
    };
    use miden_project::{
        Dependency as ProjectDependency, DependencyVersionScheme, Linkage,
        Package as ProjectPackage, Target,
    };
    use midenc_session::{
        Session,
        diagnostics::{Report, Span},
    };

    use super::{
        MasmComponent, Package, Symbol, attach_account_component_metadata,
        extend_rodata_advice_map, normalize_library_exports,
    };

    /// Assemble a MASM component through the VM project assembler.
    pub(super) fn assemble(
        component: &MasmComponent,
        link_libraries: &[Arc<miden_assembly::Library>],
        link_packages: &BTreeMap<Symbol, Arc<MastPackage>>,
        account_component_metadata_bytes: Option<&[u8]>,
        session: &Session,
    ) -> Result<Package, Report> {
        let mut store = VirtualPackageStore::default();
        let dependencies =
            register_external_dependencies(&mut store, link_libraries, link_packages, session)?;
        let target = build_root_target(component)?;
        let mut assembler = Assembler::new(session.source_manager.clone());
        let sources = prepare_sources(
            component,
            &mut assembler,
            link_libraries,
            link_packages,
            session.get_flag("test_harness"),
            session.source_manager.clone(),
        )?;
        let mut project_assembler = assembler.for_project(
            Arc::<ProjectPackage>::from(
                ProjectPackage::new(session.name.clone(), target).with_dependencies(dependencies),
            ),
            &mut store,
        )?;

        let selector = if component.entrypoint.is_some() {
            ProjectTargetSelector::Executable("main")
        } else {
            ProjectTargetSelector::Library
        };
        let mut package = Arc::unwrap_or_clone(
            project_assembler.assemble_with_sources(selector, "dev", sources)?,
        );

        package.name = session.name.clone().into();
        attach_account_component_metadata(&mut package, account_component_metadata_bytes);
        extend_rodata_advice_map(&mut package, &component.rodata);
        normalize_library_exports(&mut package)?;
        Ok(package)
    }

    /// Register externally-linked artifacts in an in-memory package store.
    fn register_external_dependencies(
        store: &mut VirtualPackageStore,
        link_libraries: &[Arc<miden_assembly::Library>],
        link_packages: &BTreeMap<Symbol, Arc<MastPackage>>,
        session: &Session,
    ) -> Result<Vec<ProjectDependency>, Report> {
        if session.options.link_libraries.len() != link_libraries.len() {
            return Err(Report::msg(
                "loaded link libraries do not match the session link library configuration",
            ));
        }

        let mut dependencies = BTreeMap::default();
        for (link_lib, library) in session.options.link_libraries.iter().zip(link_libraries.iter())
        {
            let package = Arc::from(MastPackage::from_library(
                link_lib.name.to_string().into(),
                Version::new(0, 0, 0),
                TargetType::Library,
                library.clone(),
                [],
            ));
            let version = store.add_package(package)?;
            push_project_dependency(&mut dependencies, Arc::from(link_lib.name.as_ref()), version)?;
        }
        for package in link_packages.values() {
            let version = store.add_package(package.clone())?;
            push_project_dependency(&mut dependencies, package.name.clone().into_inner(), version)?;
        }

        Ok(dependencies.into_values().collect())
    }

    /// Append a project dependency while preserving the existing exact resolution.
    fn push_project_dependency(
        dependencies: &mut BTreeMap<Arc<str>, ProjectDependency>,
        name: Arc<str>,
        version: RegistryVersion,
    ) -> Result<(), Report> {
        let dependency = ProjectDependency::new(
            Span::unknown(name.clone()),
            DependencyVersionScheme::Registry(VersionRequirement::Exact(version)),
            Linkage::Dynamic,
        );

        match dependencies.get(name.as_ref()) {
            Some(existing) if existing == &dependency => Ok(()),
            Some(_) => Err(Report::msg(format!(
                "conflicting external dependency registration for '{name}'",
            ))),
            None => {
                dependencies.insert(name, dependency);
                Ok(())
            }
        }
    }

    /// Build the synthetic root target used to assemble compiler-generated MASM.
    fn build_root_target(component: &MasmComponent) -> Result<Target, Report> {
        if component.entrypoint.is_some() {
            return Ok(Target::executable("main"));
        }

        let root = component
            .modules
            .first()
            .ok_or_else(|| Report::msg("component does not contain any MASM modules"))?;
        Ok(Target::library(root.path()))
    }

    /// Prepare project source inputs while preserving the legacy assembler behavior for intrinsics.
    fn prepare_sources(
        component: &MasmComponent,
        assembler: &mut Assembler,
        link_libraries: &[Arc<miden_assembly::Library>],
        link_packages: &BTreeMap<Symbol, Arc<MastPackage>>,
        emit_test_harness: bool,
        source_manager: Arc<dyn midenc_session::SourceManager + Send + Sync>,
    ) -> Result<ProjectSourceInputs, Report> {
        let external_modules = external_module_paths(link_libraries, link_packages);

        // Intrinsics must be linked into the assembler context directly so they do not become
        // part of the assembled package surface.
        let mut support = Vec::with_capacity(component.modules.len());
        for module in component.modules.iter() {
            if external_modules.contains(module.path()) {
                log::warn!(
                    target: "assembly",
                    "module '{}' is already registered with the assembler as dependency module, \
                     skipping",
                    module.path()
                );
                continue;
            }

            if is_intrinsics_module(module) {
                log::debug!(
                    target: "assembly",
                    "adding intrinsics '{}' to assembler",
                    module.path()
                );
                assembler.compile_and_statically_link(module.clone())?;
                continue;
            }

            support.push(Box::new(Arc::unwrap_or_clone(module.clone())));
        }

        if let Some(entrypoint) = component.entrypoint.as_ref() {
            let root = Box::new(Arc::unwrap_or_clone(component.generate_main(
                entrypoint,
                emit_test_harness,
                source_manager,
            )?));
            return Ok(ProjectSourceInputs { root, support });
        }

        let mut modules = support.into_iter();
        let root = modules.next().ok_or_else(|| {
            Report::msg("component does not contain any user-defined MASM modules")
        })?;
        Ok(ProjectSourceInputs {
            root,
            support: modules.collect(),
        })
    }

    /// Return the set of modules already supplied by external dependencies.
    fn external_module_paths(
        link_libraries: &[Arc<miden_assembly::Library>],
        link_packages: &BTreeMap<Symbol, Arc<MastPackage>>,
    ) -> BTreeSet<miden_assembly::PathBuf> {
        let mut paths = BTreeSet::default();
        for library in link_libraries {
            for module in library.module_infos() {
                paths.insert(module.path().to_path_buf());
            }
        }
        for package in link_packages.values() {
            for module in package.mast.module_infos() {
                paths.insert(module.path().to_path_buf());
            }
        }
        paths
    }

    /// Return true when the module belongs to the compiler's intrinsics namespace.
    fn is_intrinsics_module(module: &miden_assembly::ast::Module) -> bool {
        module.path().as_str().trim_start_matches("::").starts_with("intrinsics")
    }

    /// A minimal in-memory package store for compiler-provided dependencies.
    #[derive(Default)]
    struct VirtualPackageStore {
        index: BTreeMap<miden_package_registry::PackageId, PackageVersions>,
        packages: BTreeMap<(miden_package_registry::PackageId, RegistryVersion), Arc<MastPackage>>,
    }

    impl VirtualPackageStore {
        /// Register a package and return its exact version.
        fn add_package(&mut self, package: Arc<MastPackage>) -> Result<RegistryVersion, Report> {
            let version = RegistryVersion::new(package.version.clone(), package.digest());
            let record = package_record(package.as_ref(), version.clone());

            if let Some(existing) = self
                .index
                .get(&package.name)
                .and_then(|versions| versions.get(&package.version))
            {
                if existing.version() != &version {
                    return Err(Report::msg(format!(
                        "package '{}' version '{}' is already registered",
                        package.name, package.version
                    )));
                }
            } else {
                self.index
                    .entry(package.name.clone())
                    .or_default()
                    .insert(package.version.clone(), record);
            }

            self.packages.insert((package.name.clone(), version.clone()), package);
            Ok(version)
        }
    }

    impl PackageRegistry for VirtualPackageStore {
        fn available_versions(
            &self,
            package: &miden_package_registry::PackageId,
        ) -> Option<&PackageVersions> {
            self.index.get(package)
        }
    }

    impl PackageProvider for VirtualPackageStore {
        fn load_package(
            &self,
            package: &miden_package_registry::PackageId,
            version: &RegistryVersion,
        ) -> Result<Arc<MastPackage>, Report> {
            self.packages
                .get(&(package.clone(), version.clone()))
                .cloned()
                .ok_or_else(|| Report::msg(format!("missing package '{package}' at '{version}'")))
        }
    }

    impl PackageStore for VirtualPackageStore {
        type Error = Report;

        fn publish_package(
            &mut self,
            package: Arc<MastPackage>,
        ) -> Result<RegistryVersion, Self::Error> {
            self.add_package(package)
        }
    }

    /// Build the registry metadata record for a package.
    fn package_record(package: &MastPackage, version: RegistryVersion) -> PackageRecord {
        let dependencies = package.manifest.dependencies().map(|dependency| {
            (
                dependency.name.clone(),
                VersionRequirement::Exact(RegistryVersion::new(
                    dependency.version.clone(),
                    dependency.digest,
                )),
            )
        });

        match package.description.as_deref() {
            Some(description) => {
                PackageRecord::new(version, dependencies).with_description(description)
            }
            None => PackageRecord::new(version, dependencies),
        }
    }
}

/// Try to recognize Wasm CM interfaces and transform those exports to have Wasm interface encoded
/// as module name.
///
/// Temporary workaround for:
///
/// 1. Temporary exporting multiple interfaces from the same(Wasm core) module (an interface is
///    encoded in the function name)
///
/// 2. Assembler using the current module name to generate exports.
///
fn recover_wasm_cm_interfaces(lib: &Library) -> BTreeMap<Arc<Path>, LibraryExport> {
    use crate::intrinsics::INTRINSICS_MODULE_NAMES;

    let mut exports = BTreeMap::new();
    for export in lib.exports() {
        let path = export.path();
        let Some(proc_export) = export.as_procedure() else {
            exports.insert(path, export.clone());
            continue;
        };

        let Some(module) = proc_export.path.parent() else {
            exports.insert(path, export.clone());
            continue;
        };
        let Some(proc_name) = proc_export.path.last() else {
            exports.insert(path, export.clone());
            continue;
        };

        if INTRINSICS_MODULE_NAMES.contains(&module.as_str()) || proc_name.starts_with("cabi") {
            // Preserve intrinsics modules and internal Wasm CM `cabi_*` functions
            exports.insert(path, export.clone());
            continue;
        }

        if let Some((component, interface)) = proc_name.rsplit_once('/') {
            // Wasm CM interface
            let (interface, function) =
                interface.rsplit_once('#').expect("invalid wasm component model identifier");

            // Derive a new module path in which the Wasm CM interface name is encoded as part of
            // the module path, rather than being encoded in the procedure name.
            let mut module_path = component.to_string();
            module_path.push_str("::");
            module_path.push_str(interface);
            let module_path = masm::LibraryPath::new(&module_path)
                .expect("invalid wasm component model identifier");

            let name = masm::ProcedureName::from_raw_parts(masm::Ident::from_raw_parts(
                Span::unknown(Arc::from(function)),
            ));
            let qualified = masm::QualifiedProcedureName::new(module_path.as_path(), name);
            let qualified = qualified.into_inner();

            let mut new_export = ProcedureExport::new(proc_export.node, qualified.clone())
                .with_attributes(proc_export.attributes.clone());
            if let Some(signature) = proc_export.signature.clone() {
                new_export = new_export.with_signature(signature);
            }

            exports.insert(qualified, LibraryExport::Procedure(new_export));
        } else {
            // Non-Wasm CM interface, preserve as is
            exports.insert(path, export.clone());
        }
    }
    exports
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn validate_bytes_to_elements(bytes: &[u8]) {
        let result = Rodata::bytes_to_elements(bytes);

        // Each felt represents 4 bytes
        let expected_felts = bytes.len().div_ceil(4);
        // Felts should be padded to a multiple of 4 (1 word = 4 felts)
        let expected_total_felts = expected_felts.div_ceil(4) * 4;

        assert_eq!(
            result.len(),
            expected_total_felts,
            "For {} bytes, expected {} felts (padded from {} felts), but got {}",
            bytes.len(),
            expected_total_felts,
            expected_felts,
            result.len()
        );

        // Verify padding is zeros
        for (i, felt) in result.iter().enumerate().skip(expected_felts) {
            assert_eq!(*felt, miden_processor::Felt::ZERO, "Padding at index {i} should be zero");
        }
    }

    #[test]
    fn test_bytes_to_elements_edge_cases() {
        validate_bytes_to_elements(&[]);
        validate_bytes_to_elements(&[1]);
        validate_bytes_to_elements(&[0u8; 4]);
        validate_bytes_to_elements(&[0u8; 15]);
        validate_bytes_to_elements(&[0u8; 16]);
        validate_bytes_to_elements(&[0u8; 17]);
        validate_bytes_to_elements(&[0u8; 31]);
        validate_bytes_to_elements(&[0u8; 32]);
        validate_bytes_to_elements(&[0u8; 33]);
        validate_bytes_to_elements(&[0u8; 64]);
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(1000))]
        #[test]
        fn proptest_bytes_to_elements(bytes in prop::collection::vec(any::<u8>(), 0..=1000)) {
            validate_bytes_to_elements(&bytes);
        }

        #[test]
        fn proptest_bytes_to_elements_word_boundaries(size_factor in 0u32..=100) {
            // Test specifically around word boundaries
            // Test sizes around multiples of 16 (since 1 word = 4 felts = 16 bytes)
            let base_size = size_factor * 16;
            for offset in -2i32..=2 {
                let size = (base_size as i32 + offset).max(0) as usize;
                let bytes = vec![0u8; size];
                validate_bytes_to_elements(&bytes);
            }
        }
    }
}
