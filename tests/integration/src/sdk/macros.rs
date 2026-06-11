use std::panic::{self, AssertUnwindSafe};

use super::*;

fn cargo_check_miden_target(project: &crate::cargo_proj::Project) -> std::process::Output {
    std::process::Command::new("cargo")
        .arg("check")
        .arg("--target")
        .arg("wasm32-wasip2")
        .arg("--target-dir")
        .arg(project.build_dir())
        .env("RUSTFLAGS", "--cfg miden -C target-feature=+bulk-memory,+wide-arithmetic")
        .current_dir(project.root())
        .output()
        .expect("failed to spawn `cargo check` for the component macro regression test")
}

#[test]
fn component_macros_account_and_note() {
    let config = WasmTranslationConfig::default();
    let mut account = CompilerTest::rust_source_cargo_miden(
        "../fixtures/components/component-macros-account",
        config.clone(),
        [],
    );
    let result = panic::catch_unwind(AssertUnwindSafe(move || account.compile_package()));
    let panic_message = match result {
        Ok(_) => {
            panic!("Expected component export lifting with indirect pointer parameters to fail")
        }
        Err(panic_info) => {
            if let Some(message) = panic_info.downcast_ref::<String>() {
                message.clone()
            } else if let Some(message) = panic_info.downcast_ref::<&str>() {
                message.to_string()
            } else {
                "Unknown panic".to_string()
            }
        }
    };

    assert!(
        panic_message.contains("not yet implemented"),
        "unexpected panic message: {panic_message}"
    );

    //    let builder = CompilerTestBuilder::rust_source_cargo_miden(
    //        "../fixtures/components/component-macros-note",
    //        config,
    //        [],
    //    assert!(
    //        panic_message.contains("not yet implemented")
    //            && panic_message.contains("indirect pointer parameters"),
    //        "unexpected panic message: {panic_message}"
    //    );
    //    let mut note = builder.build();
    //    let note_package = note.compile_package();
    //    let program = note_package.unwrap_program();
    //
    //    let mut exec = executor_with_std(vec![], None);
    //    exec.dependency_resolver_mut()
    //        .add(account_package.digest(), account_package.into());
    //    exec.with_dependencies(note_package.manifest.dependencies())
    //        .expect("failed to add package dependencies");
    //    exec.execute(&program, note.session.source_manager.clone());
}

#[test]
fn auth_components_require_an_auth_script_method() {
    let name = "auth_components_require_an_auth_script_method";
    let sdk_path = sdk_crate_path();
    let namespace = format!("miden:{}/auth-component@0.0.1", name.replace('_', "-"));
    let component_package = format!("miden:{}", name.replace('_', "-"));
    let miden_project_toml = format!(
        r#"
[package]
name = "{name}"
version = "0.0.1"

[lib]
kind = "account-component"
namespace = "{namespace}"

[dependencies]
miden-core = "*"
miden-protocol = "*"
"#
    );
    let cargo_toml = format!(
        r#"
[package]
name = "{name}"
version = "0.0.1"
edition = "2024"
authors = []

[lib]
crate-type = ["cdylib"]

[dependencies]
miden = {{ path = "{sdk_path}" }}

[package.metadata.component]
package = "{component_package}"

[package.metadata.miden]
project-kind = "authentication-component"
"#,
        name = name,
        sdk_path = sdk_path.display(),
        component_package = component_package,
    );

    let lib_rs = r#"#![no_std]
#![feature(alloc_error_handler)]

use miden::{component, component_storage, Word};

#[component_storage]
struct AuthComponentStorage;

#[component]
trait AuthComponent {
    fn auth_procedure(&self, _arg: Word);
}

#[component]
impl AuthComponent for AuthComponentStorage {
    fn auth_procedure(&self, _arg: Word) {}
}
"#;

    let cargo_proj = project(name)
        .file("miden-project.toml", &miden_project_toml)
        .file("Cargo.toml", &cargo_toml)
        .file("src/lib.rs", lib_rs)
        .build();

    let output = cargo_check_miden_target(&cargo_proj);
    assert!(
        !output.status.success(),
        "expected auth-component compilation to fail without `#[auth_script]`"
    );
    let panic_message = String::from_utf8_lossy(&output.stderr);

    assert!(
        panic_message
            .contains("authentication components require exactly one `#[auth_script]` method"),
        "unexpected panic message: {panic_message}"
    );
}

#[test]
fn auth_script_requires_a_component_trait() {
    let name = "auth_script_requires_a_component_trait";
    let sdk_path = sdk_crate_path();
    let namespace = component_namespace(name);
    let component_package = format!("miden:{}", name.replace('_', "-"));
    let miden_project_toml = format!(
        r#"
[package]
name = "{name}"
version = "0.0.1"

[lib]
kind = "account-component"
namespace = "{namespace}"

[dependencies]
miden-core = "*"
miden-protocol = "*"
"#
    );
    let cargo_toml = format!(
        r#"
[package]
name = "{name}"
version = "0.0.1"
edition = "2024"
authors = []

[lib]
crate-type = ["cdylib"]

[dependencies]
miden = {{ path = "{sdk_path}" }}

[package.metadata.component]
package = "{component_package}"

[package.metadata.miden]
project-kind = "authentication-component"
"#,
        name = name,
        sdk_path = sdk_path.display(),
        component_package = component_package,
    );

    // `#[auth_script]` is applied to a trait method, but the trait is not annotated with
    // `#[component]`, so the helper marker attribute is left unconsumed and rejected by rustc.
    let lib_rs = r#"#![no_std]
#![feature(alloc_error_handler)]

use miden::{auth_script, Word};

trait AuthComponent {
    #[auth_script]
    fn auth_procedure(&mut self, _arg: Word);
}
"#;

    let cargo_proj = project(name)
        .file("miden-project.toml", &miden_project_toml)
        .file("Cargo.toml", &cargo_toml)
        .file("src/lib.rs", lib_rs)
        .build();

    let output = cargo_check_miden_target(&cargo_proj);
    assert!(
        !output.status.success(),
        "expected auth-script compilation to fail outside a `#[component]` trait"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        stderr.contains("miden_auth_script_requires_component"),
        "unexpected stderr: {stderr}"
    );
}

#[test]
fn note_script_requires_a_note_impl() {
    let name = "note_script_requires_a_note_impl";
    let sdk_path = sdk_crate_path();
    let namespace = component_namespace(name);
    let component_package = format!("miden:{}", name.replace('_', "-"));
    let miden_project_toml = format!(
        r#"
[package]
name = "{name}"
version = "0.0.1"

[lib]
kind = "note"
namespace = "{namespace}"

[dependencies]
miden-core = "*"
miden-protocol = "*"
"#
    );
    let cargo_toml = format!(
        r#"
[package]
name = "{name}"
version = "0.0.1"
edition = "2024"
authors = []

[lib]
crate-type = ["cdylib"]

[dependencies]
miden = {{ path = "{sdk_path}" }}

[package.metadata.component]
package = "{component_package}"
"#,
        name = name,
        sdk_path = sdk_path.display(),
        component_package = component_package,
    );

    let lib_rs = r#"#![no_std]
#![feature(alloc_error_handler)]

use miden::{note, note_script, Word};

#[note]
struct MyNote;

impl MyNote {
    #[note_script]
    pub fn execute(self, _arg: Word) {}
}
"#;

    let cargo_proj = project(name)
        .file("miden-project.toml", &miden_project_toml)
        .file("Cargo.toml", &cargo_toml)
        .file("src/lib.rs", lib_rs)
        .build();

    let output = cargo_check_miden_target(&cargo_proj);
    assert!(
        !output.status.success(),
        "expected note-script compilation to fail outside a `#[note]` impl"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        stderr.contains("miden_note_script_requires_note"),
        "unexpected stderr: {stderr}"
    );
}

#[test]
fn note_script_account_param_requires_account_wrapper_type() {
    let name = "note_script_account_param_requires_account_wrapper_type";
    let sdk_path = sdk_crate_path();
    let namespace = component_namespace(name);
    let component_package = format!("miden:{}", name.replace('_', "-"));
    let miden_project_toml = format!(
        r#"
[package]
name = "{name}"
version = "0.0.1"

[lib]
kind = "note"
namespace = "{namespace}"

[dependencies]
miden-core = "*"
miden-protocol = "*"
"#
    );
    let cargo_toml = format!(
        r#"
[package]
name = "{name}"
version = "0.0.1"
edition = "2024"
authors = []

[lib]
crate-type = ["cdylib"]

[dependencies]
miden = {{ path = "{sdk_path}" }}

[package.metadata.component]
package = "{component_package}"
"#,
        name = name,
        sdk_path = sdk_path.display(),
        component_package = component_package,
    );

    // The account parameter references a type that was not generated by `#[account(...)]`;
    // the generated glue must reject it through the `AccountWrapper` bound.
    let lib_rs = r#"#![no_std]
#![feature(alloc_error_handler)]

use miden::{note, note_script, Word};

#[derive(Default)]
struct NotAnAccount;

#[note]
struct MyNote;

#[note]
impl MyNote {
    #[note_script]
    pub fn execute(self, _arg: Word, _account: &mut NotAnAccount) {}
}
"#;

    let cargo_proj = project(name)
        .file("miden-project.toml", &miden_project_toml)
        .file("Cargo.toml", &cargo_toml)
        .file("src/lib.rs", lib_rs)
        .build();

    let output = cargo_check_miden_target(&cargo_proj);
    assert!(
        !output.status.success(),
        "expected the account parameter type to be rejected without `#[account(...)]`"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        stderr.contains("`NotAnAccount` is not an account wrapper"),
        "unexpected stderr: {stderr}"
    );
    assert!(
        stderr.contains("define a struct with `#[account(...)]`"),
        "unexpected stderr: {stderr}"
    );
}

/// Builds a generated account-component project whose component trait is named `TestComponent`
/// (WIT interface `test-component`, matching the generated `[lib].namespace`).
fn account_component_project(name: &str, lib_rs: &str) -> crate::cargo_proj::Project {
    let sdk_path = sdk_crate_path();
    let namespace = base::account_component_namespace(name, "test-component");
    let component_package = format!("miden:{}", name.replace('_', "-"));
    let miden_project_toml = format!(
        r#"
[package]
name = "{name}"
version = "0.0.1"

[lib]
kind = "account-component"
namespace = "{namespace}"

[dependencies]
miden-core = "*"
miden-protocol = "*"
"#
    );
    let cargo_toml = format!(
        r#"
[package]
name = "{name}"
version = "0.0.1"
edition = "2024"
authors = []

[lib]
crate-type = ["cdylib"]

[dependencies]
miden = {{ path = "{sdk_path}" }}

[package.metadata.component]
package = "{component_package}"

[package.metadata.miden]
project-kind = "account"
supported-types = ["RegularAccountUpdatableCode"]
"#,
        sdk_path = sdk_path.display(),
    );

    project(name)
        .file("miden-project.toml", &miden_project_toml)
        .file("Cargo.toml", &cargo_toml)
        .file("src/lib.rs", lib_rs)
        .build()
}

#[test]
fn component_trait_requires_the_component_attribute() {
    // The trait is missing `#[component]`: the implementation expansion references the hidden
    // marker constant the trait expansion would have injected, so rustc reports it as a missing
    // associated item instead of silently skipping the trait-side validation.
    let lib_rs = r#"#![no_std]
#![feature(alloc_error_handler)]

use miden::{component, component_storage, felt, Felt};

#[component_storage]
struct TestComponentStorage;

trait TestComponent {
    fn value(&self) -> Felt;
}

#[component]
impl TestComponent for TestComponentStorage {
    fn value(&self) -> Felt {
        felt!(1)
    }
}
"#;

    let cargo_proj =
        account_component_project("component_trait_requires_the_component_attribute", lib_rs);
    let output = cargo_check_miden_target(&cargo_proj);
    assert!(
        !output.status.success(),
        "expected compilation to fail without `#[component]` on the trait"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(stderr.contains("__MIDEN_COMPONENT_TRAIT_MARKER"), "unexpected stderr: {stderr}");
}

#[test]
fn component_trait_may_live_in_a_nested_module() {
    // All generation happens at the `impl` expansion, so the component trait can be declared in
    // any module, e.g. to let it share a name with the storage struct.
    let lib_rs = r#"#![no_std]
#![feature(alloc_error_handler)]

use miden::{component, component_storage, felt, Felt};

#[component_storage]
struct TestComponentStorage;

pub mod api {
    use miden::{component, Felt};

    #[component]
    pub trait TestComponent {
        fn value(&self) -> Felt;
    }
}

#[component]
impl api::TestComponent for TestComponentStorage {
    fn value(&self) -> Felt {
        felt!(1)
    }
}
"#;

    let cargo_proj =
        account_component_project("component_trait_may_live_in_a_nested_module", lib_rs);
    let output = cargo_check_miden_target(&cargo_proj);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "expected a component trait in a nested module to compile: {stderr}"
    );
}

#[test]
fn component_trait_methods_reject_default_bodies() {
    // Exports are derived from the `impl` block, so a defaulted trait method that is not
    // overridden there would silently disappear from the component's interface.
    let lib_rs = r#"#![no_std]
#![feature(alloc_error_handler)]

use miden::{component, component_storage, felt, Felt};

#[component_storage]
struct TestComponentStorage;

#[component]
trait TestComponent {
    fn value(&self) -> Felt {
        felt!(0)
    }
}

#[component]
impl TestComponent for TestComponentStorage {
    fn value(&self) -> Felt {
        felt!(1)
    }
}
"#;

    let cargo_proj =
        account_component_project("component_trait_methods_reject_default_bodies", lib_rs);
    let output = cargo_check_miden_target(&cargo_proj);
    assert!(!output.status.success(), "expected default trait method bodies to be rejected");
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        stderr.contains("component trait methods cannot have default bodies"),
        "unexpected stderr: {stderr}"
    );
}

#[test]
fn component_traits_reject_generic_parameters() {
    let lib_rs = r#"#![no_std]
#![feature(alloc_error_handler)]

use miden::{component, component_storage, felt, Felt};

#[component_storage]
struct TestComponentStorage;

#[component]
trait TestComponent<T> {
    fn value(&self) -> Felt;
}

#[component]
impl TestComponent for TestComponentStorage {
    fn value(&self) -> Felt {
        felt!(1)
    }
}
"#;

    let cargo_proj =
        account_component_project("component_traits_reject_generic_parameters", lib_rs);
    let output = cargo_check_miden_target(&cargo_proj);
    assert!(!output.status.success(), "expected generic component traits to be rejected");
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        stderr.contains("component traits cannot be generic"),
        "unexpected stderr: {stderr}"
    );
}

#[test]
fn component_trait_requires_a_miden_project_manifest() {
    // Without a `miden-project.toml` there is no `[lib].namespace` to validate the component's
    // interface against; the macro must name the missing manifest instead of failing the
    // namespace check against synthesized placeholder metadata.
    let name = "component_trait_requires_a_miden_project_manifest";
    let sdk_path = sdk_crate_path();
    let cargo_toml = format!(
        r#"
[package]
name = "{name}"
version = "0.0.1"
edition = "2024"
authors = []

[lib]
crate-type = ["cdylib"]

[dependencies]
miden = {{ path = "{sdk_path}" }}

[package.metadata.miden]
project-kind = "account"
"#,
        sdk_path = sdk_path.display(),
    );

    let lib_rs = r#"#![no_std]
#![feature(alloc_error_handler)]

use miden::{component, component_storage, felt, Felt};

#[component_storage]
struct TestComponentStorage;

#[component]
trait TestComponent {
    fn value(&self) -> Felt;
}

#[component]
impl TestComponent for TestComponentStorage {
    fn value(&self) -> Felt {
        felt!(1)
    }
}
"#;

    let cargo_proj =
        project(name).file("Cargo.toml", &cargo_toml).file("src/lib.rs", lib_rs).build();

    let output = cargo_check_miden_target(&cargo_proj);
    assert!(
        !output.status.success(),
        "expected compilation to fail without a miden-project.toml"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        stderr.contains("requires a `miden-project.toml`"),
        "unexpected stderr: {stderr}"
    );
}

#[test]
fn component_impl_rejects_a_trait_alias_mismatching_the_namespace() {
    // The WIT interface is named after the trait as spelled in the impl, so an alias would
    // silently generate an interface named after the alias; the impl-side namespace validation
    // must reject it even though the declared trait name validates fine.
    let lib_rs = r#"#![no_std]
#![feature(alloc_error_handler)]

use miden::{component, component_storage, felt, Felt};

#[component_storage]
struct TestComponentStorage;

pub mod api {
    use miden::{component, Felt};

    #[component]
    pub trait TestComponent {
        fn value(&self) -> Felt;
    }
}

use api::TestComponent as Alias;

#[component]
impl Alias for TestComponentStorage {
    fn value(&self) -> Felt {
        felt!(1)
    }
}
"#;

    let cargo_proj = account_component_project(
        "component_impl_rejects_a_trait_alias_mismatching_the_namespace",
        lib_rs,
    );
    let output = cargo_check_miden_target(&cargo_proj);
    assert!(
        !output.status.success(),
        "expected an aliased component trait impl to fail namespace validation"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(stderr.contains("produces WIT interface `alias`"), "unexpected stderr: {stderr}");
}

#[test]
fn component_impl_requires_component_storage_on_the_storage_struct() {
    // Without `#[component_storage]` the struct has no metadata link section, account trait
    // impls, or runtime boilerplate; the impl expansion references a hidden marker constant so
    // the omission fails loudly instead of building a component without storage metadata.
    let lib_rs = r#"#![no_std]
#![feature(alloc_error_handler)]

use miden::{component, felt, Felt};

#[derive(Default)]
struct TestComponentStorage;

#[component]
trait TestComponent {
    fn value(&self) -> Felt;
}

#[component]
impl TestComponent for TestComponentStorage {
    fn value(&self) -> Felt {
        felt!(1)
    }
}
"#;

    let cargo_proj = account_component_project(
        "component_impl_requires_component_storage_on_the_storage_struct",
        lib_rs,
    );
    let output = cargo_check_miden_target(&cargo_proj);
    assert!(
        !output.status.success(),
        "expected compilation to fail without `#[component_storage]` on the storage struct"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        stderr.contains("__MIDEN_COMPONENT_STORAGE_MARKER"),
        "unexpected stderr: {stderr}"
    );
}

#[test]
fn component_storage_fields_require_a_miden_project_manifest() {
    // Storage slot names derive from the `[lib].namespace` interface segment; without a
    // `miden-project.toml` they would silently be derived from placeholder metadata
    // (`empty::empty::<field>`).
    let name = "component_storage_fields_require_a_miden_project_manifest";
    let sdk_path = sdk_crate_path();
    let cargo_toml = format!(
        r#"
[package]
name = "{name}"
version = "0.0.1"
edition = "2024"
authors = []

[lib]
crate-type = ["cdylib"]

[dependencies]
miden = {{ path = "{sdk_path}" }}

[package.metadata.miden]
project-kind = "account"
"#,
        sdk_path = sdk_path.display(),
    );

    let lib_rs = r#"#![no_std]
#![feature(alloc_error_handler)]

use miden::{component_storage, StorageValue, Word};

#[component_storage]
struct TestComponentStorage {
    #[storage(description = "some value")]
    value: StorageValue<Word>,
}
"#;

    let cargo_proj =
        project(name).file("Cargo.toml", &cargo_toml).file("src/lib.rs", lib_rs).build();

    let output = cargo_check_miden_target(&cargo_proj);
    assert!(
        !output.status.success(),
        "expected storage fields without a miden-project.toml to be rejected"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        stderr.contains("storage slot names derive from the `[lib].namespace`"),
        "unexpected stderr: {stderr}"
    );
}
