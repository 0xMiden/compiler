//! End-to-end test for a schema embedded in the p2id note package.

use miden_note_schema::{NoteStorage, NoteStorageSchema};
use miden_protocol::{account::AccountId, address::NetworkId};
use midenc_integration_test_support::{compile_project, workspace_root, write_masp_file_atomic};

#[test]
fn p2id_schema_builds_and_decodes_account_id_storage() {
    let workspace = workspace_root();
    let examples = workspace.join("examples");
    let wallet_dir = examples.join("basic-wallet");
    let wallet = compile_project(&wallet_dir);
    write_masp_file_atomic(&wallet, wallet_dir.join("target/miden/release"))
        .expect("failed to persist the basic-wallet dependency package");

    let p2id = compile_project(&examples.join("p2id-note"));
    let schema = NoteStorageSchema::from_package(&p2id).expect("p2id schema must resolve");
    let account_id = AccountId::try_from(0xaa00_0000_0000_bc11_0000_bc00_0000_de00u128).unwrap();
    let bech32 = account_id.to_bech32(NetworkId::Mainnet);

    let built = schema.builder().set("target-account-id", &bech32).unwrap().build().unwrap();
    let expected =
        NoteStorage::new(vec![account_id.prefix().as_felt(), account_id.suffix()]).unwrap();
    assert_eq!(built, expected);

    let decoded = schema.decode(&built).unwrap();
    assert_eq!(decoded.field("target_account_id").unwrap().to_string(), bech32);
}
