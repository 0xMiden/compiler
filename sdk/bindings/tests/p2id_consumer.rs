//! End-to-end test for package discovery and generated p2id consumer bindings.

mod common;

use std::fs;

use miden_mast_package::Section;
use midenc_frontend_wasm_metadata::package_note_storage_schema_section_id;
use midenc_integration_test_support::{compile_project, workspace_root, write_masp_file_atomic};

#[test]
fn generated_p2id_bindings_compile_and_run_in_a_consumer_crate() {
    let workspace = workspace_root();
    let examples = workspace.join("examples");
    let wallet_dir = examples.join("basic-wallet");
    let wallet = compile_project(&wallet_dir);
    write_masp_file_atomic(&wallet, wallet_dir.join("target/miden/release"))
        .expect("failed to persist the basic-wallet dependency package");

    let p2id_dir = examples.join("p2id-note");
    let p2id = compile_project(&p2id_dir);
    let package_dir = p2id_dir.join("target/miden/release");
    write_masp_file_atomic(&p2id, &package_dir).expect("failed to persist the p2id package");
    let package_path = package_dir.join("p2id.masp");

    let temp = tempfile::tempdir().unwrap();
    let second_package_dir = temp.path().join("packages/counter");
    fs::create_dir_all(&second_package_dir).unwrap();
    let mut second_package = (*p2id).clone();
    let schema_id = package_note_storage_schema_section_id();
    second_package.sections.retain(|section| section.id != schema_id);
    second_package.sections.push(Section::new(
        schema_id,
        br#"package example:counter-schema;

interface note-storage {
    record counter-note { value: u64 }
    type storage = counter-note;
}
"#
        .to_vec(),
    ));
    write_masp_file_atomic(&second_package, &second_package_dir)
        .expect("failed to persist the second schema package");
    let second_package_path = second_package_dir.join("p2id.masp");

    let source = format!(
        r#"use std::collections::BTreeMap;

use miden_bindings::{{account::AccountId, address::NetworkId}};

mod project_bindings {{
    miden_bindings::from_project!({project_dir:?});
}}

mod package_bindings {{
    miden_bindings::from_package!({package_path:?});
    miden_bindings::from_package!({second_package_path:?});
}}

fn main() {{
    let account_id =
        AccountId::try_from(0xaa00_0000_0000_bc11_0000_bc00_0000_de00u128).unwrap();
    let bech32 = account_id.to_bech32(NetworkId::Mainnet);
    let mut values = BTreeMap::new();
    values.insert("target_account_id".to_owned(), bech32.clone());

    let typed = project_bindings::P2idNote::from_str_values(&values).unwrap();
    assert_eq!(typed.target_account_id, account_id);
    let storage = typed.to_note_storage().unwrap();
    assert_eq!(
        storage.items(),
        &[account_id.prefix().as_felt(), account_id.suffix()],
    );
    typed.validate().unwrap();
    assert_eq!(
        typed.display().unwrap(),
        format!("{{{{target-account-id: {{bech32}}}}}}"),
    );

    let decoded = project_bindings::P2idNote::from_note_storage(&storage).unwrap();
    assert_eq!(decoded, typed);

    let exact = package_bindings::P2idNote::from_note_storage(&storage).unwrap();
    assert_eq!(exact.target_account_id, account_id);

    let counter = package_bindings::CounterNote {{ value: 9 }};
    let counter_storage = counter.to_note_storage().unwrap();
    assert_eq!(counter_storage.items()[0].as_canonical_u64(), 9);
    assert_eq!(counter_storage.items()[1].as_canonical_u64(), 0);
}}
"#,
        project_dir = p2id_dir.to_string_lossy(),
        package_path = package_path.to_string_lossy(),
        second_package_path = second_package_path.to_string_lossy(),
    );
    common::run_consumer(temp.path(), "bindings-consumer", &source);
}
