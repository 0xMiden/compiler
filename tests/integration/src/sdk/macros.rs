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
    let namespace = base::account_component_namespace(name, "auth-component");
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

/// Hand-written stand-in for the generated WIT of a sibling component dependency.
///
/// Mirrors the shape the `#[component]` macro writes to `target/generated-wit` so the sibling
/// tests don't have to compile a real dependency project with cargo-miden.
const TEST_SIBLING_GENERATED_WIT: &str = r#"package miden:test-sibling@0.0.1;

use miden:base/core-types@1.0.0;

interface test-sibling {
    use core-types.{felt};

    get-value: func() -> felt;
    bump-value: func(delta: felt) -> felt;
}

world test-sibling-world {
    export test-sibling;
}
"#;

/// A sibling WIT whose interface owns a record type passed across a sibling call.
///
/// Exercises the dependency-type remapping in the sibling generator: the owned `point` record must
/// resolve to the caller's `crate::bindings` rather than being regenerated in the sibling bindings.
const TEST_SIBLING_OWNED_TYPE_WIT: &str = r#"package miden:test-sibling@0.0.1;

use miden:base/core-types@1.0.0;

interface test-sibling {
    use core-types.{felt};

    record point {
        x: felt,
        y: felt,
    }

    echo-point: func(p: point) -> point;
}

world test-sibling-world {
    export test-sibling;
}
"#;

/// Builds an account component project with one sibling component dependency named `test-sibling`.
///
/// The sibling exists only as its generated WIT (under `dep/target/generated-wit`), which is all
/// the macros need: sibling calls resolve at link time and read no `.masp` during expansion.
fn account_component_project_with_sibling_dep(
    name: &str,
    lib_rs: &str,
) -> crate::cargo_proj::Project {
    account_component_project_with_sibling_dep_inner(name, lib_rs, TEST_SIBLING_GENERATED_WIT, true)
}

/// Builds an account component project with one sibling component dependency named `test-sibling`.
///
/// `sibling_wit` is the dependency's generated WIT written under `dep/target/generated-wit`.
/// `declare_sibling_wit` controls whether that WIT is declared under
/// `[package.metadata.miden.dependencies]` in `miden-project.toml`. Omitting it reproduces the
/// case where the reference selects (the WIT is read from `target/generated-wit`) but the inline
/// `generate!` cannot resolve the import, so the macro emits the missing-WIT diagnostic.
fn account_component_project_with_sibling_dep_inner(
    name: &str,
    lib_rs: &str,
    sibling_wit: &str,
    declare_sibling_wit: bool,
) -> crate::cargo_proj::Project {
    let sdk_path = sdk_crate_path();
    let namespace = base::account_component_namespace(name, "test-component");
    let component_package = format!("miden:{}", name.replace('_', "-"));
    let sibling_wit_entry = if declare_sibling_wit {
        "\n[package.metadata.miden.dependencies]\ntest-sibling = { wit = \
         \"dep/target/generated-wit\" }\n"
    } else {
        ""
    };
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
test-sibling = {{ path = "dep" }}
{sibling_wit_entry}"#
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

[package.metadata.component.target.dependencies]
"miden:test-sibling" = {{ path = "dep/target/generated-wit/test-sibling.wit" }}

[package.metadata.miden]
project-kind = "account"
supported-types = ["RegularAccountUpdatableCode"]
"#,
        sdk_path = sdk_path.display(),
    );

    project(name)
        .file("miden-project.toml", &miden_project_toml)
        .file("Cargo.toml", &cargo_toml)
        .file("dep/target/generated-wit/test-sibling.wit", sibling_wit)
        .file("src/lib.rs", lib_rs)
        .build()
}

#[test]
fn component_trait_with_sibling_dependency_compiles() {
    // The sibling reference generates `trait TestSibling` with default methods calling the
    // dependency's imports, attached to the storage struct through the `NativeAccount` blanket
    // impl — so both the supertrait bound and the `self.*` calls type-check with no user glue.
    let lib_rs = r#"#![no_std]
#![feature(alloc_error_handler)]

use miden::{component, component_storage, felt, native_account::NativeAccount, Felt};

#[component_storage]
struct TestComponentStorage;

#[component(test_sibling::TestSibling)]
trait TestComponent: NativeAccount + TestSibling {
    fn value(&mut self) -> Felt;
}

#[component]
impl TestComponent for TestComponentStorage {
    fn value(&mut self) -> Felt {
        let _bumped = self.bump_value(felt!(1));
        self.get_value()
    }
}
"#;

    let cargo_proj =
        account_component_project_with_sibling_dep("component_sibling_dependency", lib_rs);
    let output = cargo_check_miden_target(&cargo_proj);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "expected the sibling component reference to compile: {stderr}"
    );
}

#[test]
fn component_traits_allow_plain_supertraits() {
    // Supertraits no longer require sibling references; declaring account traits the component
    // relies on is part of its API.
    let lib_rs = r#"#![no_std]
#![feature(alloc_error_handler)]

use miden::{component, component_storage, felt, native_account::NativeAccount, Felt};

#[component_storage]
struct TestComponentStorage;

#[component]
trait TestComponent: NativeAccount {
    fn value(&self) -> Felt;
}

#[component]
impl TestComponent for TestComponentStorage {
    fn value(&self) -> Felt {
        felt!(1)
    }
}
"#;

    let cargo_proj = account_component_project("component_traits_allow_plain_supertraits", lib_rs);
    let output = cargo_check_miden_target(&cargo_proj);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "expected a component trait with supertraits to compile: {stderr}"
    );
}

#[test]
fn component_impl_blocks_reject_arguments() {
    let lib_rs = r#"#![no_std]
#![feature(alloc_error_handler)]

use miden::{component, component_storage, felt, Felt};

#[component_storage]
struct TestComponentStorage;

#[component]
trait TestComponent {
    fn value(&self) -> Felt;
}

#[component(test_sibling::TestSibling)]
impl TestComponent for TestComponentStorage {
    fn value(&self) -> Felt {
        felt!(1)
    }
}
"#;

    let cargo_proj = account_component_project_with_sibling_dep(
        "component_impl_blocks_reject_arguments",
        lib_rs,
    );
    let output = cargo_check_miden_target(&cargo_proj);
    assert!(!output.status.success(), "expected arguments on the impl block to be rejected");
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        stderr.contains("only accepts arguments on the component trait declaration"),
        "unexpected stderr: {stderr}"
    );
}

#[test]
fn component_sibling_references_require_the_interface_segment() {
    let lib_rs = r#"#![no_std]
#![feature(alloc_error_handler)]

use miden::{component, component_storage, felt, Felt};

#[component_storage]
struct TestComponentStorage;

#[component(test_sibling)]
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

    let cargo_proj = account_component_project_with_sibling_dep(
        "component_sibling_requires_interface_segment",
        lib_rs,
    );
    let output = cargo_check_miden_target(&cargo_proj);
    assert!(!output.status.success(), "expected a bare sibling reference to be rejected");
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(stderr.contains("missing the WIT interface name"), "unexpected stderr: {stderr}");
    assert!(stderr.contains("test_sibling::TestSibling"), "unexpected stderr: {stderr}");
}

#[test]
fn component_sibling_references_validate_the_interface_name() {
    let lib_rs = r#"#![no_std]
#![feature(alloc_error_handler)]

use miden::{component, component_storage, felt, Felt};

#[component_storage]
struct TestComponentStorage;

#[component(test_sibling::WrongName)]
trait TestComponent: WrongName {
    fn value(&self) -> Felt;
}

#[component]
impl TestComponent for TestComponentStorage {
    fn value(&self) -> Felt {
        felt!(1)
    }
}
"#;

    let cargo_proj = account_component_project_with_sibling_dep(
        "component_sibling_validates_interface_name",
        lib_rs,
    );
    let output = cargo_check_miden_target(&cargo_proj);
    assert!(!output.status.success(), "expected an unknown sibling interface to be rejected");
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        stderr.contains("does not export a WIT interface named `wrong-name`"),
        "unexpected stderr: {stderr}"
    );
    assert!(
        stderr.contains("exported interfaces: test-sibling"),
        "unexpected stderr: {stderr}"
    );
}

#[test]
fn component_sibling_reports_missing_wit_dependency_manifest_entry() {
    // The reference is valid and the dependency WIT exists under `target/generated-wit`, but the
    // `[package.metadata.miden.dependencies]` entry that puts it on the macro's WIT search path is
    // omitted. Expansion should surface the actionable diagnostic, not a bare wit-parser error.
    let lib_rs = r#"#![no_std]
#![feature(alloc_error_handler)]

use miden::{component, component_storage, felt, native_account::NativeAccount, Felt};

#[component_storage]
struct TestComponentStorage;

#[component(test_sibling::TestSibling)]
trait TestComponent: NativeAccount + TestSibling {
    fn value(&mut self) -> Felt;
}

#[component]
impl TestComponent for TestComponentStorage {
    fn value(&mut self) -> Felt {
        self.get_value()
    }
}
"#;

    let cargo_proj = account_component_project_with_sibling_dep_inner(
        "component_sibling_missing_wit_entry",
        lib_rs,
        TEST_SIBLING_GENERATED_WIT,
        false,
    );
    let output = cargo_check_miden_target(&cargo_proj);
    assert!(
        !output.status.success(),
        "expected the missing sibling WIT entry to fail the build"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        stderr.contains("could not resolve the WIT for sibling component dependencies"),
        "unexpected stderr: {stderr}"
    );
    assert!(
        stderr.contains("[package.metadata.miden.dependencies]"),
        "unexpected stderr: {stderr}"
    );
}

#[test]
fn component_sibling_call_passes_an_interface_owned_record() {
    // The sibling interface owns the `point` record and passes it across a sibling call. This
    // exercises the dependency-type remap path (`dependency_type_with_entries`): the generated
    // `echo_point` resolves `Point` to the caller's `crate::bindings`, where the impl-site
    // `#[component]` materializes it from the imported sibling interface.
    let lib_rs = r#"#![no_std]
#![feature(alloc_error_handler)]

use miden::{component, component_storage, native_account::NativeAccount, Felt};

use crate::bindings::miden::test_sibling::test_sibling::Point;

#[component_storage]
struct TestComponentStorage;

#[component(test_sibling::TestSibling)]
trait TestComponent: NativeAccount + TestSibling {
    fn relay_point(&mut self, x: Felt, y: Felt) -> Felt;
}

#[component]
impl TestComponent for TestComponentStorage {
    fn relay_point(&mut self, x: Felt, y: Felt) -> Felt {
        let echoed = self.echo_point(Point { x, y });
        echoed.x
    }
}
"#;

    let cargo_proj = account_component_project_with_sibling_dep_inner(
        "component_sibling_owned_record",
        lib_rs,
        TEST_SIBLING_OWNED_TYPE_WIT,
        true,
    );
    let output = cargo_check_miden_target(&cargo_proj);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "expected a sibling call passing an interface-owned record to compile: {stderr}"
    );
}

/// Builds a generated account project that deliberately has no `miden-project.toml`, for tests
/// of the missing-manifest diagnostics.
fn manifestless_account_project(name: &str, lib_rs: &str) -> crate::cargo_proj::Project {
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

    project(name).file("Cargo.toml", &cargo_toml).file("src/lib.rs", lib_rs).build()
}

#[test]
fn component_trait_requires_a_miden_project_manifest() {
    // Without a `miden-project.toml` there is no `[lib].namespace` to validate the component's
    // interface against; the macro must name the missing manifest instead of failing the
    // namespace check against synthesized placeholder metadata.
    let name = "component_trait_requires_a_miden_project_manifest";
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

    let cargo_proj = manifestless_account_project(name, lib_rs);

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
    // The impl-side expansion must report the same friendly error, not a namespace mismatch
    // against the synthesized placeholder metadata.
    assert!(!stderr.contains("miden:empty"), "unexpected stderr: {stderr}");
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
    let lib_rs = r#"#![no_std]
#![feature(alloc_error_handler)]

use miden::{component_storage, StorageValue, Word};

#[component_storage]
struct TestComponentStorage {
    #[storage(description = "some value")]
    value: StorageValue<Word>,
}
"#;

    let cargo_proj = manifestless_account_project(name, lib_rs);

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

/// Builds a generated account-component project like [`account_component_project`], but with an
/// explicitly provided `[lib].namespace`.
fn account_component_project_with_namespace(
    name: &str,
    namespace: &str,
    lib_rs: &str,
) -> crate::cargo_proj::Project {
    let sdk_path = sdk_crate_path();
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

/// Component source whose trait yields WIT interface `test-component`, shared by the namespace
/// negative tests.
const NAMESPACE_TEST_COMPONENT: &str = r#"#![no_std]
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

#[test]
fn component_namespace_rejects_a_mismatching_package() {
    // The interface segment matches the trait, but the package segment diverges from the
    // manifest's package name; only full namespace equality catches it.
    let name = "component_namespace_rejects_a_mismatching_package";
    let namespace = "miden:wrong-package/test-component@0.0.1";
    let cargo_proj =
        account_component_project_with_namespace(name, namespace, NAMESPACE_TEST_COMPONENT);

    let output = cargo_check_miden_target(&cargo_proj);
    assert!(!output.status.success(), "expected a wrong package segment to be rejected");
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(stderr.contains("declares `miden:wrong-package/"), "unexpected stderr: {stderr}");
    assert!(
        stderr.contains(&format!(
            "Update `[lib].namespace` to `miden:{}/test-component@0.0.1`",
            name.replace('_', "-")
        )),
        "unexpected stderr: {stderr}"
    );
}

#[test]
fn component_namespace_rejects_a_mismatching_version() {
    let name = "component_namespace_rejects_a_mismatching_version";
    let namespace = format!("miden:{}/test-component@9.9.9", name.replace('_', "-"));
    let cargo_proj =
        account_component_project_with_namespace(name, &namespace, NAMESPACE_TEST_COMPONENT);

    let output = cargo_check_miden_target(&cargo_proj);
    assert!(!output.status.success(), "expected a wrong namespace version to be rejected");
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(stderr.contains("@9.9.9`"), "unexpected stderr: {stderr}");
    assert!(stderr.contains("Update `[lib].namespace` to"), "unexpected stderr: {stderr}");
}

#[test]
fn auth_script_on_an_impl_method_is_rejected() {
    // The outer `#[component]` impl expansion sees the raw `#[auth_script]` tokens before the
    // standalone attribute macro runs; it must hard-error rather than silently strip the marker.
    let lib_rs = r#"#![no_std]
#![feature(alloc_error_handler)]

use miden::{auth_script, component, component_storage, felt, Felt};

#[component_storage]
struct TestComponentStorage;

#[component]
trait TestComponent {
    fn value(&self) -> Felt;
}

#[component]
impl TestComponent for TestComponentStorage {
    #[auth_script]
    fn value(&self) -> Felt {
        felt!(1)
    }
}
"#;

    let cargo_proj = account_component_project("auth_script_on_an_impl_method_is_rejected", lib_rs);
    let output = cargo_check_miden_target(&cargo_proj);
    assert!(
        !output.status.success(),
        "expected `#[auth_script]` on an impl method to be rejected"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(stderr.contains("not the implementation block"), "unexpected stderr: {stderr}");
}

#[test]
fn component_storage_rejects_generic_parameters() {
    // The storage expansion emits bare-ident impls (`Default`, account traits, the marker
    // constant), so a generic struct must get the actionable macro error rather than a pile of
    // rustc "missing generics" errors pointing at generated impls.
    let lib_rs = r#"#![no_std]
#![feature(alloc_error_handler)]

use miden::{component_storage, StorageValue, Word};

#[component_storage]
struct TestComponentStorage<T> {
    #[storage(description = "some value")]
    value: StorageValue<Word>,
    marker: core::marker::PhantomData<T>,
}
"#;

    let cargo_proj =
        account_component_project("component_storage_rejects_generic_parameters", lib_rs);
    let output = cargo_check_miden_target(&cargo_proj);
    assert!(!output.status.success(), "expected generic storage structs to be rejected");
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        stderr.contains("component storage structs cannot be generic"),
        "unexpected stderr: {stderr}"
    );
}

#[test]
fn auth_script_in_a_plain_impl_block_is_rejected() {
    // A non-pub method with a body parses as a `TraitItemFn` (the body reads as a default), so
    // without an explicit check the macro would append its helper marker and the user would see
    // rustc's "cannot find attribute" error instead of the placement guidance.
    let lib_rs = r#"#![no_std]
#![feature(alloc_error_handler)]

use miden::{auth_script, component_storage, Word};

#[component_storage]
struct TestComponentStorage;

struct PlainAuth;

impl PlainAuth {
    #[auth_script]
    fn check_signature(&mut self, _arg: Word) {}
}
"#;

    let cargo_proj =
        account_component_project("auth_script_in_a_plain_impl_block_is_rejected", lib_rs);
    let output = cargo_check_miden_target(&cargo_proj);
    assert!(
        !output.status.success(),
        "expected `#[auth_script]` in a plain impl block to be rejected"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(stderr.contains("not the implementation block"), "unexpected stderr: {stderr}");
    assert!(
        !stderr.contains("cannot find attribute `miden_auth_script_requires_component`"),
        "unexpected stderr: {stderr}"
    );
}
