use alloc::{boxed::Box, sync::Arc, vec::Vec};
use core::cell::RefCell;

use miden_assembly::{
    DefaultSourceManager, ProjectSourceInputs, ProjectSourceProvider, ProjectTargetSelector,
    ResolvedPackage, utils::DisplayHex,
};
use miden_mast_package::{Package, TargetType, Version};
use midenc_codegen_masm::{MasmComponent, intrinsics};
use midenc_hir::FxHashMap;
use midenc_session::PackageId;

use super::*;

/// The artifact produced by the full compiler pipeline.
///
/// The type of artifact depends on what outputs were requested, and what options were specified.
pub enum Artifact {
    Lowered(CodegenOutput),
    Assembled(Arc<Package>),
}
impl Artifact {
    pub fn unwrap_mast(self) -> Arc<Package> {
        match self {
            Self::Assembled(mast) => mast,
            Self::Lowered(_) => {
                panic!("expected 'mast' artifact, but assembler stage was not run")
            }
        }
    }
}

/// Perform assembly of the generated Miden Assembly, producing MAST
pub struct AssembleStage;

impl Stage for AssembleStage {
    type Input = CodegenOutput;
    type Output = Artifact;

    fn run(&mut self, input: Self::Input, context: Rc<Context>) -> CompilerResult<Self::Output> {
        use midenc_hir::formatter::DisplayHex;

        let session = context.session_rc();
        if !session.should_assemble() {
            log::debug!(
                "skipping assembly of mast package from masm artifact (should-assemble=false)"
            );
            return Ok(Artifact::Lowered(input));
        }

        log::debug!("assembling package");

        let project_package = session.project.package();
        let is_executable_target = session.options.target_type.is_some_and(|tt| tt.is_executable())
            || project_package.library_target().is_none()
            || session.options.target.as_deref().is_some_and(|tname| {
                project_package.executable_targets().iter().any(|t| tname == &**t.name)
            });
        let selector = if is_executable_target {
            ProjectTargetSelector::Executable(selected_executable_target_name(
                project_package.as_ref(),
                &session,
            )?)
        } else {
            ProjectTargetSelector::Library
        };
        let package_id = project_package.name().into_inner();
        let version = project_package.version().into_inner().clone();

        let mut registry = session.package_registry()?;
        let package = if project_package.manifest_path().is_some() {
            assemble_project_with_registry(
                project_package.clone(),
                selector,
                &session,
                &mut registry,
                [Box::new(RustSourceProvider {
                    session: session.clone(),
                    compiled: RefCell::new(FxHashMap::from_iter([((package_id, version), input)])),
                }) as Box<dyn ProjectSourceProvider>],
            )?
        } else {
            let sources = input.clone();
            assemble_virtual_project_with_registry(
                project_package.clone(),
                selector,
                sources,
                &session,
                &mut registry,
                [Box::new(RustSourceProvider {
                    session: session.clone(),
                    compiled: RefCell::new(FxHashMap::default()),
                }) as Box<dyn ProjectSourceProvider>],
            )?
        };

        log::debug!(
            "successfully assembled package with digest {}",
            DisplayHex::new(&package.digest().as_bytes())
        );
        Ok(Artifact::Assembled(package))
    }
}

/// Perform assembly of a Miden Assembly project
pub struct AssembleProjectStage;

impl Stage for AssembleProjectStage {
    type Input = Option<MasmSources>;
    type Output = Artifact;

    fn run(&mut self, input: Self::Input, context: Rc<Context>) -> CompilerResult<Self::Output> {
        let session = context.session();
        let package = session.project.package();
        let mut registry = session.package_registry()?;

        let package = match input {
            Some(sources) => {
                let mut assembler = miden_assembly::Assembler::new(session.source_manager.clone())
                    .with_warnings_as_errors(
                        session.options.diagnostics.warnings.warnings_as_errors(),
                    );

                prepare_assembler(&mut assembler, &package, session)?;

                let selector = if session.options.target_type.unwrap_or_default().is_executable() {
                    ProjectTargetSelector::Executable(session.name.as_str())
                } else {
                    ProjectTargetSelector::Library
                };

                let target = selector.select_target(&package)?;
                let package_id = package.target_package_name(&target);
                match target.ty {
                    TargetType::Executable => {
                        assembler.compile_and_statically_link_all(sources.inputs.support)?;
                        assembler
                            .assemble_program(package_id, sources.inputs.root)
                            .map(Arc::from)?
                    }
                    TargetType::Kernel => assembler
                        .assemble_kernel(package_id, sources.inputs.root, sources.inputs.support)
                        .map(Arc::from)?,
                    _ => assembler
                        .assemble_library(package_id, sources.inputs.root, sources.inputs.support)
                        .map(Arc::from)?,
                }
            }
            None => {
                let selector = if session.options.target_type.unwrap_or_default().is_executable() {
                    ProjectTargetSelector::Executable(session.name.as_str())
                } else {
                    ProjectTargetSelector::Library
                };
                assemble_project_with_registry(
                    package,
                    selector,
                    session,
                    &mut registry,
                    [Box::new(RustSourceProvider {
                        session: context.session_rc(),
                        compiled: RefCell::new(FxHashMap::default()),
                    }) as Box<dyn ProjectSourceProvider>],
                )?
            }
        };

        log::debug!(
            "successfully assembled package with digest {}",
            DisplayHex::new(&package.digest().as_bytes())
        );

        Ok(Artifact::Assembled(package))
    }
}

pub struct RustSourceProvider {
    pub session: Rc<Session>,
    pub compiled: RefCell<FxHashMap<(PackageId, Version), CodegenOutput>>,
}

impl ProjectSourceProvider for RustSourceProvider {
    fn file_type(&self) -> &'static str {
        "rs"
    }

    fn provide_sources(
        &self,
        context: &miden_assembly::TargetAssemblyContext<'_>,
    ) -> Result<ProjectSourceInputs, Report> {
        let package_id = context.package.name().into_inner();
        let version = context.package.version().into_inner().clone();
        let key = (package_id, version);
        {
            let compiled = self.compiled.borrow();
            if let Some(found) = compiled.get(&key) {
                return found.component.source_inputs(context.target, &self.session);
            }
        }

        let filesystem_cache_dir = self
            .session
            .project
            .manifest_path()
            .and_then(|p| p.parent())
            .map(|p| p.join("target").join("miden").join("packages"));
        let cargo_opts = crate::cargo::CargoOptions::from_compiler(&self.session.options)?;
        let source_manager = Arc::new(DefaultSourceManager::default());
        let compiled = crate::cargo::cargo_build(
            context.package.clone(),
            context.target,
            context.manifest_path.with_file_name("Cargo.toml"),
            filesystem_cache_dir.as_deref(),
            &self.session.options,
            &cargo_opts,
            source_manager,
        )?;

        let source_inputs = compiled.component.source_inputs(context.target, &self.session)?;

        self.compiled.borrow_mut().insert(key, compiled);

        Ok(source_inputs)
    }

    fn provide_source_provenance(
        &self,
        context: &miden_assembly::TargetAssemblyContext<'_>,
    ) -> Result<miden_assembly::ProjectSourceProvenanceInputs, Report> {
        let package_id = context.package.name().into_inner();
        let version = context.package.version().into_inner().clone();
        let key = (package_id, version);
        {
            let compiled = self.compiled.borrow();
            if let Some(found) = compiled.get(&key) {
                return Ok(found.source_provenance());
            }
        }

        let filesystem_cache_dir = self
            .session
            .project
            .manifest_path()
            .and_then(|p| p.parent())
            .map(|p| p.join("target").join("miden").join("packages"));
        let cargo_opts = crate::cargo::CargoOptions::from_compiler(&self.session.options)?;
        let source_manager = Arc::new(DefaultSourceManager::default());
        let compiled = crate::cargo::cargo_build(
            context.package.clone(),
            context.target,
            context.manifest_path.with_file_name("Cargo.toml"),
            filesystem_cache_dir.as_deref(),
            &self.session.options,
            &cargo_opts,
            source_manager,
        )?;

        let provenance = compiled.source_provenance();

        self.compiled.borrow_mut().insert(key, compiled);

        Ok(provenance)
    }

    fn post_process_package(
        &self,
        package: &mut Package,
        context: &miden_assembly::TargetAssemblyContext<'_>,
    ) -> Result<(), Report> {
        let package_id = context.package.name().into_inner();
        let version = context.package.version().into_inner().clone();
        let key = (package_id, version);

        let compiled = self.compiled.borrow();
        let CodegenOutput {
            component,
            account_component_metadata_bytes,
            source_provenance: _,
        } = &compiled[&key];
        post_process_package(
            package,
            component,
            account_component_metadata_bytes.as_deref(),
            context.target,
            context.package_registry,
        )
    }
}

fn selected_executable_target_name<'a>(
    project_package: &'a midenc_session::miden_project::Package,
    session: &'a Session,
) -> Result<&'a str, Report> {
    if let Some(target_name) = session.options.target.as_deref() {
        return Ok(target_name);
    }

    let executable_targets = project_package.executable_targets();
    if executable_targets.len() == 1 {
        return Ok(&**executable_targets[0].name);
    }

    Ok(session.name.as_ref())
}

pub(super) fn assemble_project_with_registry(
    project_package: Arc<midenc_session::miden_project::Package>,
    selector: ProjectTargetSelector,
    session: &Session,
    registry: &mut midenc_session::registry::HybridPackageRegistry,
    source_providers: impl IntoIterator<Item = Box<dyn ProjectSourceProvider>>,
) -> Result<Arc<Package>, Report> {
    let mut assembler = miden_assembly::Assembler::new(session.source_manager.clone())
        .with_warnings_as_errors(session.options.diagnostics.warnings.warnings_as_errors());

    prepare_assembler(&mut assembler, &project_package, session)?;

    let mut project_assembler =
        assembler.for_project_with_providers(project_package, registry, source_providers)?;

    project_assembler.assemble(selector, "dev")
}

pub(super) fn assemble_virtual_project_with_registry(
    project_package: Arc<midenc_session::miden_project::Package>,
    selector: ProjectTargetSelector,
    input: CodegenOutput,
    session: &Session,
    registry: &mut midenc_session::registry::HybridPackageRegistry,
    source_providers: impl IntoIterator<Item = Box<dyn ProjectSourceProvider>>,
) -> Result<Arc<Package>, Report> {
    let target = selector.select_target(&project_package)?;

    let mut assembler = miden_assembly::Assembler::new(session.source_manager.clone())
        .with_warnings_as_errors(session.options.diagnostics.warnings.warnings_as_errors());

    prepare_assembler(&mut assembler, &project_package, session)?;

    let has_required_lib = target.is_executable() && project_package.library_target().is_some();
    assert!(
        !has_required_lib,
        "cannot compile virtual targets that depend on other targets of the same project"
    );

    let mut project_assembler = assembler.for_project_with_providers(
        project_package.clone(),
        registry,
        source_providers,
    )?;

    let package_id = project_package.name().into_inner();
    let sources = input.component.source_inputs(&target, session)?;
    let source_provenance = input.source_provenance;
    let mut cache = alloc::collections::BTreeMap::new();
    let ResolvedPackage { mut package, .. } = project_assembler.assemble_source_package(
        package_id,
        project_package,
        &target,
        "dev",
        None,
        Some(sources),
        Some(source_provenance),
        &mut cache,
    )?;
    // Drop the cache so we know that the `package` is the only reference
    drop(cache);

    {
        let package = Arc::make_mut(&mut package);
        post_process_package(
            package,
            &input.component,
            input.account_component_metadata_bytes.as_deref(),
            &target,
            registry,
        )?;
    }

    Ok(package)
}

pub(super) fn prepare_assembler(
    assembler: &mut miden_assembly::Assembler,
    project_package: &midenc_session::miden_project::Package,
    session: &Session,
) -> Result<(), Report> {
    // Link the compiler intrinsics statically
    assembler.link_package(intrinsics::load(), miden_assembly::Linkage::Static)?;

    // Link extra standalone modules
    let mut link_modules = Vec::default();
    for (path, content) in session.options.link_modules.iter() {
        let source = session.source_manager.load(
            midenc_hir::diagnostics::SourceLanguage::Masm,
            path.as_str().into(),
            content.clone(),
        );
        let module =
            miden_assembly::ModuleParser::new(Some(miden_assembly::ast::ModuleKind::Library))
                .parse(Some(path.as_path()), source, session.source_manager.clone())?;
        link_modules.push(module);
    }
    assembler.compile_and_statically_link_all(link_modules)?;

    // Link libraries which are not direct dependencies of the package
    for link_lib in session.options.link_libraries.iter() {
        if !project_package
            .dependencies()
            .iter()
            .any(|dep| dep.name().as_ref() == link_lib.name.as_ref())
        {
            let package = link_lib.load(&session.options)?;
            assembler.link_package(package, link_lib.linkage)?;
        }
    }

    Ok(())
}

fn post_process_package(
    package: &mut Package,
    component: &MasmComponent,
    account_component_metadata_bytes: Option<&[u8]>,
    target: &midenc_session::miden_project::Target,
    registry: &dyn miden_package_registry::PackageRegistryAndProvider,
) -> Result<(), Report> {
    use miden_assembly::serde::Serializable;
    use miden_mast_package::{Section, SectionId};
    use midenc_session::miden_project::TargetType;

    attach_account_component_metadata(package, account_component_metadata_bytes);
    extend_rodata_advice_map(package, &component.rodata);

    // Embed the kernel in note/transaction script packages, if not already embedded
    if matches!(target.ty, TargetType::Note | TargetType::TransactionScript)
        && !package.sections.iter().any(|section| section.id == SectionId::KERNEL)
        && let Ok(Some(kernel_dep)) = package.kernel_runtime_dependency()
    {
        let version = midenc_session::miden_project::Version::new(
            kernel_dep.version().clone(),
            kernel_dep.digest,
        );
        let kernel_package = registry.load_package(kernel_dep.id(), &version)?;
        package
            .sections
            .push(Section::new(SectionId::KERNEL, kernel_package.to_bytes()));
    }
    //normalize_library_exports(package)?;

    Ok(())
}

/// Attach serialized account component metadata to the assembled package.
fn attach_account_component_metadata(
    package: &mut Package,
    account_component_metadata_bytes: Option<&[u8]>,
) {
    use miden_mast_package::{Section, SectionId};
    if let Some(bytes) = account_component_metadata_bytes {
        package
            .sections
            .push(Section::new(SectionId::ACCOUNT_COMPONENT_METADATA, bytes.to_vec()));
    }
}

/// Rewrite library exports to preserve Wasm component-model interface names.
#[cfg(false)]
fn normalize_library_exports(package: &mut Package) -> Result<(), Report> {
    if !package.kind.is_library() {
        return Ok(());
    }

    let dependencies = package.manifest.dependencies().cloned().collect::<Vec<_>>();
    let exports = recover_wasm_cm_interfaces(package);
    let manifest = miden_mast_package::PackageManifest::new(exports)
        .and_then(|manifest| manifest.with_dependencies(dependencies))
        .map_err(Report::msg)?;
    package.manifest = manifest;
    Ok(())
}

/// Extend the package advice map with the component's rodata segments.
fn extend_rodata_advice_map(package: &mut Package, rodata: &[midenc_codegen_masm::Rodata]) {
    if rodata.is_empty() {
        return;
    }

    let advice_map = rodata.iter().map(|segment| (segment.digest, segment.to_elements())).collect();
    package.extend_advice_map(advice_map);
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
#[cfg(false)]
fn recover_wasm_cm_interfaces(package: &Package) -> Vec<PackageExport> {
    use miden_assembly::{Span, ast as masm};

    let mut exports = Vec::with_capacity(package.manifest.num_exports());
    for export in package.manifest.exports() {
        let Some(proc_export) = export.as_procedure() else {
            exports.push(export.clone());
            continue;
        };

        log::debug!(target: "assemble", "recovering wasm cm interface for export '{}'", &proc_export.path);

        let Some(proc_name) = proc_export.path.last() else {
            exports.push(export.clone());
            continue;
        };

        if proc_name.starts_with("cabi") {
            // Preserve intrinsics modules and internal Wasm CM `cabi_*` functions
            exports.push(export.clone());
            continue;
        }

        if let Some((component, interface)) = proc_name.rsplit_once('/') {
            // Wasm CM interface
            let (interface, function) =
                interface.rsplit_once('#').expect("invalid wasm component model identifier");
            log::debug!(target: "assemble", "recovering wasm cm interface: component is '{component}', interface is '{interface}', function is '{function}'");

            // Derive a new module path in which the Wasm CM interface name is encoded as part of
            // the module path, rather than being encoded in the procedure name.
            let mut module_path = component.to_string();
            module_path.push_str("::");
            module_path.push_str(interface);
            let module_path = masm::Path::new(&module_path);

            let name = masm::ProcedureName::from_raw_parts(masm::Ident::from_raw_parts(
                Span::unknown(Arc::from(function)),
            ));
            let qualified = masm::QualifiedProcedureName::new(module_path, name);
            let qualified = qualified.into_inner();
            log::debug!(target: "assemble", "new export path is '{qualified}'");

            let mut new_export = proc_export.clone();
            new_export.path = qualified;

            exports.push(PackageExport::Procedure(new_export));
        } else {
            // Non-Wasm CM interface, preserve as is
            exports.push(export.clone());
        }
    }
    exports
}
