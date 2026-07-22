//! Shared fixtures for sibling-component mock-chain tests.
//!
//! These builders generate the temporary cargo-miden projects for a sibling-call scenario: one or
//! more sibling counter components, a caller component that depends on them, and a note that calls
//! the caller component natively. The generic project scaffolding is shared with the FPI tests via
//! the `support` module; only the sibling-specific wiring lives here.

use std::{path::Path, sync::Arc};

use miden_mast_package::Package;
use miden_protocol::account::StorageSlotName;
use midenc_integration_test_support::{cargo_proj::Project, project};

use super::super::support::*;

/// Builds sibling counter, caller component, and note projects for a sibling-call test case.
///
/// The caller component declares the sibling counter package as a dependency and calls it through
/// the generated sibling trait (intra-account cross-context calls); the note calls the caller
/// component natively as the transaction's active account.
pub(super) fn build_sibling_test_packages(
    test_name: &str,
    counter_source: &str,
    caller_source: &str,
    note_source: &str,
) -> (Arc<Package>, Arc<Package>, Arc<Package>, StorageSlotName) {
    let names = SiblingProjectNames::new(test_name);
    let counter_storage_slot =
        counter_storage_slot_name_for_package(&names.counter_account_package);

    let counter_project = build_sibling_component_project(
        &names.counter_account_name,
        &names.counter_account_package,
        "counter-contract",
        counter_source,
    );
    let counter_package = compile_rust_package(counter_project.root(), true);

    let counter_root = counter_project.root();
    let dependencies = [(names.counter_account_package.as_str(), counter_root.as_path())];
    let (caller_project, caller_package) =
        build_sibling_caller_project(&names, &dependencies, caller_source);
    let note_package =
        build_sibling_note_package(&names, caller_project.root().as_path(), note_source);

    (counter_package, caller_package, note_package, counter_storage_slot)
}

/// Builds two sibling components, a caller component depending on both, and the trigger note.
///
/// The second sibling exports a `second-counter-contract` interface so the two generated sibling
/// traits get distinct Rust names.
pub(super) fn build_multi_sibling_test_packages(
    test_name: &str,
    first_counter_source: &str,
    second_counter_source: &str,
    caller_source: &str,
    note_source: &str,
) -> (
    Arc<Package>,
    Arc<Package>,
    Arc<Package>,
    Arc<Package>,
    StorageSlotName,
    StorageSlotName,
) {
    let names = SiblingProjectNames::new(test_name);
    let second_account_name = format!("{}-second-counter-account", names.base_name);
    let second_account_package = format!("miden:{second_account_name}");

    let first_storage_slot = counter_storage_slot_name_for_package(&names.counter_account_package);
    let second_storage_slot =
        storage_slot_name_for_package(&second_account_package, "second_counter_contract");

    let first_project = build_sibling_component_project(
        &names.counter_account_name,
        &names.counter_account_package,
        "counter-contract",
        first_counter_source,
    );
    let first_package = compile_rust_package(first_project.root(), true);

    let second_project = build_sibling_component_project(
        &second_account_name,
        &second_account_package,
        "second-counter-contract",
        second_counter_source,
    );
    let second_package = compile_rust_package(second_project.root(), true);

    let first_root = first_project.root();
    let second_root = second_project.root();
    let dependencies = [
        (names.counter_account_package.as_str(), first_root.as_path()),
        (second_account_package.as_str(), second_root.as_path()),
    ];
    let (caller_project, caller_package) =
        build_sibling_caller_project(&names, &dependencies, caller_source);
    let note_package =
        build_sibling_note_package(&names, caller_project.root().as_path(), note_source);

    (
        first_package,
        second_package,
        caller_package,
        note_package,
        first_storage_slot,
        second_storage_slot,
    )
}

/// Generates one sibling component project exporting the given WIT interface.
fn build_sibling_component_project(
    account_name: &str,
    account_package: &str,
    interface: &str,
    source: &str,
) -> Project {
    project(account_name)
        .file(
            "miden-project.toml",
            &account_miden_project_toml_with_interface(account_name, account_package, interface),
        )
        .file("Cargo.toml", &account_cargo_toml_for(account_name, account_package))
        .file("src/lib.rs", source)
        .build()
}

/// Generates and compiles the caller component project with its sibling dependencies.
fn build_sibling_caller_project(
    names: &SiblingProjectNames,
    dependencies: &[(&str, &Path)],
    caller_source: &str,
) -> (Project, Arc<Package>) {
    let caller_project = project(&names.caller_account_name)
        .file(
            "miden-project.toml",
            &caller_account_miden_project_toml(
                &names.caller_account_name,
                &names.caller_account_package,
                dependencies,
            ),
        )
        .file(
            "Cargo.toml",
            &caller_account_cargo_toml(
                &names.caller_account_name,
                &names.caller_account_package,
                dependencies,
            ),
        )
        .file("src/lib.rs", caller_source)
        .build();
    let caller_package = compile_rust_package(caller_project.root(), true);
    (caller_project, caller_package)
}

/// Generates and compiles the note project depending on the caller component package.
fn build_sibling_note_package(
    names: &SiblingProjectNames,
    caller_project_root: &Path,
    note_source: &str,
) -> Arc<Package> {
    let note_project = project(&names.note_name)
        .file(
            "miden-project.toml",
            &note_miden_project_toml_for_dependency(
                &names.note_name,
                &names.note_package,
                &names.caller_account_package,
                caller_project_root,
            ),
        )
        .file(
            "Cargo.toml",
            &note_cargo_toml_for_dependency(
                &names.note_name,
                &names.caller_account_package,
                caller_project_root,
            ),
        )
        .file("src/lib.rs", note_source)
        .build();
    compile_rust_package(note_project.root(), true)
}

/// Returns a generated caller-account `miden-project.toml` with the given sibling dependencies.
fn caller_account_miden_project_toml(
    account_name: &str,
    account_package: &str,
    dependencies: &[(&str, &Path)],
) -> String {
    let namespace = account_component_namespace(account_package, "caller-account");
    let mut manifest = format!(
        r#"
[package]
name = "{account_name}"
version = "0.0.1"

[lib]
kind = "account-component"
namespace = "{namespace}"
path = "src/lib.rs"

[package.metadata.miden]
supported-types = ["RegularAccountUpdatableCode"]

[dependencies]
miden-core = "*"
miden-protocol = "*"
"#
    );
    append_miden_project_dependencies(&mut manifest, dependencies);
    manifest
}

/// Returns a generated caller-account `Cargo.toml` with the given sibling dependencies.
fn caller_account_cargo_toml(
    account_name: &str,
    account_package: &str,
    dependencies: &[(&str, &Path)],
) -> String {
    let mut manifest = account_cargo_toml_for(account_name, account_package);
    append_cargo_dependency_metadata(&mut manifest, dependencies);
    manifest
}

/// Names derived from a sibling-call test for the sibling counter, caller, and note projects.
struct SiblingProjectNames {
    base_name: String,
    counter_account_name: String,
    caller_account_name: String,
    note_name: String,
    counter_account_package: String,
    caller_account_package: String,
    note_package: String,
}

impl SiblingProjectNames {
    /// Builds Cargo crate names, WIT package names, and project paths from `test_name`.
    fn new(test_name: &str) -> Self {
        let base_name = test_name.replace('_', "-");
        let counter_account_name = format!("{base_name}-counter-account");
        let caller_account_name = format!("{base_name}-caller-account");
        let note_name = format!("{base_name}-note");
        let counter_account_package = format!("miden:{counter_account_name}");
        let caller_account_package = format!("miden:{caller_account_name}");
        let note_package = format!("miden:{note_name}");

        Self {
            base_name,
            counter_account_name,
            caller_account_name,
            note_name,
            counter_account_package,
            caller_account_package,
            note_package,
        }
    }
}
