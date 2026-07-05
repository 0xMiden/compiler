//! Shared fixtures for base-macros unit tests.

use std::{fs, path::Path, sync::Arc};

use miden_assembly::{Assembler, DefaultSourceManager, ModuleParser, ast::ModuleKind};
use miden_protocol::utils::serde::Serializable;

/// Writes a minimal `.masp` package fixture with the given package id, optionally embedding
/// `wit` in the WIT section. The fixture version is `0.1.0`.
pub(crate) fn write_masp_fixture(package_path: &Path, package_id: &str, wit: Option<&str>) {
    let source_manager = Arc::new(DefaultSourceManager::default());
    let module = ModuleParser::new(Some(ModuleKind::Library))
        .parse_str(
            Some(miden_assembly::Path::new("dep")),
            "pub proc callee(a: felt) -> felt\n    add.1\nend",
            source_manager.clone(),
        )
        .expect("fixture module must parse");
    let mut package = Assembler::new(source_manager)
        .assemble_library(package_id, module, None::<Box<miden_assembly::ast::Module>>)
        .expect("fixture library must assemble");
    package.version = "0.1.0".parse().expect("fixture version must parse");
    if let Some(wit) = wit {
        package.sections.push(miden_mast_package::Section::new(
            crate::dependency_package::wit_section_id(),
            wit.as_bytes().to_vec(),
        ));
    }

    fs::create_dir_all(package_path.parent().expect("package path must have a parent"))
        .expect("package directory must be created");
    fs::write(package_path, package.to_bytes()).expect("package fixture must be written");
}
