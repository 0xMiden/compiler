//! Project-assembler support for compiler-generated MASM components.

use alloc::{collections::BTreeMap, string::ToString, sync::Arc, vec::Vec};

use miden_assembly::{
    Assembler, Library, Path, ProjectSourceInputs, ProjectTargetSelector,
    library::{LibraryExport, ProcedureExport},
};
use miden_mast_package::{PackageManifest, Section, SectionId};
use midenc_session::{
    Session,
    diagnostics::{Report, Span},
};

use super::{MasmComponent, Package, Rodata};
use crate::{intrinsics::INTRINSICS_MODULE_NAMES, masm};

/// Assemble a MASM component through the VM project assembler.
pub(super) fn assemble(
    component: &MasmComponent,
    account_component_metadata_bytes: Option<&[u8]>,
    session: &Session,
) -> Result<Arc<Package>, Report> {
    let mut assembler = Assembler::new(session.source_manager.clone())
        .with_warnings_as_errors(session.options.diagnostics.warnings.warnings_as_errors());
    let mut link_modules = Vec::default();
    for (path, content) in session.options.link_modules.iter() {
        let source = session.source_manager.load(
            midenc_hir::diagnostics::SourceLanguage::Masm,
            path.as_str().into(),
            content.clone(),
        );
        let module =
            miden_assembly_syntax::ModuleParser::new(miden_assembly::ast::ModuleKind::Library)
                .parse(path, source, session.source_manager.clone())?;
        link_modules.push(module);
    }
    assembler.compile_and_statically_link_all(link_modules)?;
    let sources =
        prepare_sources(component, &mut assembler, session.get_flag("test_harness"), session)?;
    let mut registry = session.package_registry()?;
    let project_package = session.project.package();
    let is_executable_target = session.options.target_type.is_some_and(|tt| tt.is_executable())
        || project_package.library_target().is_none()
        || session.options.target.as_deref().is_some_and(|tname| {
            project_package.executable_targets().iter().any(|t| tname == &**t.name)
        });
    std::dbg!(is_executable_target);
    let mut project_assembler = assembler.for_project(project_package, registry.as_mut())?;

    let executable_name = session.name.as_ref();
    let selector = if std::dbg!(component.entrypoint.as_ref()).is_some() && is_executable_target {
        std::dbg!(&executable_name);
        ProjectTargetSelector::Executable(executable_name)
    } else {
        ProjectTargetSelector::Library
    };
    let mut package = project_assembler.assemble_with_sources(selector, "dev", sources)?;
    {
        let package = Arc::make_mut(&mut package);

        attach_account_component_metadata(package, account_component_metadata_bytes);
        extend_rodata_advice_map(package, &component.rodata);
        normalize_library_exports(package)?;
    }
    Ok(package)
}

/// Prepare the synthetic project target and source inputs used to assemble compiler-generated MASM.
fn prepare_sources(
    component: &MasmComponent,
    assembler: &mut Assembler,
    emit_test_harness: bool,
    session: &Session,
) -> Result<ProjectSourceInputs, Report> {
    // Intrinsics must be linked into the assembler context directly so they do not become part of
    // the assembled package surface.
    let mut support = Vec::with_capacity(component.modules.len());
    let mut root = None;
    for module in component.modules.iter() {
        if is_intrinsics_module(module) {
            log::debug!(
                target: "assembly",
                "adding intrinsics '{}' to assembler",
                module.path()
            );
            assembler.compile_and_statically_link(module.clone())?;
            continue;
        }

        if module.path() == component.root.as_ref() {
            root = Some(Box::new(Arc::unwrap_or_clone(module.clone())));
            continue;
        }

        support.push(Box::new(Arc::unwrap_or_clone(module.clone())));
    }

    if let Some(entrypoint) = component.entrypoint.as_ref() {
        // Our generated main module takes precedence here, so move the root module into support
        support.extend(root);
        let root = component.generate_main(
            entrypoint,
            emit_test_harness,
            session.source_manager.clone(),
        )?;
        return Ok(ProjectSourceInputs { root, support });
    }

    let root = root.expect("components must always have a root module");
    Ok(ProjectSourceInputs { root, support })
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

    let dependencies = package.manifest.dependencies().cloned().collect::<Vec<_>>();
    let exports = recover_wasm_cm_interfaces(package.mast.as_ref());
    package.mast = Arc::new(Library::new(package.mast.mast_forest().clone(), exports)?);
    package.manifest = PackageManifest::from_library(package.mast.as_ref())
        .with_dependencies(dependencies)
        .map_err(|error| Report::msg(error.to_string()))?;
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

/// Return true when the module belongs to the compiler's intrinsics namespace.
fn is_intrinsics_module(module: &miden_assembly::ast::Module) -> bool {
    module.path().as_str().trim_start_matches("::").starts_with("intrinsics")
}
