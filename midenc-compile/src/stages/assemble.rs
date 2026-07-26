use alloc::{boxed::Box, sync::Arc, vec::Vec};
use core::ops::ControlFlow;

use miden_assembly::{
    ProjectSourceProvider, ProjectTargetSelector, ResolvedPackage, utils::DisplayHex,
};
use miden_mast_package::{Package, TargetType};
use midenc_codegen_masm::{MasmComponent, intrinsics};

use super::*;
use crate::pipeline::{
    CheckpointId, Frontend, FrontendProvider, Goal, RequestState, RootTarget, TargetKey,
    frontends::RustProjectFrontend,
};

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
        let mut registry = session.package_registry()?;
        let package = if project_package.manifest_path().is_some() {
            // This run has *already* produced `input` for the target `selector` names, so hand
            // it to the provider rather than letting it spawn a nested cargo build to
            // reproduce work we just did. The seed key must name the resolved target, not
            // just the package: a package-derived key inserts fine and simply never matches.
            let target = selector.select_target(&project_package)?;
            let key = TargetKey::new(
                project_package.name().into_inner(),
                target.name.inner().clone(),
                target.ty,
            );
            let root = RootTarget::new(project_package.clone(), &target);
            assemble_project_with_registry(
                project_package.clone(),
                selector,
                &session,
                &mut registry,
                [rust_source_provider(
                    session.clone(),
                    root,
                    RustProjectFrontend::seeded(session.clone(), key, input),
                )],
            )?
        } else {
            let root = RootTarget::new(
                project_package.clone(),
                &selector.select_target(&project_package)?,
            );
            assemble_virtual_project_with_registry(
                project_package.clone(),
                selector,
                input,
                &session,
                &mut registry,
                [rust_source_provider(
                    session.clone(),
                    root,
                    RustProjectFrontend::new(session.clone()),
                )],
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
                let root = RootTarget::new(package.clone(), &selector.select_target(&package)?);
                assemble_project_with_registry(
                    package.clone(),
                    selector,
                    session,
                    &mut registry,
                    [rust_source_provider(
                        context.session_rc(),
                        root,
                        RustProjectFrontend::new(context.session_rc()),
                    )],
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

/// The single `"rs"` source provider the standalone path registers with the assembler.
///
/// These `Stage` chains always drive a whole build through package assembly, so the request
/// state's goal is [`CheckpointId::PACKAGE_ASSEMBLED`], it carries no observers, and its
/// capture slot is a throwaway: [`RustProjectFrontend`] publishes no checkpoints, so nothing
/// can stop and nothing is ever captured.
///
/// `root` is the target the surrounding selector resolved, and is what lets the provider
/// derive each callback's role. Which role a callback gets makes no difference to this
/// frontend — it neither publishes nor reads the role — but deriving it is free and leaves
/// nothing hardcoded to go stale.
///
/// No `"masm"` provider is registered alongside it, deliberately. Claiming that extension
/// displaces the assembler's own [`MasmSourceProvider`](miden_assembly::MasmSourceProvider)
/// for every Miden Assembly target in the graph; the project path does exactly that on
/// purpose, and the standalone path has no reason to.
fn rust_source_provider(
    session: Rc<Session>,
    root: RootTarget,
    frontend: RustProjectFrontend,
) -> Box<dyn ProjectSourceProvider> {
    Box::new(FrontendProvider::new(
        "rs",
        Rc::new(frontend) as Rc<dyn Frontend>,
        session,
        Rc::new(RequestState::new(Goal::at(CheckpointId::PACKAGE_ASSEMBLED), Vec::new())),
        root,
    ))
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

    // The build profile is hardcoded here, where `Pipeline::compile` passes the name from
    // `Options::profile`. That asymmetry is the remaining half of the profile rule: project
    // inputs (`miden-project.toml`/`Cargo.toml`) go through the pipeline and honor `--release`,
    // while standalone inputs still reach this legacy chain and always build `dev`. It
    // converges when standalone inputs move onto the pipeline, with the standalone frontends.
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
    let mut package = match project_assembler.assemble_source_package(
        package_id,
        project_package,
        &target,
        // Hardcoded for the same reason as in `assemble_project_with_registry` above: this is
        // the standalone-input arm, which does not yet honor `Options::profile`.
        "dev",
        miden_assembly::InterruptedTargetRole::Root,
        None,
        Some(sources),
        Some(source_provenance),
        &mut cache,
    )? {
        ControlFlow::Continue(ResolvedPackage { package, .. }) => package,
        ControlFlow::Break(interrupted) => {
            return Err(Report::msg(format!(
                "unexpected interruption assembling '{}'",
                interrupted.package
            )));
        }
    };
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

/// Apply the session's link inputs to `assembler` before a project is assembled with it.
///
/// `pub(crate)` rather than `pub(super)` because [`crate::pipeline`] assembles through this
/// same sequence: the pipeline driver builds its own [`miden_assembly::Assembler`] and must
/// prepare it exactly as the legacy stages do, or the two paths link different inputs.
pub(crate) fn prepare_assembler(
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

pub(crate) fn post_process_package(
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
