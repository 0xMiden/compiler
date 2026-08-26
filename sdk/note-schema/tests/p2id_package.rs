//! End-to-end test for a schema embedded in the p2id note package.

use std::{
    fs::{self, File},
    path::{Path, PathBuf},
    sync::Arc,
};

use miden_mast_package::Package;
use miden_note_schema::{NoteStorage, NoteStorageSchema};
use miden_protocol::{account::AccountId, address::NetworkId};
use midenc_frontend_wasm::WasmTranslationConfig;
use midenc_integration_test_support::CompilerTest;

/// Compiles one Cargo Miden project without debug output.
fn compile_project(project_path: &Path) -> Arc<Package> {
    let mut test = CompilerTest::rust_source_cargo_miden(
        project_path,
        WasmTranslationConfig::default(),
        ["--debug".to_owned(), "none".to_owned()],
    );
    test.compile_package()
}

/// Returns the compiler workspace root.
fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap()
}

/// Locks the shared p2id example outputs for the full build and consume span.
fn p2id_build_lock(workspace: &Path) -> File {
    let target_dir = workspace.join("target");
    fs::create_dir_all(&target_dir).expect("failed to create the workspace target directory");
    let lock = File::options()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(target_dir.join("p2id-end-to-end-build.lock"))
        .expect("failed to open the p2id end-to-end build lock");
    lock.lock().expect("failed to lock the p2id end-to-end build");
    lock
}

#[test]
fn p2id_schema_builds_and_decodes_account_id_storage() {
    let workspace = workspace_root();
    let _build_lock = p2id_build_lock(&workspace);
    let examples = workspace.join("examples");
    let wallet_dir = examples.join("basic-wallet");
    let wallet = compile_project(&wallet_dir);
    wallet
        .write_masp_file(wallet_dir.join("target/miden/release"))
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
