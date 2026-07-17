//! Shared fixtures for base-macros unit tests.

use std::{fs, path::Path, sync::Arc};

use miden_assembly::{Assembler, DefaultSourceManager, Parse, ParseOptions, ast::ModuleKind};
use miden_mast_package::Package;
use miden_protocol::utils::serde::Serializable;

/// Builds a minimal package fixture with the given package id, optionally embedding `wit` in the
/// WIT section. The fixture version is `0.1.0`.
pub(crate) fn build_package(package_id: &str, wit: Option<&str>) -> Arc<Package> {
    let source_manager = Arc::new(DefaultSourceManager::default());
    let module = "pub proc callee(a: felt) -> felt\n    add.1\nend"
        .parse_with_options(source_manager.clone(), ParseOptions::new(ModuleKind::Library, "::dep"))
        .expect("fixture module must parse");
    let library = Assembler::new(source_manager)
        .assemble_library([module])
        .expect("fixture library must assemble");
    let mut package = Package::from_library(
        miden_mast_package::PackageId::from(package_id),
        "0.1.0".parse().expect("fixture version must parse"),
        miden_mast_package::TargetType::Library,
        library,
        core::iter::empty(),
    );
    if let Some(wit) = wit {
        package.sections.push(miden_mast_package::Section::new(
            crate::dependency_package::wit_section_id(),
            wit.as_bytes().to_vec(),
        ));
    }
    Arc::from(package)
}

/// Writes a minimal `.masp` package fixture with the given package id, optionally embedding
/// `wit` in the WIT section.
pub(crate) fn write_masp_fixture(package_path: &Path, package_id: &str, wit: Option<&str>) {
    let package = build_package(package_id, wit);
    fs::create_dir_all(package_path.parent().expect("package path must have a parent"))
        .expect("package directory must be created");
    fs::write(package_path, package.to_bytes()).expect("package fixture must be written");
}
