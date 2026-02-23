use super::*;

#[allow(clippy::uninlined_format_args)]
fn run_account_binding_test_with_struct(name: &str, account_struct: &str, method: &str) {
    let lib_rs = format!(
        r"#![no_std]
#![feature(alloc_error_handler)]

use miden::*;

#[component]
{account_struct}

#[component]
impl TestAccount {{
    {method}
}}
",
        account_struct = account_struct,
        method = method
    );

    let sdk_path = sdk_crate_path();
    let component_package = format!("miden:{}", name.replace('_', "-"));
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

[profile.release]
opt-level = "z"
panic = "abort"
debug = false
"#,
        name = name,
        sdk_path = sdk_path.display(),
        component_package = component_package,
    );

    let cargo_proj = project(name)
        .file("Cargo.toml", &cargo_toml)
        .file("src/lib.rs", &lib_rs)
        .build();

    let mut test = CompilerTestBuilder::rust_source_cargo_miden(
        cargo_proj.root(),
        WasmTranslationConfig::default(),
        [],
    )
    .build();

    test.compile_package();
}

#[allow(clippy::uninlined_format_args)]
fn run_account_binding_test(name: &str, method: &str) {
    run_account_binding_test_with_struct(name, "struct TestAccount;", method)
}

#[test]
fn rust_sdk_account_get_code_commitment_binding() {
    run_account_binding_test(
        "rust_sdk_account_get_code_commitment_binding",
        "pub fn binding(&self) -> Word {
        self.get_code_commitment()
    }",
    );
}

#[test]
fn rust_sdk_account_get_initial_storage_commitment_binding() {
    run_account_binding_test(
        "rust_sdk_account_get_initial_storage_commitment_binding",
        "pub fn binding(&self) -> Word {
        self.get_initial_storage_commitment()
    }",
    );
}

#[test]
fn rust_sdk_account_compute_storage_commitment_binding() {
    run_account_binding_test(
        "rust_sdk_account_compute_storage_commitment_binding",
        "pub fn binding(&self) -> Word {
        self.compute_storage_commitment()
    }",
    );
}

#[test]
fn rust_sdk_account_compute_commitment_binding() {
    run_account_binding_test(
        "rust_sdk_account_compute_commitment_binding",
        "pub fn binding(&self) -> Word {
        self.compute_commitment()
    }",
    );
}

#[test]
fn rust_sdk_account_compute_delta_commitment_binding() {
    run_account_binding_test(
        "rust_sdk_account_compute_delta_commitment_binding",
        "pub fn binding(&self) -> Word {
        self.compute_delta_commitment()
    }",
    );
}

#[test]
fn rust_sdk_account_get_initial_balance_binding() {
    run_account_binding_test(
        "rust_sdk_account_get_initial_balance_binding",
        "pub fn binding(&self) -> Felt {
        let faucet = AccountId { prefix: Felt::from_u32(1), suffix: Felt::from_u32(0) };
        self.get_initial_balance(faucet)
    }",
    );
}

#[test]
fn rust_sdk_account_has_non_fungible_asset_binding() {
    run_account_binding_test(
        "rust_sdk_account_has_non_fungible_asset_binding",
        "pub fn binding(&self) -> Felt {
        let asset = Asset::from([Felt::from_u32(0); 4]);
        if self.has_non_fungible_asset(asset) {
            Felt::from_u32(1)
        } else {
            Felt::from_u32(0)
        }
    }",
    );
}

#[test]
fn rust_sdk_account_get_initial_vault_root_binding() {
    run_account_binding_test(
        "rust_sdk_account_get_initial_vault_root_binding",
        "pub fn binding(&self) -> Word {
        self.get_initial_vault_root()
    }",
    );
}

#[test]
fn rust_sdk_account_get_vault_root_binding() {
    run_account_binding_test(
        "rust_sdk_account_get_vault_root_binding",
        "pub fn binding(&self) -> Word {
        self.get_vault_root()
    }",
    );
}

#[test]
fn rust_sdk_account_get_num_procedures_binding() {
    run_account_binding_test(
        "rust_sdk_account_get_num_procedures_binding",
        "pub fn binding(&self) -> Felt {
        self.get_num_procedures()
    }",
    );
}

#[test]
fn rust_sdk_account_get_procedure_root_binding() {
    run_account_binding_test(
        "rust_sdk_account_get_procedure_root_binding",
        "pub fn binding(&self) -> Word {
        self.get_procedure_root(0)
    }",
    );
}

#[test]
fn rust_sdk_account_has_procedure_binding() {
    run_account_binding_test(
        "rust_sdk_account_has_procedure_binding",
        "pub fn binding(&self) -> Felt {
        let proc_root = Word::from([Felt::from_u32(0); 4]);
        if self.has_procedure(proc_root) {
            Felt::from_u32(1)
        } else {
            Felt::from_u32(0)
        }
    }",
    );
}

#[test]
fn rust_sdk_account_was_procedure_called_binding() {
    run_account_binding_test(
        "rust_sdk_account_was_procedure_called_binding",
        "pub fn binding(&self) -> Felt {
        let proc_root = Word::from([Felt::from_u32(0); 4]);
        if self.was_procedure_called(proc_root) {
            Felt::from_u32(1)
        } else {
            Felt::from_u32(0)
        }
    }",
    );
}

#[test]
fn rust_sdk_account_storage_get_initial_item_binding() {
    run_account_binding_test_with_struct(
        "rust_sdk_account_storage_get_initial_item_binding",
        r#"struct TestAccount {
    #[storage(description = "test value")]
    value: Value,
}"#,
        "pub fn binding(&self) -> Word {
        storage::get_initial_item(Self::default().value.slot)
    }",
    );
}

#[test]
fn rust_sdk_account_storage_get_initial_map_item_binding() {
    run_account_binding_test_with_struct(
        "rust_sdk_account_storage_get_initial_map_item_binding",
        r#"struct TestAccount {
    #[storage(description = "test map")]
    map: StorageMap,
}"#,
        "pub fn binding(&self) -> Word {
        let key = Word::from([Felt::from_u32(0); 4]);
        storage::get_initial_map_item(Self::default().map.slot, &key)
    }",
    );
}
