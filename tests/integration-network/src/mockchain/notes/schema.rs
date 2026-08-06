//! Schema-driven note storage tests on the mock chain.

use std::{
    env,
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::Arc,
};

use miden_client::{
    account::{AccountComponent, component::InitStorageData},
    asset::{Asset, FungibleAsset},
    transaction::RawOutputNote,
};
use miden_mast_package::{Package, SectionId};
use miden_note_schema::{CodecRegistry, NoteStorage, NoteStorageSchema};
use miden_protocol::{
    account::{AccountId, auth::AuthScheme},
    address::NetworkId,
    crypto::rand::RandomCoin,
    note::Note,
};
use miden_standards::testing::note::NoteBuilder;
use miden_testing::{Auth, MockChain};
use midenc_frontend_wasm_metadata::{
    PACKAGE_NOTE_CODEC_SECTION_ID, PACKAGE_NOTE_STORAGE_SCHEMA_SECTION_ID,
};

use super::super::support::{
    assert_account_has_fungible_asset, build_send_notes_script, compile_rust_package, execute_tx,
    note_script_root,
};

/// Builds and consumes a note, then returns the consumed note and account ID.
fn transfer_with_storage(
    note_package: Arc<Package>,
    build_storage: impl FnOnce(AccountId) -> NoteStorage,
) -> (Note, AccountId) {
    let wallet_package = compile_rust_package("../../examples/basic-wallet", true);
    let wallet_component =
        AccountComponent::from_package(&wallet_package, &InitStorageData::default()).unwrap();

    let mut builder = MockChain::builder();
    let faucet = builder
        .add_existing_basic_faucet(
            Auth::BasicAuth {
                auth_scheme: AuthScheme::Falcon512Poseidon2,
            },
            "TEST",
            1_000_000,
            None,
        )
        .unwrap();
    let faucet_id = faucet.id();
    let recipient = builder
        .add_existing_account_from_components(
            Auth::BasicAuth {
                auth_scheme: AuthScheme::Falcon512Poseidon2,
            },
            [wallet_component],
        )
        .unwrap();
    let recipient_id = recipient.id();

    let mut chain = builder.build().unwrap();
    chain.prove_next_block().unwrap();
    chain.prove_next_block().unwrap();

    let transfer_amount = 100_000;
    let asset = FungibleAsset::new(faucet_id, transfer_amount).unwrap();
    let mut note_rng = RandomCoin::new(note_script_root(&note_package));
    let note = NoteBuilder::new(faucet_id, &mut note_rng)
        .package((*note_package).clone())
        .add_assets([Asset::from(asset)])
        .note_storage(build_storage(recipient_id).to_elements())
        .unwrap()
        .build()
        .unwrap();

    let faucet = chain.committed_account(faucet_id).unwrap().clone();
    let send_script = build_send_notes_script(&faucet, std::slice::from_ref(&note));
    let send = chain
        .build_tx_context(faucet_id, &[], &[])
        .unwrap()
        .tx_script(send_script.into())
        .extend_expected_output_notes(vec![RawOutputNote::Full(note.clone())]);
    execute_tx(&mut chain, send);

    let consume = chain
        .build_tx_context(recipient_id, &[note.id()], &[])
        .unwrap()
        .foreign_accounts(vec![chain.get_foreign_account_inputs(faucet_id).unwrap()]);
    execute_tx(&mut chain, consume);

    assert_account_has_fungible_asset(
        chain.committed_account(recipient_id).unwrap(),
        faucet_id,
        transfer_amount,
    );
    (note, recipient_id)
}

#[test]
fn dex_note_uses_embedded_schema_and_component_codec() {
    let note_package = build_dex_note_package();
    assert_package_section(&note_package, PACKAGE_NOTE_STORAGE_SCHEMA_SECTION_ID);
    assert_package_section(&note_package, PACKAGE_NOTE_CODEC_SECTION_ID);
    let schema = NoteStorageSchema::from_package(&note_package).unwrap();
    let codecs = CodecRegistry::load_from_package(&note_package).unwrap();

    let (note, recipient_id) = transfer_with_storage(Arc::clone(&note_package), |recipient_id| {
        schema
            .builder_with_registry(&codecs)
            .set("target", recipient_id.to_bech32(NetworkId::Mainnet))
            .unwrap()
            .set("price", "1.5")
            .unwrap()
            .build()
            .unwrap()
    });

    let decoded = schema.decode_with_registry(note.storage(), &codecs).unwrap();
    assert_eq!(
        decoded.field("target").unwrap().to_string(),
        recipient_id.to_bech32(NetworkId::Mainnet)
    );
    assert_eq!(decoded.field("price").unwrap().to_string(), "1.5");
}

#[test]
fn p2id_note_builds_storage_without_a_component_codec() {
    let note_package = compile_rust_package("../../examples/p2id-note", true);
    let schema = NoteStorageSchema::from_package(&note_package).unwrap();

    let (note, recipient_id) = transfer_with_storage(note_package, |recipient_id| {
        schema
            .builder()
            .set("target-account-id", recipient_id.to_bech32(NetworkId::Mainnet))
            .unwrap()
            .build()
            .unwrap()
    });

    let decoded = schema.decode(note.storage()).unwrap();
    assert_eq!(
        decoded.field("target_account_id").unwrap().to_string(),
        recipient_id.to_bech32(NetworkId::Mainnet)
    );
}

/// Runs cargo-miden so the DEX package receives its codec section.
fn build_dex_note_package() -> Arc<Package> {
    let root = workspace_root();
    let project = root.join("examples/dex-note");
    let binary = cargo_miden_binary(&root);
    let output = Command::new(binary)
        .args(["miden", "build", "--release"])
        .current_dir(&project)
        .output()
        .expect("failed to start cargo miden for dex-note");
    assert_command_succeeded("cargo miden build for dex-note", &output);

    Arc::new(
        Package::deserialize_from_file(project.join("target/miden/release/dex-note.masp"))
            .expect("failed to read the cargo-miden DEX package"),
    )
}

/// Returns the cargo-miden binary built by the workspace test workflow.
fn cargo_miden_binary(root: &Path) -> PathBuf {
    let candidates = [root.join("target/debug/cargo-miden"), root.join("bin/cargo-miden")];
    candidates.into_iter().find(|candidate| candidate.is_file()).unwrap_or_else(|| {
        panic!("cargo-miden is not built; run `cargo build -p cargo-miden` before this test")
    })
}

/// Returns the compiler workspace root.
fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("integration-network must be under tests/")
        .to_owned()
}

/// Asserts that a package carries one named custom section.
fn assert_package_section(package: &Package, name: &str) {
    let id = SectionId::custom(name).unwrap();
    assert!(
        package.sections.iter().any(|section| section.id == id),
        "package does not contain the `{name}` section"
    );
}

/// Includes both output streams when a child process fails.
fn assert_command_succeeded(action: &str, output: &Output) {
    assert!(
        output.status.success(),
        "{action} failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}
