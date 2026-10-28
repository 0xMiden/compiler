//! End-to-end test for an author codec bundled with an account component package.

#![cfg(feature = "codec-component")]

use std::{env, fs, path::Path, process::Command};

use miden_account_storage_schema::{
    AccountStorageSchema, CODEC_GUEST_RUSTFLAGS, CodecRegistry, Felt, Word, felts_from_word,
};
use miden_mast_package::{Section, SectionId};
use miden_protocol::{
    account::{
        AccountComponentMetadata, StorageSlotName,
        component::{SchemaType, StorageSchema, StorageSlotSchema, WordValue},
    },
    utils::serde::Serializable,
};
use midenc_frontend_wasm_metadata::{
    package_account_storage_schema_section_id, package_codec_section_id,
};
use midenc_integration_test_support::{
    compile_project, scrub_nested_cargo_env, wasm_target_is_installed, workspace_root,
};

const WASM_TARGET: &str = "wasm32-wasip2";
const RATIO_FQN: &str = "example:codec-storage-schema/account-storage.ratio";

/// An account storage schema with a custom value type and a custom map value type.
const STORAGE_SCHEMA: &str = r#"
package example:codec-storage-schema;

use miden:base/core-types@1.0.0;

interface account-storage {
    use core-types.{word};

    record ratio {
        numerator: u32,
        denominator: u32,
    }

    record storage {
        price: ratio,
        prices: list<tuple<word, ratio>>,
    }
}

package miden:base@1.0.0 {
    interface core-types {
        record felt { inner: f32 }
        record word { a: felt, b: felt, c: felt, d: felt }
    }
}
"#;

/// Returns protocol metadata that declares the slots of [`STORAGE_SCHEMA`].
fn metadata() -> AccountComponentMetadata {
    let slot =
        |leaf: &str| StorageSlotName::new(format!("example::codec_storage::{leaf}")).unwrap();
    let word = SchemaType::native_word;
    AccountComponentMetadata::new("codec-storage").with_storage_schema(
        StorageSchema::new([
            (slot("price"), StorageSlotSchema::value("price", word())),
            (slot("prices"), StorageSlotSchema::map("prices", word(), word())),
        ])
        .unwrap(),
    )
}

/// Builds the codec component of [`STORAGE_SCHEMA`].
fn build_codec_component() -> Vec<u8> {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path();
    fs::create_dir(root.join("src")).unwrap();
    // Seed the workspace lockfile so the offline build resolves to the versions that the
    // workspace already downloaded instead of racing the registry index.
    fs::copy(workspace_root().join("Cargo.lock"), root.join("Cargo.lock")).unwrap();
    let codec_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../codec");
    fs::write(
        root.join("Cargo.toml"),
        format!(
            r#"[package]
name = "storage-codec-fixture"
version = "0.0.0"
edition = "2024"

[lib]
crate-type = ["cdylib"]

[dependencies]
miden-codec = {{ path = {codec_path:?} }}

[workspace]
"#
        ),
    )
    .unwrap();
    fs::write(
        root.join("src/lib.rs"),
        [
            "use miden_codec::AuthorTypeCodec;\n\nmiden_codec::from_wit_text!(r#\"",
            STORAGE_SCHEMA,
            r##""#);

#[miden_codec::codec]
impl AuthorTypeCodec for Ratio {
    fn parse(value: &str) -> Result<Self, String> {
        let (numerator, denominator) = value
            .split_once('/')
            .ok_or_else(|| "a ratio must use `numerator/denominator`".to_owned())?;
        Ok(Self {
            numerator: numerator.parse::<u32>().map_err(|error| error.to_string())?,
            denominator: denominator.parse::<u32>().map_err(|error| error.to_string())?,
        })
    }

    fn display(&self) -> String {
        format!("{}/{}", self.numerator, self.denominator)
    }

    fn validate(&self) -> Result<(), String> {
        if self.denominator == 0 {
            Err("the denominator must not be zero".to_owned())
        } else {
            Ok(())
        }
    }
}

miden_codec::export_codecs!();
"##,
        ]
        .concat(),
    )
    .unwrap();

    let target_dir = workspace_root().join("target/storage-codec-component-test");
    let mut command = Command::new(env::var_os("CARGO").unwrap_or_else(|| "cargo".into()));
    command
        .args(["build", "--release", "--offline", "--target", WASM_TARGET, "--manifest-path"])
        .arg(root.join("Cargo.toml"))
        .env("CARGO_TARGET_DIR", &target_dir)
        // Share the hash-keyed intermediates with the other test builds; only the
        // name-keyed final artifacts stay in the directory above. Convention from
        // `tests/support`.
        .env("CARGO_BUILD_BUILD_DIR", workspace_root().join("target/miden_build_cache"));
    scrub_nested_cargo_env(&mut command);
    // Pin the guest rustflags after the scrub, the way the compiler builds a codec crate.
    command.env("RUSTFLAGS", CODEC_GUEST_RUSTFLAGS);
    let output = command.output().expect("failed to start the codec fixture build");
    assert!(
        output.status.success(),
        "building the codec fixture failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    fs::read(target_dir.join(format!("{WASM_TARGET}/release/storage_codec_fixture.wasm")))
        .expect("the codec fixture did not produce its Wasm component")
}

#[test]
fn bundled_codec_parses_and_displays_custom_slot_values() {
    if !wasm_target_is_installed() {
        eprintln!("skipping the storage codec test: {WASM_TARGET} is not installed");
        return;
    }

    // Any account component package carries the sections the test replaces.
    let mut package =
        (*compile_project(&workspace_root().join("examples/storage-example"))).clone();
    let schema_id = package_account_storage_schema_section_id();
    package.sections.retain(|section| {
        section.id != schema_id && section.id != SectionId::ACCOUNT_COMPONENT_METADATA
    });
    package
        .sections
        .push(Section::new(schema_id, STORAGE_SCHEMA.as_bytes().to_vec()));
    package
        .sections
        .push(Section::new(SectionId::ACCOUNT_COMPONENT_METADATA, metadata().to_bytes()));
    package
        .sections
        .push(Section::new(package_codec_section_id(), build_codec_component()));

    let schema = AccountStorageSchema::from_package(&package).unwrap();
    assert!(schema.custom_type_fqns().contains(RATIO_FQN));
    let codecs = CodecRegistry::load_from_package(&package, &schema).unwrap();
    assert!(codecs.contains(RATIO_FQN));
    let schema = schema.with_codec_registry(codecs);

    let key = Word::new([Felt::from_u32(1), Felt::ZERO, Felt::ZERO, Felt::ZERO]);
    let data = schema
        .init_builder()
        .set("price", "3/2")
        .unwrap()
        .set_entry("prices", &key.to_hex(), "5/4")
        .unwrap()
        .build()
        .unwrap();
    let price_slot = StorageSlotName::new("example::codec_storage::price").unwrap();
    let Some(WordValue::FullyTyped(price)) = data.slot_value_entry(&price_slot) else {
        panic!("the price slot must have a literal word");
    };
    let three = Felt::from_u32(3);
    assert_eq!(felts_from_word(*price), [three, Felt::from_u32(2), Felt::ZERO, Felt::ZERO]);
    assert_eq!(schema.decode_value("price", *price).unwrap().to_string(), "3/2");

    let prices_slot = StorageSlotName::new("example::codec_storage::prices").unwrap();
    let [(_, WordValue::FullyTyped(value))] = data.map_entries(&prices_slot).unwrap().as_slice()
    else {
        panic!("the prices slot must have one entry with literal words");
    };
    let (_, decoded) = schema.decode_map_entry("prices", key, *value).unwrap();
    assert_eq!(decoded.to_string(), "5/4");

    // The codec validates text values and decoded words alike.
    let error = schema.init_builder().set("price", "3/0").err().unwrap().to_string();
    assert!(error.contains("denominator"), "{error}");
    let zero_denominator = Word::new([three, Felt::ZERO, Felt::ZERO, Felt::ZERO]);
    let error = schema.decode_value("price", zero_denominator).unwrap_err().to_string();
    assert!(error.contains("denominator"), "{error}");
}
