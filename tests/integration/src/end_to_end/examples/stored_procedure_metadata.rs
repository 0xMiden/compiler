//! Account-component metadata of the stored-procedure example.
//!
//! Pins the storage schema the `stored-procedure-example` package carries, so a change of the
//! schema type a `StorageValue<StoredProcedure<..>>` slot maps to is visible in the metadata a
//! host reads.
//!
//! The example has two procedure slots of different signatures. The signature is a guest-side
//! type, so both slots must render as one and the same schema type.

use std::borrow::Borrow;

use miden_core::serde::Deserializable;
use miden_mast_package::SectionId;
use miden_protocol::account::AccountComponentMetadata;
use midenc_expect_test::expect;
use midenc_frontend_wasm::WasmTranslationConfig;

use crate::CompilerTest;

/// Asserts that a `StorageValue<StoredProcedure<..>>` slot renders as a word-typed slot.
#[test]
fn stored_procedure_example() {
    let config = WasmTranslationConfig::default();
    let mut test = CompilerTest::rust_source_cargo_miden(
        "../../examples/stored-procedure-example",
        config,
        [],
    );

    let package = test.compile_package();
    let account_component_metadata_bytes = package
        .as_ref()
        .sections
        .iter()
        .find_map(|s| {
            if s.id == SectionId::ACCOUNT_COMPONENT_METADATA {
                Some(s.data.borrow())
            } else {
                None
            }
        })
        .unwrap();
    let toml = AccountComponentMetadata::read_from_bytes(account_component_metadata_bytes)
        .unwrap()
        .to_toml()
        .unwrap();
    // TODO(i1352 phase 2): expect miden::protocol::stored_procedure
    expect![[r#"
        name = "stored-procedure-example"
        description = "A Miden account component that dispatches through a stored procedure root"
        version = "0.1.0"

        [[storage.slots]]
        name = "stored_procedure_example::stored_procedure_example::handler"
        description = "root of the procedure dispatch calls"
        type = "word"

        [[storage.slots]]
        name = "stored_procedure_example::stored_procedure_example::value"
        description = "value get_value returns"
        type = "felt"

        [[storage.slots]]
        name = "stored_procedure_example::stored_procedure_example::weighted_handler"
        description = "root of the procedure dispatch_weighted calls"
        type = "word"
    "#]]
    .assert_eq(&toml);
}
