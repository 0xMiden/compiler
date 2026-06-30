//! Integration tests for component-model CanonABI values.

use std::{fs, path::Path};

use miden_core::program::Program;
use miden_mast_package::Package;
use miden_protocol::note::NoteScript;
use midenc_frontend_wasm::WasmTranslationConfig;
use midenc_integration_test_support::{
    CompilerTest, CompilerTestBuilder, Project, cargo_proj::project, compiler_test::sdk_crate_path,
    testing::executor_with_std,
};

mod option;
mod result;
mod variant;

/// Names and package identifiers used by one generated account/note pair.
struct CanonAbiProjectNames {
    /// The Rust crate name of the account project.
    account_crate: String,
    /// The component package name of the account project without the `miden:` namespace.
    account_slug: String,
    /// The Rust module generated for the account package in note bindings.
    account_package_module: String,
    /// The Rust module generated for the shared account interface in note bindings.
    account_interface_module: String,
    /// The Rust crate name of the note project.
    note_crate: String,
    /// The component package name of the note project without the `miden:` namespace.
    note_slug: String,
}

impl CanonAbiProjectNames {
    /// Constructs generated project names for `case`.
    fn new(case: &str) -> Self {
        let case = case.replace('-', "_");
        let account_crate = format!("canonabi_{case}_account");
        let account_slug = account_crate.replace('_', "-");
        let account_package_module = account_slug.replace('-', "_");
        let account_interface_module = "canonabi_component".to_string();
        let note_crate = format!("canonabi_{case}_note");
        let note_slug = note_crate.replace('_', "-");

        Self {
            account_crate,
            account_slug,
            account_package_module,
            account_interface_module,
            note_crate,
            note_slug,
        }
    }
}

/// Builds a generated account project with the provided component source.
fn build_account_project(names: &CanonAbiProjectNames, source: &str) -> Project {
    let sdk_path = sdk_crate_path();
    let cargo_toml = format!(
        r#"cargo-features = ["trim-paths"]

[package]
name = "{account_crate}"
version = "0.1.0"
edition = "2024"

[lib]
crate-type = ["cdylib"]

[dependencies]
miden = {{ path = "{sdk_path}" }}

[package.metadata.component]
package = "miden:{account_slug}"

[package.metadata.miden]
project-kind = "account"
supported-types = ["RegularAccountUpdatableCode"]

[profile.release]
trim-paths = ["diagnostics", "object"]

[profile.dev]
trim-paths = ["diagnostics", "object"]
"#,
        account_crate = names.account_crate,
        account_slug = names.account_slug,
        sdk_path = sdk_path.display(),
    );
    let miden_project_toml = format!(
        r#"[package]
name = "{account_crate}"
version = "0.1.0"

[lib]
kind = "account-component"
namespace = "miden:{account_slug}/canonabi-component@0.1.0"

[dependencies]
miden-core = "*"
miden-protocol = "*"

[package.metadata.miden]
supported-types = ["RegularAccountUpdatableCode"]
"#,
        account_crate = names.account_crate,
        account_slug = names.account_slug,
    );

    project(&names.account_crate)
        .file("Cargo.toml", &cargo_toml)
        .file("miden-project.toml", &miden_project_toml)
        .file("src/lib.rs", source)
        .build()
}

/// Builds a generated note project that imports the generated account project.
fn build_note_project(
    names: &CanonAbiProjectNames,
    account_root: &Path,
    note_body: &str,
) -> Project {
    let sdk_path = sdk_crate_path();
    let generated_wit = account_root.join("target/generated-wit");
    let cargo_toml = format!(
        r#"cargo-features = ["trim-paths"]

[package]
name = "{note_crate}"
version = "0.1.0"
edition = "2024"

[lib]
crate-type = ["cdylib"]

[dependencies]
miden = {{ path = "{sdk_path}" }}

[package.metadata.component]
package = "miden:{note_slug}"

[package.metadata.miden]
project-kind = "note-script"

[package.metadata.miden.dependencies]
"miden:{account_slug}" = {{ path = "{account_root}" }}

[package.metadata.component.target.dependencies]
"miden:{account_slug}" = {{ path = "{generated_wit}" }}

[profile.release]
trim-paths = ["diagnostics", "object"]

[profile.dev]
trim-paths = ["diagnostics", "object"]
"#,
        note_crate = names.note_crate,
        note_slug = names.note_slug,
        account_slug = names.account_slug,
        sdk_path = sdk_path.display(),
        account_root = account_root.display(),
        generated_wit = generated_wit.display(),
    );
    let miden_project_toml = format!(
        r#"[package]
name = "{note_crate}"
version = "0.1.0"

[lib]
kind = "note"
namespace = "miden:{note_slug}/miden-{note_slug}@0.1.0"

[dependencies]
miden-core = "*"
miden-protocol = "*"
{account_crate} = {{ path = "{account_root}" }}

[package.metadata.miden.dependencies]
{account_crate} = {{ wit = "{generated_wit}" }}
"#,
        note_crate = names.note_crate,
        note_slug = names.note_slug,
        account_crate = names.account_crate,
        account_root = account_root.display(),
        generated_wit = generated_wit.display(),
    );
    let source = format!(
        r#"#![no_std]
#![feature(alloc_error_handler)]

use miden::*;

use crate::bindings::miden::{package_module}::{interface_module}::*;

#[note]
struct CanonabiNote;

#[note]
impl CanonabiNote {{
    #[note_script]
    pub fn run(self, _arg: Word) {{
{note_body}
    }}
}}
"#,
        package_module = names.account_package_module,
        interface_module = names.account_interface_module,
        note_body = indent(note_body, 8),
    );

    project(&names.note_crate)
        .file("Cargo.toml", &cargo_toml)
        .file("miden-project.toml", &miden_project_toml)
        .file("src/lib.rs", &source)
        .build()
}

/// Builds a compiler test for a generated Cargo-Miden project.
fn build_generated_test(root: impl AsRef<Path>) -> CompilerTest {
    let mut builder =
        CompilerTestBuilder::rust_source_cargo_miden(root, WasmTranslationConfig::default(), []);
    builder.with_release(true);
    builder.build()
}

/// Rebuilds an executable program from a compiled note-script package.
fn note_script_program(package: &Package) -> Program {
    let note_script =
        NoteScript::from_package(package).expect("compiled package should contain a note script");
    Program::new(note_script.mast(), note_script.entrypoint())
}

/// Reads the single generated WIT file emitted by the account project.
fn read_generated_wit(project: &Project) -> String {
    let generated_wit_dir = project.root().join("target/generated-wit");
    let mut wit_paths = fs::read_dir(&generated_wit_dir)
        .unwrap_or_else(|err| {
            panic!("failed to read generated WIT dir {}: {err}", generated_wit_dir.display())
        })
        .map(|entry| entry.expect("failed to inspect generated WIT entry").path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("wit"))
        .collect::<Vec<_>>();
    wit_paths.sort();
    assert_eq!(wit_paths.len(), 1, "expected one generated WIT file, got {wit_paths:?}");
    fs::read_to_string(&wit_paths[0]).unwrap_or_else(|err| {
        panic!("failed to read generated WIT {}: {err}", wit_paths[0].display())
    })
}

/// Runs a generated account/note pair by executing the compiled note script directly.
fn run_canonabi_case(
    case: &str,
    account_source: &str,
    note_body: &str,
    assert_generated_wit: impl FnOnce(&str),
) {
    let names = CanonAbiProjectNames::new(case);
    let account_project = build_account_project(&names, account_source);
    let account_root = account_project.root();
    let mut account_test = build_generated_test(&account_root);
    let account_package = account_test.compile_package();
    assert!(account_package.is_library());
    let generated_wit = read_generated_wit(&account_project);
    assert_generated_wit(&generated_wit);

    let note_project = build_note_project(&names, &account_root, note_body);
    let mut note_test = build_generated_test(note_project.root());
    let note_package = note_test.compile_package();
    assert!(note_package.is_library());

    let program = note_script_program(note_package.as_ref());
    let mut exec = executor_with_std(vec![], None);
    exec.dependency_resolver_mut()
        .insert(*account_package.mast.digest(), account_package.mast.clone());
    exec.with_dependencies(note_package.manifest.dependencies())
        .expect("failed to add generated note dependencies");
    let _trace = exec.execute(&program, note_test.session.source_manager.clone());
}

/// Indents every line of `source` by `spaces`.
fn indent(source: &str, spaces: usize) -> String {
    let padding = " ".repeat(spaces);
    source
        .lines()
        .map(|line| format!("{padding}{line}"))
        .collect::<Vec<_>>()
        .join("\n")
}
