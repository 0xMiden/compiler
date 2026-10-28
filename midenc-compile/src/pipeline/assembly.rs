//! The two points at which both compilation paths touch the assembler.
//!
//! [`prepare_assembler`] runs before assembly, applying the session's link inputs;
//! [`post_process_package`] runs after it, attaching to the assembled package the sections
//! and advice-map entries that codegen produced but the assembler knows nothing about. For note
//! and account component targets, it also builds, validates, and attaches an author codec
//! declared by project metadata.
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

use crate::cargo::CODEC_TABLE_NAME;

/// Apply the session's link inputs to `assembler` before a project is assembled with it.
pub(crate) fn prepare_assembler(
    assembler: &mut miden_assembly::Assembler,
    project_package: &midenc_session::miden_project::Package,
    session: &Session,
) -> Result<(), Report> {
    // Link the compiler intrinsics statically
    assembler.link_package(intrinsics::load(session)?, miden_assembly::Linkage::Static)?;

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
            .any(|dep| dep.name().as_ref() == link_lib.canonical_name())
        {
            let package = link_lib.load(&session.options)?;
            assembler.link_package(package, link_lib.linkage)?;
        }
    }

    Ok(())
}

/// Attaches frontend metadata, advice-map data, and target-specific sections after assembly.
///
/// The author codec is attached only when the note or account component is the root target of
/// the build, as [`attaches_codec`] decides. Every other section is attached for every role.
pub(crate) fn post_process_package(
    package: &mut Package,
    component: &MasmComponent,
    sections: &midenc_frontend_wasm_metadata::PackageSections,
    context: &TargetAssemblyContext<'_>,
    role: crate::pipeline::observer::TargetRole,
) -> Result<(), Report> {
    use miden_assembly::serde::Serializable;
    use miden_mast_package::{Section, SectionId};
    use midenc_session::miden_project::TargetType;

    let has_codec = crate::cargo::has_project_codec(context.package.metadata());
    validate_codec_declaration(
        has_codec,
        package_has_codec_target(&context.package),
        context.target.ty,
        sections.note_storage_schema.is_some(),
        sections.account_storage_schema.is_some(),
        context.target.name.inner(),
    )?;

    attach_account_component_metadata(package, sections.account_component_metadata.as_deref())?;
    attach_component_wit(package, sections.component_wit.as_deref())?;
    attach_note_storage_schema(package, sections.note_storage_schema.as_deref())?;
    attach_account_storage_schema(package, sections.account_storage_schema.as_deref())?;
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

    if attaches_codec(has_codec, context.target.ty, role) {
        // Run after schema and kernel attachment. The codec stages this package state and hashes it.
        attach_codec(package, context)?;
    }

    Ok(())
}

/// Returns true when the assembled package must carry an author codec section.
///
/// The codec is built only for a note or an account component that is the root target of the
/// build. A target that is a source dependency of another project gets no codec section, for two
/// reasons. The codec crate's inputs are outside the build provenance, so a dependency copy that
/// is kept in a package store can carry a stale codec after the codec crate changes. And every
/// dependency assembly would otherwise run a nested cargo build. A consumer that needs the codec
/// loads the target's own package. This is also the behavior of the package post-processor that
/// will take over the codec build, because a post-processor never runs on a dependency.
fn attaches_codec(
    has_codec: bool,
    target_type: midenc_session::miden_project::TargetType,
    role: crate::pipeline::observer::TargetRole,
) -> bool {
    has_codec && is_codec_target(target_type) && role.is_root()
}

/// Returns true for the target types whose packages carry a schema an author codec serves.
fn is_codec_target(target_type: midenc_session::miden_project::TargetType) -> bool {
    use midenc_session::miden_project::TargetType;

    matches!(target_type, TargetType::Note | TargetType::AccountComponent)
}

/// Validates the target and schema required by an author codec declaration.
fn validate_codec_declaration(
    has_codec: bool,
    package_has_codec_target: bool,
    target_type: midenc_session::miden_project::TargetType,
    has_note_storage_schema: bool,
    has_account_storage_schema: bool,
    target_name: &str,
) -> Result<(), Report> {
    use midenc_session::miden_project::TargetType;

    if !has_codec {
        return Ok(());
    }
    if !package_has_codec_target {
        return Err(Report::msg(format!(
            "`[package.metadata.{CODEC_TABLE_NAME}]` requires a note or account component target, \
             but the package that contains target '{target_name}' defines neither"
        )));
    }
    if target_type == TargetType::Note && !has_note_storage_schema {
        return Err(Report::msg(format!(
            "note target '{target_name}' declares `[package.metadata.{CODEC_TABLE_NAME}]` but \
             emitted no note storage schema; add one named-field `#[note]` struct"
        )));
    }
    if target_type == TargetType::AccountComponent && !has_account_storage_schema {
        return Err(Report::msg(format!(
            "account component target '{target_name}' declares \
             `[package.metadata.{CODEC_TABLE_NAME}]` but emitted no account storage schema; add \
             at least one `#[storage]` slot"
        )));
    }
    Ok(())
}

/// Returns true when a package defines at least one note or account component target.
fn package_has_codec_target(package: &midenc_session::miden_project::Package) -> bool {
    package
        .library_target()
        .is_some_and(|target| is_codec_target(target.inner().ty))
        || package
            .executable_targets()
            .iter()
            .any(|target| is_codec_target(target.inner().ty))
}

/// Builds and attaches the codec declared by the current project package.
fn attach_codec(package: &mut Package, context: &TargetAssemblyContext<'_>) -> Result<(), Report> {
    let Some(component) = crate::cargo::build_project_codec(
        context.package.as_ref(),
        context.manifest_path,
        context.project_root.as_ref(),
        package,
    )?
    else {
        return Ok(());
    };

    let section_id = midenc_frontend_wasm_metadata::package_codec_section_id();
    set_unique_section(package, section_id, component, "codec")
}

/// Attach the note storage schema to the assembled package.
fn attach_note_storage_schema(
    package: &mut Package,
    note_storage_schema: Option<&[u8]>,
) -> Result<(), Report> {
    attach_schema(
        package,
        note_storage_schema,
        midenc_frontend_wasm_metadata::package_note_storage_schema_section_id(),
        "note storage",
        |text| miden_note_schema::NoteStorageSchema::from_wit_text(text).map(drop),
    )
}

/// Attach the account storage schema to the assembled package.
fn attach_account_storage_schema(
    package: &mut Package,
    account_storage_schema: Option<&[u8]>,
) -> Result<(), Report> {
    attach_schema(
        package,
        account_storage_schema,
        midenc_frontend_wasm_metadata::package_account_storage_schema_section_id(),
        "account storage",
        |text| miden_account_storage_schema::AccountStorageSchema::from_wit_text(text).map(drop),
    )
}

/// Validates a schema with `read`, the reader every consumer applies, and attaches it.
///
/// The check makes the producer build fail with the consumer diagnostic. Without it, a
/// package could publish a schema that every reader rejects, and the author would learn
/// about the defect from a consumer instead of the build.
fn attach_schema(
    package: &mut Package,
    schema: Option<&[u8]>,
    section_id: miden_mast_package::SectionId,
    kind: &str,
    read: impl FnOnce(&str) -> miden_schema::Result<()>,
) -> Result<(), Report> {
    let Some(bytes) = schema else {
        return Ok(());
    };
    let text = core::str::from_utf8(bytes)
        .map_err(|error| Report::msg(format!("the {kind} schema is not UTF-8: {error}")))?;
    read(text).map_err(|error| {
        Report::msg(format!("the {kind} schema fails consumer validation: {error}"))
    })?;
    set_unique_section(package, section_id, bytes.to_vec(), &format!("{kind} schema"))
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
    use alloc::{string::ToString, sync::Arc, vec};

    use miden_assembly_syntax::debuginfo::Span;
    use miden_mast_package::SectionId;
    use midenc_session::{
        InputFile, LinkLibrary, Options,
        miden_project::{
            Dependency, DependencyVersionScheme, Linkage, Package as ProjectPackage, Target,
            TargetType, VersionReq, VersionRequirement,
        },
    };

    use super::*;
    use crate::pipeline::testing::{fixture_source, options_linked_to_the_toolchain};

    /// A link library whose path names a file that is not a package, so `prepare_assembler`
    /// reaching for it fails with a message naming that path — which is how these tests observe
    /// whether the library was linked at all.
    fn unloadable(name: &'static str, dir: &str) -> LinkLibrary {
        let path = fixture_source(dir, "not-a-package.masp", "this is not a Miden package");
        LinkLibrary {
            name: name.into(),
            path: Some(path),
            linkage: Linkage::Dynamic,
        }
    }

    /// A package with one library target, depending on `dependencies` by name.
    fn project_package(dependencies: &[&str]) -> Arc<ProjectPackage> {
        let target = Target::library(
            Arc::from(
                midenc_session::miden_assembly_syntax::PathBuf::new("::probe")
                    .unwrap()
                    .into_boxed_path(),
            ),
            midenc_session::diagnostics::Uri::new("lib.wasm"),
        );
        assert_eq!(target.ty, TargetType::Library);
        Arc::from(ProjectPackage::new("probe", target).with_dependencies(dependencies.iter().map(
            |name| {
                Dependency::new(
                    Span::unknown((*name).to_string().into()),
                    DependencyVersionScheme::Registry(VersionRequirement::Semantic(Span::unknown(
                        VersionReq::STAR.clone(),
                    ))),
                    Linkage::Dynamic,
                )
            },
        )))
    }

    fn session_linking(libraries: Vec<LinkLibrary>) -> Session {
        let options = Options {
            link_libraries: libraries,
            ..options_linked_to_the_toolchain()
        };
        Session::new(
            InputFile::empty(),
            alloc::boxed::Box::new(options),
            None,
            Arc::new(midenc_session::diagnostics::DefaultSourceManager::default()),
        )
        .expect("should build a session")
    }

    fn prepare(package: &ProjectPackage, session: &Session) -> Result<(), Report> {
        let mut assembler = miden_assembly::Assembler::new(session.source_manager.clone());
        prepare_assembler(&mut assembler, package, session)
    }

    /// `-l std` and a `miden-core` dependency name the same library, so core must not be linked a
    /// second time. The comparison is over canonical names, which is what makes the two match;
    /// comparing the alias as written would not, and core would be loaded and linked twice.
    #[test]
    fn a_link_library_that_aliases_a_dependency_is_not_linked_again() {
        let session = session_linking(vec![unloadable("std", "assembly-alias-dependency")]);
        prepare(&project_package(&["miden-core"]), &session)
            .expect("core is already a dependency, so `-l std` must not be loaded again");
    }

    /// The converse, so the test above cannot pass by the loop doing nothing: a link library that
    /// is not among the package's dependencies *is* loaded and linked.
    #[test]
    fn a_link_library_that_is_not_a_dependency_is_linked() {
        let session = session_linking(vec![unloadable("std", "assembly-non-dependency")]);
        let err = prepare(&project_package(&["miden-protocol"]), &session)
            .expect_err("nothing depends on core, so `-l std` must be loaded")
            .to_string();
        assert!(err.contains("not-a-package.masp"), "{err}");
    }

    /// A package to attach sections to.
    fn some_package() -> miden_mast_package::Package {
        (*crate::pipeline::testing::some_package()).clone()
    }

    #[test]
    fn attach_rejects_a_schema_that_consumers_cannot_read() {
        let mut package = some_package();

        let error = attach_note_storage_schema(&mut package, Some(b"not wit at all"))
            .unwrap_err()
            .to_string();

        assert!(error.contains("fails consumer validation"), "unexpected error: {error}");
    }

    #[test]
    fn attach_accepts_a_schema_that_consumers_can_read() {
        let mut package = some_package();
        let schema = b"package test:note-schema;

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
        let mut package = some_package();
        let id = SectionId::custom("test_unique_section").unwrap();

        set_unique_section(&mut package, id.clone(), vec![1], "test section").unwrap();
        let error = set_unique_section(&mut package, id, vec![2], "test section")
            .unwrap_err()
            .to_string();

        assert!(error.contains("already contains section `test_unique_section`"));
    }

    /// An account storage schema with one value slot.
    const ACCOUNT_STORAGE_SCHEMA: &[u8] = b"package test:account-storage-schema;

interface account-storage {
    record storage {
        count: u32,
    }
}
";

    #[test]
    fn attach_accepts_an_account_storage_schema_that_consumers_can_read() {
        let mut package = some_package();

        attach_account_storage_schema(&mut package, Some(ACCOUNT_STORAGE_SCHEMA)).unwrap();

        let id = midenc_frontend_wasm_metadata::package_account_storage_schema_section_id();
        let section = package.sections.iter().find(|section| section.id == id).unwrap();
        assert_eq!(section.data.as_ref(), ACCOUNT_STORAGE_SCHEMA);
    }

    #[test]
    fn attach_rejects_an_account_storage_schema_that_consumers_cannot_read() {
        let mut package = some_package();
        let too_wide = b"package test:account-storage-schema;

interface account-storage {
    record wide {
        a: u64,
        b: u64,
        c: u8,
    }

    record storage {
        wide: wide,
    }
}
";

        for schema in [b"not wit at all".as_slice(), too_wide.as_slice()] {
            let error = attach_account_storage_schema(&mut package, Some(schema))
                .unwrap_err()
                .to_string();
            assert!(
                error.contains("the account storage schema fails consumer validation"),
                "unexpected error: {error}"
            );
        }
        let id = midenc_frontend_wasm_metadata::package_account_storage_schema_section_id();
        assert!(!package.sections.iter().any(|section| section.id == id));
    }

    #[test]
    fn attach_rejects_a_second_account_storage_schema() {
        let mut package = some_package();

        attach_account_storage_schema(&mut package, Some(ACCOUNT_STORAGE_SCHEMA)).unwrap();
        let error = attach_account_storage_schema(&mut package, Some(ACCOUNT_STORAGE_SCHEMA))
            .unwrap_err()
            .to_string();

        assert!(
            error.contains("cannot attach account storage schema: package already contains"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn codec_metadata_skips_a_target_without_a_schema_kind_when_the_package_has_one() {
        validate_codec_declaration(true, true, TargetType::Library, false, false, "library")
            .unwrap();
    }

    #[test]
    fn codec_metadata_requires_a_note_or_component_target_in_the_package() {
        let error =
            validate_codec_declaration(true, false, TargetType::Library, false, false, "library")
                .unwrap_err()
                .to_string();
        assert!(error.contains("requires a note or account component target"), "{error}");
    }

    #[test]
    fn codec_metadata_requires_the_matching_schema_on_the_target() {
        let missing_schema =
            validate_codec_declaration(true, true, TargetType::Note, false, true, "schema-less")
                .unwrap_err()
                .to_string();
        assert!(missing_schema.contains("emitted no note storage schema"));
        validate_codec_declaration(true, true, TargetType::Note, true, false, "note").unwrap();

        let missing_schema = validate_codec_declaration(
            true,
            true,
            TargetType::AccountComponent,
            true,
            false,
            "slot-less",
        )
        .unwrap_err()
        .to_string();
        assert!(missing_schema.contains("emitted no account storage schema"));
        assert!(missing_schema.contains("add at least one `#[storage]` slot"));
        validate_codec_declaration(
            true,
            true,
            TargetType::AccountComponent,
            false,
            true,
            "component",
        )
        .unwrap();

        validate_codec_declaration(false, false, TargetType::Library, false, false, "library")
            .unwrap();
        validate_codec_declaration(
            false,
            false,
            TargetType::AccountComponent,
            false,
            false,
            "component",
        )
        .unwrap();
    }

    #[test]
    fn codec_attaches_only_to_a_root_note_or_component_that_declares_one() {
        use crate::pipeline::observer::TargetRole;

        for target_type in [TargetType::Note, TargetType::AccountComponent] {
            assert!(attaches_codec(true, target_type, TargetRole::Root));
            assert!(!attaches_codec(true, target_type, TargetRole::Dependency));
            assert!(!attaches_codec(true, target_type, TargetRole::RequiredLibrary));
            assert!(!attaches_codec(false, target_type, TargetRole::Root));
        }
        for target_type in [
            TargetType::Library,
            TargetType::Executable,
            TargetType::Kernel,
            TargetType::TransactionScript,
        ] {
            assert!(!attaches_codec(true, target_type, TargetRole::Root));
        }
    }
}
