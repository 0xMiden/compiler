//! Account storage schema tests on the mock chain.
//!
//! The storage-example component bundles a codec for its custom slot types: its init storage
//! data is built from text through that codec, and the committed account's slot words decode back
//! to the same text. A generated component whose slots a note writes from guest code shows that
//! the guest's slot words decode on the host.

use miden_account_storage_schema::{AccountStorageSchema, CodecRegistry};
use miden_client::{account::AccountComponent, transaction::RawOutputNote};
use miden_mast_package::{Package, SectionId};
use miden_protocol::{
    Felt, Word,
    account::{StorageMapKey, StorageSlotName},
    crypto::rand::RandomCoin,
};
use miden_standards::testing::note::NoteBuilder;
use miden_testing::MockChain;
use midenc_frontend_wasm_metadata::{
    PACKAGE_ACCOUNT_STORAGE_SCHEMA_SECTION_ID, PACKAGE_CODEC_SECTION_ID,
    package_account_storage_schema_section_id, package_codec_section_id,
};
use midenc_integration_test_support::{
    compiler_test::sdk_crate_path, project, wasm_target_is_installed,
};

use super::support::{
    account_cargo_toml_for, account_miden_project_toml_with_interface,
    build_existing_counter_account_builder_with_auth_package, compile_rust_package, execute_tx,
    note_cargo_toml_for_dependency, note_miden_project_toml_for_dependency, note_script_root,
};

/// Project name of the generated account component.
const LIMITS_ACCOUNT_NAME: &str = "storage-limits-account";
/// Miden package name of the generated account component.
const LIMITS_ACCOUNT_PACKAGE: &str = "miden:storage-limits-account";

/// Account component with a custom-typed value slot and a custom-typed map value.
const LIMITS_ACCOUNT_SOURCE: &str = r#"
#![no_std]
#![feature(alloc_error_handler)]

use miden::{AssetAmount, StorageMap, StorageValue, Word, component, component_storage, export_type};
use miden_field_repr::{FromFeltRepr, ToFeltRepr};

/// Limits written by the note.
#[export_type]
#[derive(Clone, Copy, FromFeltRepr, ToFeltRepr)]
pub struct Limits {
    pub max_qty: AssetAmount,
    pub paused: bool,
}

#[component_storage]
struct LimitsStorage {
    #[storage(description = "limits")]
    limits: StorageValue<Limits>,
    #[storage(description = "limits by asset")]
    holdings: StorageMap<Word, Limits>,
}

/// API of the limits account component.
#[component]
trait LimitsAccount {
    /// Replaces the stored limits.
    #[account_procedure]
    fn set_limits(&mut self, limits: Limits);
    /// Stores the limits of one asset.
    #[account_procedure]
    fn set_holding(&mut self, asset: Word, limits: Limits);
}

#[component]
impl LimitsAccount for LimitsStorage {
    fn set_limits(&mut self, limits: Limits) {
        self.limits.set(limits);
    }

    fn set_holding(&mut self, asset: Word, limits: Limits) {
        self.holdings.set(asset, limits);
    }
}
"#;

/// Note script writing constant limits through the account's procedures.
const LIMITS_NOTE_SOURCE: &str = r#"
#![no_std]
#![feature(alloc_error_handler)]

use miden::{AssetAmount, Word, account, felt, note};

use crate::bindings::miden::storage_limits_account::limits_account::Limits;

/// Native account of the note: exposes the limits account component methods.
#[account(storage_limits_account::LimitsAccount)]
pub struct Account;

/// A note that writes `{ max_qty: 7, paused: true }` to both slots of the consuming account.
#[note]
struct LimitsNote;

#[note]
impl LimitsNote {
    #[note_script]
    pub fn run(self, _arg: Word, account: &mut Account) {
        account.set_limits(limits());
        account.set_holding(Word::new([felt!(1), felt!(2), felt!(3), felt!(4)]), limits());
    }
}

/// Returns the limits the note writes.
fn limits() -> Limits {
    Limits { max_qty: AssetAmount::from(7u32), paused: true }
}
"#;

/// Builds the storage-example account from text values and decodes its committed slots.
#[test]
fn storage_example_slots_round_trip_through_the_bundled_codec() {
    if !wasm_target_is_installed() {
        eprintln!("skipping storage-example schema test: wasm32-wasip2 is not installed");
        return;
    }
    let package = compile_rust_package("../../examples/storage-example", true);
    let no_auth = compile_rust_package("../../examples/auth-component-no-auth", true);
    assert_package_section(
        &package,
        package_account_storage_schema_section_id(),
        PACKAGE_ACCOUNT_STORAGE_SCHEMA_SECTION_ID,
    );
    assert_package_section(&package, package_codec_section_id(), PACKAGE_CODEC_SECTION_ID);
    let schema = AccountStorageSchema::from_package(&package).unwrap();
    let codecs = CodecRegistry::load_from_package(&package, &schema).unwrap();

    let owner = word([1, 2, 3, 4]);
    let asset = word([5, 6, 7, 8]);
    let data = schema
        .init_builder_with_registry(&codecs)
        .set("owner_public_key", owner.to_hex())
        .unwrap()
        .set("limits", "100 paused")
        .unwrap()
        .set_entry("holdings", &asset.to_hex(), "7 locked")
        .unwrap()
        .build()
        .unwrap();
    let component = AccountComponent::from_package(package.as_ref().clone(), &data).unwrap();
    let account = build_existing_counter_account_builder_with_auth_package(
        component,
        no_auth,
        vec![],
        [3_u8; 32],
    )
    .build_existing()
    .unwrap();

    let mut builder = MockChain::builder();
    builder.add_account(account.clone()).unwrap();
    let chain = builder.build().unwrap();
    let storage = chain.committed_account(account.id()).unwrap().storage();

    let slot_name =
        |leaf: &str| StorageSlotName::new(schema.slot(leaf).unwrap().name().unwrap()).unwrap();
    let value = |leaf: &str| {
        let word = storage.get_item(&slot_name(leaf)).unwrap();
        schema.decode_value_with_registry(leaf, word, &codecs).unwrap().to_string()
    };
    assert_eq!(value("owner_public_key"), owner.to_hex());
    assert_eq!(value("limits"), "100 paused");

    let stored = storage
        .get_map_item(&slot_name("holdings"), StorageMapKey::from_raw(asset))
        .unwrap();
    let (key, holding) = schema
        .decode_map_entry_with_registry("holdings", asset, stored, &codecs)
        .unwrap();
    assert_eq!(key.to_string(), asset.to_hex());
    assert_eq!(holding.to_string(), "7 locked");
}

/// Consumes a note whose script writes both custom-typed slots of the account from guest code,
/// then decodes the committed slot words on the host.
#[test]
fn guest_written_slots_decode_on_the_host() {
    let account_project = project(LIMITS_ACCOUNT_NAME)
        .file(
            "miden-project.toml",
            &account_miden_project_toml_with_interface(
                LIMITS_ACCOUNT_NAME,
                LIMITS_ACCOUNT_PACKAGE,
                "limits-account",
            ),
        )
        .file("Cargo.toml", &account_cargo_toml_with_felt_repr())
        .file("src/lib.rs", LIMITS_ACCOUNT_SOURCE)
        .build();
    let account_package = compile_rust_package(account_project.root(), true);
    let note_name = "storage-limits-note";
    let note_project = project(note_name)
        .file(
            "miden-project.toml",
            &note_miden_project_toml_for_dependency(
                note_name,
                "miden:storage-limits-note",
                LIMITS_ACCOUNT_PACKAGE,
                account_project.root(),
            ),
        )
        .file(
            "Cargo.toml",
            &note_cargo_toml_for_dependency(
                note_name,
                LIMITS_ACCOUNT_PACKAGE,
                account_project.root(),
            ),
        )
        .file("src/lib.rs", LIMITS_NOTE_SOURCE)
        .build();
    let note_package = compile_rust_package(note_project.root(), true);
    let no_auth = compile_rust_package("../../examples/auth-component-no-auth", true);

    let schema = AccountStorageSchema::from_package(&account_package).unwrap();
    let data = schema
        .init_builder()
        .set("limits.max_qty", "1")
        .unwrap()
        .set("limits.paused", "false")
        .unwrap()
        .build()
        .unwrap();
    let component =
        AccountComponent::from_package(account_package.as_ref().clone(), &data).unwrap();
    let account = build_existing_counter_account_builder_with_auth_package(
        component,
        no_auth,
        vec![],
        [4_u8; 32],
    )
    .build_existing()
    .unwrap();

    let mut builder = MockChain::builder();
    builder.add_account(account.clone()).unwrap();
    let mut note_rng = RandomCoin::new(note_script_root(&note_package));
    let note = NoteBuilder::new(account.id(), &mut note_rng)
        .package((*note_package).clone())
        .build()
        .unwrap();
    builder.add_output_note(RawOutputNote::Full(note.clone()));
    let mut chain = builder.build().unwrap();
    chain.prove_next_block().unwrap();

    let slot_name =
        |leaf: &str| StorageSlotName::new(schema.slot(leaf).unwrap().name().unwrap()).unwrap();
    let limits = |chain: &MockChain| {
        let storage = chain.committed_account(account.id()).unwrap().storage();
        storage.get_item(&slot_name("limits")).unwrap()
    };
    let initial = limits(&chain);
    assert_eq!(
        schema.decode_value("limits", initial).unwrap().to_string(),
        "{max-qty: 1, paused: false}"
    );

    let tx = chain
        .build_transaction(account.id())
        .authenticated_input_note(note.id())
        .build()
        .unwrap();
    execute_tx(&mut chain, tx);

    let written = limits(&chain);
    // The guest pads the two-felt value with zeros to one word.
    assert_eq!(written, word([7, 1, 0, 0]));
    assert_eq!(
        schema.decode_value("limits", written).unwrap().to_string(),
        "{max-qty: 7, paused: true}"
    );

    let asset = word([1, 2, 3, 4]);
    let storage = chain.committed_account(account.id()).unwrap().storage();
    let stored = storage
        .get_map_item(&slot_name("holdings"), StorageMapKey::from_raw(asset))
        .unwrap();
    let (key, value) = schema.decode_map_entry("holdings", asset, stored).unwrap();
    assert_eq!(key.to_string(), asset.to_hex());
    assert_eq!(value.to_string(), "{max-qty: 7, paused: true}");
}

/// Returns the generated account `Cargo.toml` with the felt-repr derive crate added.
fn account_cargo_toml_with_felt_repr() -> String {
    let felt_repr = sdk_crate_path().join("../field-repr/repr");
    account_cargo_toml_for(LIMITS_ACCOUNT_NAME, LIMITS_ACCOUNT_PACKAGE).replacen(
        "[dependencies]\n",
        &format!("[dependencies]\nmiden-field-repr = {{ path = \"{}\" }}\n", felt_repr.display()),
        1,
    )
}

/// Returns a word from small integers.
fn word(values: [u32; 4]) -> Word {
    Word::new(values.map(Felt::from_u32))
}

/// Asserts that a package carries one named custom section.
fn assert_package_section(package: &Package, id: SectionId, name: &str) {
    assert!(
        package.sections.iter().any(|section| section.id == id),
        "package does not contain the `{name}` section"
    );
}
