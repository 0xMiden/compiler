//! The two points at which both compilation paths touch the assembler.
//!
//! [`prepare_assembler`] runs before assembly, applying the session's link inputs;
//! [`post_process_package`] runs after it, attaching to the assembled package the sections
//! and advice-map entries that codegen produced but the assembler knows nothing about.
//!
//! Both are shared: the [`Pipeline`](super::Pipeline) driver prepares its own assembler
//! through the first, and every frontend that lowers HIR post-processes through the second —
//! [`RustProjectFrontend`](super::frontends::rust::RustProjectFrontend),
//! [`WasmFrontend`](super::frontends::wasm::WasmFrontend) and
//! [`HirFrontend`](super::frontends::hir::HirFrontend) alike. Keeping one copy is what makes
//! every route link the same inputs and emit the same sections — with one caveat: a `.hir`
//! input carries no Wasm custom sections to extract, so recompiling an emitted HIR artifact
//! produces a package without the account-metadata and WIT sections its Wasm-derived
//! counterpart has.

use alloc::vec::Vec;

use miden_mast_package::Package;
use midenc_codegen_masm::{MasmComponent, intrinsics};
use midenc_session::{Session, diagnostics::Report};

/// Apply the session's link inputs to `assembler` before a project is assembled with it.
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
    sections: &midenc_frontend_wasm_metadata::PackageSections,
    target: &midenc_session::miden_project::Target,
    registry: &dyn miden_package_registry::PackageRegistryAndProvider,
) -> Result<(), Report> {
    use miden_assembly::serde::Serializable;
    use miden_mast_package::{Section, SectionId};
    use midenc_session::miden_project::TargetType;

    attach_account_component_metadata(package, sections.account_component_metadata.as_deref());
    attach_component_wit(package, sections.component_wit.as_deref());
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

/// Attach the component's public WIT source to the assembled package.
fn attach_component_wit(package: &mut Package, component_wit_bytes: Option<&[u8]>) {
    use miden_mast_package::Section;
    if let Some(bytes) = component_wit_bytes {
        let id = midenc_frontend_wasm_metadata::package_wit_section_id();
        package.sections.push(Section::new(id, bytes.to_vec()));
    }
}

/// Extend the package advice map with the component's rodata segments.
fn extend_rodata_advice_map(package: &mut Package, rodata: &[midenc_codegen_masm::Rodata]) {
    if rodata.is_empty() {
        return;
    }

    let advice_map = rodata.iter().map(|segment| (segment.digest, segment.to_elements())).collect();
    package.extend_advice_map(advice_map);
}
