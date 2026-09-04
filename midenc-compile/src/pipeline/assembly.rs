//! The two points at which both compilation paths touch the assembler.
//!
//! [`prepare_assembler`] runs before assembly, applying the session's link inputs;
//! [`post_process_package`] runs after it, attaching to the assembled package the sections
//! and advice-map entries that codegen produced but the assembler knows nothing about. For note
//! targets, it also builds, validates, and attaches an author codec declared by project metadata.
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

use miden_assembly::TargetAssemblyContext;
use miden_mast_package::Package;
use midenc_codegen_masm::{MasmComponent, intrinsics};
use midenc_session::{Session, diagnostics::Report};

use crate::cargo::NOTE_CODEC_TABLE_NAME;

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

/// Attaches frontend metadata, advice-map data, and target-specific sections after assembly.
pub(crate) fn post_process_package(
    package: &mut Package,
    component: &MasmComponent,
    sections: &midenc_frontend_wasm_metadata::PackageSections,
    context: &TargetAssemblyContext<'_>,
) -> Result<(), Report> {
    use miden_assembly::serde::Serializable;
    use miden_mast_package::{Section, SectionId};
    use midenc_session::miden_project::TargetType;

    let has_note_codec = crate::cargo::has_project_note_codec(context.package.metadata());
    validate_note_codec_declaration(
        has_note_codec,
        package_has_note_target(&context.package),
        context.target.ty,
        sections.note_storage_schema.is_some(),
        context.target.name.inner(),
    )?;

    attach_account_component_metadata(package, sections.account_component_metadata.as_deref())?;
    attach_component_wit(package, sections.component_wit.as_deref())?;
    attach_note_storage_schema(package, sections.note_storage_schema.as_deref())?;
    extend_rodata_advice_map(package, &component.rodata);

    // Embed the kernel in note/transaction script packages, if not already embedded
    if matches!(context.target.ty, TargetType::Note | TargetType::TransactionScript)
        && !package.sections.iter().any(|section| section.id == SectionId::KERNEL)
        && let Ok(Some(kernel_dep)) = package.kernel_runtime_dependency()
    {
        let version = midenc_session::miden_project::Version::new(
            kernel_dep.version().clone(),
            kernel_dep.digest,
        );
        let kernel_package = context.package_registry.load_package(kernel_dep.id(), &version)?;
        package
            .sections
            .push(Section::new(SectionId::KERNEL, kernel_package.to_bytes()));
    }

    if has_note_codec && context.target.ty == TargetType::Note {
        // Run after schema and kernel attachment. The codec stages this package state and hashes it.
        attach_note_codec(package, context)?;
    }

    Ok(())
}

/// Validates the target and schema required by an author codec declaration.
fn validate_note_codec_declaration(
    has_note_codec: bool,
    package_has_note_target: bool,
    target_type: midenc_session::miden_project::TargetType,
    has_note_storage_schema: bool,
    target_name: &str,
) -> Result<(), Report> {
    use midenc_session::miden_project::TargetType;

    if has_note_codec && !package_has_note_target {
        return Err(Report::msg(format!(
            "`[package.metadata.{NOTE_CODEC_TABLE_NAME}]` requires a note target, but the package \
             that contains target '{target_name}' defines no note target"
        )));
    }
    if has_note_codec && target_type == TargetType::Note && !has_note_storage_schema {
        return Err(Report::msg(format!(
            "note target '{target_name}' declares `[package.metadata.{NOTE_CODEC_TABLE_NAME}]` \
             but emitted no note storage schema; add one named-field `#[note]` struct"
        )));
    }
    Ok(())
}

/// Returns true when a package defines at least one note target.
fn package_has_note_target(package: &midenc_session::miden_project::Package) -> bool {
    use midenc_session::miden_project::TargetType;

    package
        .library_target()
        .is_some_and(|target| target.inner().ty == TargetType::Note)
        || package
            .executable_targets()
            .iter()
            .any(|target| target.inner().ty == TargetType::Note)
}

/// Builds and attaches the note codec declared by the current project package.
fn attach_note_codec(
    package: &mut Package,
    context: &TargetAssemblyContext<'_>,
) -> Result<(), Report> {
    let Some(component) = crate::cargo::build_project_note_codec(
        context.package.as_ref(),
        context.manifest_path,
        context.project_root.as_ref(),
        package,
    )?
    else {
        return Ok(());
    };

    let section_id = midenc_frontend_wasm_metadata::package_note_codec_section_id();
    set_unique_section(package, section_id, component, "note codec")
}

/// Attach the note storage schema to the assembled package.
fn attach_note_storage_schema(
    package: &mut Package,
    note_storage_schema: Option<&[u8]>,
) -> Result<(), Report> {
    if let Some(bytes) = note_storage_schema {
        validate_note_storage_schema(bytes)?;
        let section_id = midenc_frontend_wasm_metadata::package_note_storage_schema_section_id();
        set_unique_section(package, section_id, bytes.to_vec(), "note storage schema")?;
    }
    Ok(())
}

/// Validates a note storage schema with the rules every consumer applies on read.
///
/// The check makes the producer build fail with the consumer diagnostic. Without it, a
/// package could publish a schema that every reader rejects, and the author would learn
/// about the defect from a consumer instead of the build.
fn validate_note_storage_schema(bytes: &[u8]) -> Result<(), Report> {
    let text = core::str::from_utf8(bytes)
        .map_err(|error| Report::msg(format!("the note storage schema is not UTF-8: {error}")))?;
    miden_note_schema::NoteStorageSchema::from_wit_text(text).map_err(|error| {
        Report::msg(format!("the note storage schema fails consumer validation: {error}"))
    })?;
    Ok(())
}

/// Adds one package section and rejects an existing section with the same identifier.
fn set_unique_section(
    package: &mut Package,
    section_id: miden_mast_package::SectionId,
    bytes: Vec<u8>,
    description: &str,
) -> Result<(), Report> {
    use miden_mast_package::Section;

    if package.sections.iter().any(|section| section.id == section_id) {
        return Err(Report::msg(format!(
            "cannot attach {description}: package already contains section `{section_id}`"
        )));
    }
    package.sections.push(Section::new(section_id, bytes));
    Ok(())
}

/// Attach serialized account component metadata to the assembled package.
fn attach_account_component_metadata(
    package: &mut Package,
    account_component_metadata_bytes: Option<&[u8]>,
) -> Result<(), Report> {
    use miden_mast_package::SectionId;
    if let Some(bytes) = account_component_metadata_bytes {
        set_unique_section(
            package,
            SectionId::ACCOUNT_COMPONENT_METADATA,
            bytes.to_vec(),
            "account component metadata",
        )?;
    }
    Ok(())
}

/// Attach the component's public WIT source to the assembled package.
fn attach_component_wit(
    package: &mut Package,
    component_wit_bytes: Option<&[u8]>,
) -> Result<(), Report> {
    if let Some(bytes) = component_wit_bytes {
        let id = midenc_frontend_wasm_metadata::package_wit_section_id();
        set_unique_section(package, id, bytes.to_vec(), "component WIT")?;
    }
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

#[cfg(test)]
mod tests {
    use alloc::string::ToString;

    use miden_mast_package::SectionId;
    use midenc_session::miden_project::TargetType;

    use super::*;

    #[test]
    fn attach_rejects_a_schema_that_consumers_cannot_read() {
        let mut package = (*midenc_codegen_masm::intrinsics::load()).clone();

        let error = attach_note_storage_schema(&mut package, Some(b"not wit at all"))
            .unwrap_err()
            .to_string();

        assert!(error.contains("fails consumer validation"), "unexpected error: {error}");
    }

    #[test]
    fn attach_accepts_a_schema_that_consumers_can_read() {
        let mut package = (*midenc_codegen_masm::intrinsics::load()).clone();
        let schema = b"package test:note-schema@1.0.0;

interface note-storage {
    record note {
        value: u64,
    }

    type storage = note;
}
";

        attach_note_storage_schema(&mut package, Some(schema.as_slice())).unwrap();

        let id = midenc_frontend_wasm_metadata::package_note_storage_schema_section_id();
        assert!(package.sections.iter().any(|section| section.id == id));
    }

    #[test]
    fn unique_sections_reject_an_existing_identifier() {
        let mut package = (*midenc_codegen_masm::intrinsics::load()).clone();
        let id = SectionId::custom("test_unique_section").unwrap();

        set_unique_section(&mut package, id.clone(), vec![1], "test section").unwrap();
        let error = set_unique_section(&mut package, id, vec![2], "test section")
            .unwrap_err()
            .to_string();

        assert!(error.contains("already contains section `test_unique_section`"));
    }

    #[test]
    fn codec_metadata_skips_non_note_target_when_package_has_note_target() {
        validate_note_codec_declaration(true, true, TargetType::Library, false, "library").unwrap();
    }

    #[test]
    fn codec_metadata_requires_a_note_target_in_the_package() {
        let error =
            validate_note_codec_declaration(true, false, TargetType::Library, false, "library")
                .unwrap_err()
                .to_string();
        assert!(error.contains("defines no note target"));
    }

    #[test]
    fn codec_metadata_requires_a_schema_on_the_note_target() {
        let missing_schema =
            validate_note_codec_declaration(true, true, TargetType::Note, false, "schema-less")
                .unwrap_err()
                .to_string();
        assert!(missing_schema.contains("emitted no note storage schema"));

        validate_note_codec_declaration(true, true, TargetType::Note, true, "note").unwrap();
        validate_note_codec_declaration(false, false, TargetType::Library, false, "library")
            .unwrap();
    }
}
