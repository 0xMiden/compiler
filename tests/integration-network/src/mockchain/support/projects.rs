//! Shared scaffolding for generating temporary cargo-miden projects used by mock-chain tests.
//!
//! These helpers are mechanism-agnostic — they render the `miden-project.toml`/`Cargo.toml`
//! manifests and derive storage slot names for generated account-component and note projects, and
//! are reused by both the FPI tests and the sibling-component tests.

use std::path::Path;

use miden_protocol::account::StorageSlotName;
use midenc_integration_test_support::compiler_test::sdk_crate_path;

/// Returns a generated account project `miden-project.toml` exporting the given WIT interface.
pub(crate) fn account_miden_project_toml_with_interface(
    account_name: &str,
    account_package: &str,
    interface: &str,
) -> String {
    let namespace = account_component_namespace(account_package, interface);
    format!(
        r#"
[package]
name = "{account_name}"
version = "0.0.1"

[lib]
kind = "account-component"
namespace = "{namespace}"
path = "src/lib.rs"

[dependencies]
miden-core = "*"
miden-protocol = "*"

[package.metadata.miden]
supported-types = ["RegularAccountUpdatableCode"]
"#
    )
}

/// Returns the generated account `Cargo.toml` for a package without dependencies.
pub(crate) fn account_cargo_toml_for(account_name: &str, account_package: &str) -> String {
    let sdk_path = sdk_crate_path();
    format!(
        r#"
[package]
name = "{account_name}"
version = "0.0.1"
edition = "2024"
authors = []

[lib]
crate-type = ["cdylib"]

[dependencies]
miden = {{ path = "{sdk_path}", features = ["internal-wit-emit"] }}

[package.metadata.component]
package = "{account_package}"

[package.metadata.miden]
project-kind = "account"
supported-types = ["RegularAccountUpdatableCode"]

[profile.release]
opt-level = "z"
panic = "abort"
debug = false

[profile.dev]
panic = "abort"
opt-level = 1
debug-assertions = true
overflow-checks = false
debug = false
"#,
        sdk_path = sdk_path.display(),
        account_name = account_name,
        account_package = account_package,
    )
}

/// Returns the generated note project `miden-project.toml` with one Miden dependency.
pub(crate) fn note_miden_project_toml_for_dependency(
    note_name: &str,
    note_package: &str,
    dependency_package: &str,
    dependency_root: &Path,
) -> String {
    note_miden_project_toml_for_dependencies(
        note_name,
        note_package,
        &[(dependency_package, dependency_root)],
    )
}

/// Returns the generated note project `miden-project.toml` with the given Miden dependencies.
pub(crate) fn note_miden_project_toml_for_dependencies(
    note_name: &str,
    note_package: &str,
    dependencies: &[(&str, &Path)],
) -> String {
    let namespace = miden_project_namespace(note_package, note_name);
    let mut manifest = format!(
        r#"
[package]
name = "{note_name}"
version = "0.0.1"

[lib]
kind = "note"
namespace = "{namespace}"
path = "src/lib.rs"

[dependencies]
miden-core = "*"
miden-protocol = "*"
"#
    );
    append_miden_project_dependencies(&mut manifest, dependencies);
    manifest
}

/// Returns the generated note `Cargo.toml` with one Miden dependency.
pub(crate) fn note_cargo_toml_for_dependency(
    note_name: &str,
    note_package: &str,
    dependency_package: &str,
    dependency_root: &Path,
) -> String {
    note_cargo_toml_for_dependencies(
        note_name,
        note_package,
        &[(dependency_package, dependency_root)],
    )
}

/// Returns the generated note `Cargo.toml` with the given Miden dependencies.
pub(crate) fn note_cargo_toml_for_dependencies(
    note_name: &str,
    note_package: &str,
    dependencies: &[(&str, &Path)],
) -> String {
    let sdk_path = sdk_crate_path();

    let mut manifest = format!(
        r#"
[package]
name = "{note_name}"
version = "0.0.1"
edition = "2024"
authors = []

[lib]
crate-type = ["cdylib"]

[dependencies]
miden = {{ path = "{sdk_path}", features = ["internal-wit-emit"] }}

[package.metadata.miden]
project-kind = "note-script"

[package.metadata.component]
package = "{note_package}"

[profile.release]
opt-level = "z"
panic = "abort"
debug = false

[profile.dev]
panic = "abort"
opt-level = 1
debug-assertions = true
overflow-checks = false
debug = false
"#,
        sdk_path = sdk_path.display(),
        note_name = note_name,
        note_package = note_package,
    );
    append_cargo_dependency_metadata(&mut manifest, dependencies);
    manifest
}

/// Appends path dependencies and WIT mappings to a generated Miden project manifest.
pub(crate) fn append_miden_project_dependencies(
    manifest: &mut String,
    dependencies: &[(&str, &Path)],
) {
    for (dependency_package, dependency_root) in dependencies {
        let dependency_name = miden_dependency_name(dependency_package);
        manifest.push_str(&format!(
            r#"
"{dependency_name}" = {{ path = "{dependency_root}" }}
"#,
            dependency_root = dependency_root.display(),
        ));
    }

    manifest.push_str(
        r#"
[package.metadata.miden.dependencies]
"#,
    );

    for (dependency_package, dependency_root) in dependencies {
        let dependency_name = miden_dependency_name(dependency_package);
        let dependency_wit_path = dependency_root.join("target/generated-wit");
        manifest.push_str(&format!(
            r#"
"{dependency_name}" = {{ wit = "{dependency_wit_path}" }}
"#,
            dependency_wit_path = dependency_wit_path.display(),
        ));
    }
}

/// Appends package metadata for dependencies to a generated Cargo manifest.
pub(crate) fn append_cargo_dependency_metadata(
    manifest: &mut String,
    dependencies: &[(&str, &Path)],
) {
    manifest.push_str(
        r#"
[package.metadata.miden.dependencies]
"#,
    );
    for (dependency_package, dependency_root) in dependencies {
        manifest.push_str(&format!(
            r#"
"{dependency_package}" = {{ path = "{dependency_root}" }}
"#,
            dependency_package = dependency_package,
            dependency_root = dependency_root.display(),
        ));
    }

    manifest.push_str(
        r#"
[package.metadata.component.target.dependencies]
"#,
    );
    for (dependency_package, dependency_root) in dependencies {
        let dependency_wit_path = dependency_root.join("target/generated-wit");
        manifest.push_str(&format!(
            r#"
"{dependency_package}" = {{ path = "{dependency_wit_path}" }}
"#,
            dependency_package = dependency_package,
            dependency_wit_path = dependency_wit_path.display(),
        ));
    }
}

/// Returns the package-local dependency name accepted by `miden-project.toml`.
pub(crate) fn miden_dependency_name(package: &str) -> &str {
    package
        .rsplit([':', '/'])
        .next()
        .unwrap_or(package)
        .split('@')
        .next()
        .unwrap_or(package)
}

/// Returns the generated WIT namespace used by temporary note projects.
pub(crate) fn miden_project_namespace(package: &str, project_name: &str) -> String {
    format!("{package}/miden-{project_name}@0.0.1")
}

/// Builds the `[lib].namespace` for a generated account component. The interface segment must
/// equal the component trait name (kebab-case).
pub(crate) fn account_component_namespace(package: &str, interface: &str) -> String {
    format!("{package}/{interface}@0.0.1")
}

/// Returns the derived `count_map` storage slot name for the generated counter account package.
///
/// The middle segment tracks the `counter-contract` interface declared in the generated account's
/// `[lib].namespace`, from which the macro derives slot names.
pub(crate) fn counter_storage_slot_name_for_package(account_package: &str) -> StorageSlotName {
    storage_slot_name_for_package(account_package, "counter_contract")
}

/// Returns the derived `count_map` storage slot name for a package and interface segment.
pub(crate) fn storage_slot_name_for_package(
    account_package: &str,
    interface_segment: &str,
) -> StorageSlotName {
    let package_name = account_package.strip_prefix("miden:").unwrap_or(account_package);
    let namespace = sanitize_slot_name_component(package_name);
    StorageSlotName::new(format!("{namespace}::{interface_segment}::count_map"))
        .expect("generated counter storage slot name must be valid")
}

/// Normalizes a generated component package into its storage slot namespace segment.
fn sanitize_slot_name_component(component: &str) -> String {
    let component = component.split('@').next().unwrap_or(component);
    let mut out: String = component
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();

    if out.is_empty() {
        out.push('x');
    }
    if out.starts_with('_') {
        out.insert(0, 'x');
    }

    out
}
