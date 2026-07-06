//! Integration test for `note::get_entrypoint_root()`.
//!
//! Executes a note constructor that returns `get_entrypoint_root()` verbatim and asserts the
//! returned digest equals the note script root selected from the compiled package by
//! `NoteScript::from_package` — i.e. the root the transaction kernel executes. This pins the
//! whole intrinsic pipeline (linker stub → `hir.procedure_root` → retarget at export lifting →
//! MASM `procref`) at the exact layer it can break, independently of the heavier mock-chain
//! note-creation flow.

use miden_core::program::Program;
use miden_mast_package::{Package, PackageExport};
use miden_protocol::note::NoteScript;
use midenc_frontend_wasm::WasmTranslationConfig;
use midenc_integration_test_support::{
    CompilerTestBuilder, Project, cargo_proj::project, compiler_test::sdk_crate_path,
    testing::executor_with_std,
};

/// Builds a minimal note project whose `probe` constructor returns `get_entrypoint_root()`.
fn build_probe_note_project() -> Project {
    let sdk_path = sdk_crate_path();
    let cargo_toml = format!(
        r#"cargo-features = ["trim-paths"]

[package]
name = "note_script_root_probe"
version = "0.1.0"
edition = "2024"

[lib]
crate-type = ["cdylib"]

[dependencies]
miden = {{ path = "{sdk_path}" }}

[package.metadata.component]
package = "miden:note-script-root-probe"

[package.metadata.miden]
project-kind = "note-script"

[profile.release]
trim-paths = ["diagnostics", "object"]

[profile.dev]
trim-paths = ["diagnostics", "object"]
"#,
        sdk_path = sdk_path.display(),
    );
    let miden_project_toml = r#"[package]
name = "note_script_root_probe"
version = "0.1.0"

[lib]
kind = "note"
namespace = "miden:note-script-root-probe/miden-note-script-root-probe@0.1.0"

[dependencies]
miden-core = "*"
miden-protocol = "*"
"#;
    let source = r#"#![no_std]
#![feature(alloc_error_handler)]

use miden::*;

#[note]
struct ProbeNote;

#[note]
impl ProbeNote {
    /// Returns the note script root of this crate, as the compiler resolved it.
    #[note_constructor]
    pub fn probe() -> Word {
        note::get_entrypoint_root()
    }

    #[note_script]
    pub fn run(self, _arg: Word) {}
}
"#;

    project("note_script_root_probe")
        .file("Cargo.toml", &cargo_toml)
        .file("miden-project.toml", miden_project_toml)
        .file("src/lib.rs", source)
        .build()
}

/// Rebuilds an executable program from the lifted component export with the given leaf name.
///
/// A package manifest exposes two exports per component function under the same leaf name: the
/// core-Wasm function (under its `namespace::interface` path) and the compiler-lifted component
/// export (under the component-id root module). They have different digests and calling
/// conventions; execution must target the lifted export, recognizable by its module segment
/// holding the full `ns:pkg/interface@version` component id (the only segment containing `/`).
fn export_program(package: &Package, name: &str) -> Program {
    let matches: Vec<_> = package
        .manifest
        .exports()
        .filter_map(|export| match export {
            PackageExport::Procedure(procedure) if export.name() == name => Some(procedure),
            _ => None,
        })
        .collect();
    let lifted: Vec<_> = matches
        .iter()
        .filter(|procedure| procedure.path.to_string().contains('/'))
        .collect();
    let [procedure] = lifted.as_slice() else {
        panic!(
            "expected exactly one lifted component export named '{name}', got {} among {:?}",
            lifted.len(),
            matches.iter().map(|procedure| procedure.path.to_string()).collect::<Vec<_>>()
        );
    };
    let entrypoint = package
        .mast
        .mast_forest()
        .find_procedure_root(procedure.digest)
        .unwrap_or_else(|| panic!("export '{name}' should have a MAST node"));
    Program::new(package.mast.mast_forest().clone(), entrypoint)
}

/// Asserts that `get_entrypoint_root()` returns the MAST root of the `@note_script` export.
#[test]
fn get_entrypoint_root_returns_the_note_script_root() {
    let probe_project = build_probe_note_project();
    let mut test = CompilerTestBuilder::rust_source_cargo_miden(
        probe_project.root(),
        WasmTranslationConfig::default(),
        [],
    )
    .build();
    let package = test.compile_package();
    assert!(package.is_library(), "expected a note library package");

    // The root the transaction kernel executes: the `@note_script`-attributed export.
    let expected_root: miden_core::Word = NoteScript::from_package(&package)
        .expect("compiled package should contain exactly one note script export")
        .root()
        .into();

    // Execute the `probe` constructor, which returns `get_entrypoint_root()` verbatim.
    let program = export_program(&package, "probe");
    let exec = executor_with_std(vec![], None);
    let trace = exec.execute(&program, test.session.source_manager.clone());

    let actual_root = trace
        .outputs()
        .get_word(0)
        .expect("probe should leave its Word result on top of the stack");
    assert_eq!(
        actual_root, expected_root,
        "get_entrypoint_root() must return the MAST root of the note script export"
    );
}
