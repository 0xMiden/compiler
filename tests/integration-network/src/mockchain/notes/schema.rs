//! Schema-driven note storage tests on the mock chain.

use std::{process::Command, sync::Arc};

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
    if !wasm_target_is_installed() {
        eprintln!("skipping DEX note schema test: wasm32-unknown-unknown is not installed");
        return;
    }
    let note_package = compile_rust_package("../../examples/dex-note", true);
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

/// Returns true when rustup reports the codec component target as installed.
fn wasm_target_is_installed() -> bool {
    let output = match Command::new("rustup").args(["target", "list"]).output() {
        Ok(output) if output.status.success() => output,
        Ok(output) => {
            eprintln!("`rustup target list` failed:\n{}", String::from_utf8_lossy(&output.stderr));
            return false;
        }
        Err(error) => {
            eprintln!("could not run `rustup target list`: {error}");
            return false;
        }
    };
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .any(|line| line.starts_with("wasm32-unknown-unknown") && line.contains("(installed)"))
}

/// Asserts that a package carries one named custom section.
fn assert_package_section(package: &Package, name: &str) {
    let id = SectionId::custom(name).unwrap();
    assert!(
        package.sections.iter().any(|section| section.id == id),
        "package does not contain the `{name}` section"
    );
}
