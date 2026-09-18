use super::*;

#[allow(clippy::uninlined_format_args)]
fn run_note_binding_test(name: &str, method: &str) {
    let component = account_component_source("TestNote", method);
    let lib_rs = format!(
        r"#![no_std]
#![feature(alloc_error_handler)]

extern crate alloc;

use miden::*;

{component}
"
    );

    let sdk_path = sdk_crate_path();
    let namespace = account_component_namespace(name, "test-note");
    let miden_project_toml = format!(
        r#"
[package]
name = "{name}"
version = "0.0.1"

[lib]
kind = "account"
namespace = "{namespace}"
path = "src/lib.rs"

[package.metadata.miden]
supported-types = ["RegularAccountUpdatableCode"]
"#
    );
    let cargo_toml = format!(
        r#"
cargo-features = ["trim-paths"]

[package]
name = "{name}"
version = "0.0.1"
edition = "2024"
authors = []

[lib]
crate-type = ["cdylib"]

[dependencies]
miden = {{ path = "{sdk_path}" }}

[profile.release]
trim-paths = ["diagnostics", "object"]

[profile.dev]
trim-paths = ["diagnostics", "object"]
"#,
        name = name,
        sdk_path = sdk_path.display(),
    );

    let cargo_proj = project(name)
        .file("miden-project.toml", &miden_project_toml)
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

#[test]
fn note_compute_and_store_recipient_binding() {
    run_note_binding_test(
        "note_compute_and_store_recipient_binding",
        "pub fn binding(&self) -> Recipient {
        note::compute_and_store_recipient(
            Word::from([Felt::new(0).unwrap(); 4]),
            Word::from([Felt::new(0).unwrap(); 4]),
            alloc::vec![Felt::new(0).unwrap(); 4],
        )
    }",
    );
}

#[test]
fn note_build_recipient_binding() {
    run_note_binding_test(
        "note_build_recipient_binding",
        "pub fn binding(&self) -> Recipient {
        note::build_recipient(
            Word::from([Felt::new(0).unwrap(); 4]),
            Word::from([Felt::new(0).unwrap(); 4]),
            alloc::vec![Felt::new(0).unwrap(); 4],
        )
    }",
    );
}

#[test]
fn note_compute_storage_commitment_binding() {
    run_note_binding_test(
        "note_compute_storage_commitment_binding",
        "pub fn binding(&self) -> Word {
        let storage = alloc::vec![Felt::new(0).unwrap(); 4];
        note::compute_storage_commitment(&storage)
    }",
    );
}

#[test]
fn note_write_attachment_commitments_to_memory_binding() {
    run_note_binding_test(
        "note_write_attachment_commitments_to_memory_binding",
        "pub fn binding(&self) -> Felt {
        let commitments = note::write_attachment_commitments_to_memory(
            Word::from([Felt::new(0).unwrap(); 4]),
        );
        Felt::new(commitments.len() as u64).unwrap()
    }",
    );
}

#[test]
fn note_write_attachment_to_memory_binding() {
    run_note_binding_test(
        "note_write_attachment_to_memory_binding",
        "pub fn binding(&self) -> Felt {
        let attachment = note::write_attachment_to_memory(
            Word::from([Felt::new(0).unwrap(); 4]),
        );
        Felt::new(attachment.len() as u64).unwrap()
    }",
    );
}

#[test]
fn note_write_indexed_attachment_to_memory_binding() {
    run_note_binding_test(
        "note_write_indexed_attachment_to_memory_binding",
        "pub fn binding(&self) -> Felt {
        let commitments = [Word::from([Felt::new(0).unwrap(); 4])];
        let attachment =
            note::write_indexed_attachment_to_memory(&commitments, 0);
        Felt::new(attachment.len() as u64).unwrap()
    }",
    );
}

#[test]
fn note_compute_recipient_binding() {
    run_note_binding_test(
        "note_compute_recipient_binding",
        "pub fn binding(&self) -> Recipient {
        note::compute_recipient(
            Word::from([Felt::new(0).unwrap(); 4]),
            Word::from([Felt::new(0).unwrap(); 4]),
            Word::from([Felt::new(0).unwrap(); 4]),
        )
    }",
    );
}

#[test]
fn note_metadata_into_sender_binding() {
    run_note_binding_test(
        "note_metadata_into_sender_binding",
        "pub fn binding(&self) -> AccountId {
        note::metadata_into_sender(Word::from([Felt::new(0).unwrap(); 4]))
    }",
    );
}

#[test]
fn note_metadata_into_attachment_schemes_binding() {
    run_note_binding_test(
        "note_metadata_into_attachment_schemes_binding",
        "pub fn binding(&self) -> Word {
        note::metadata_into_attachment_schemes(Word::from([Felt::new(0).unwrap(); 4]))
    }",
    );
}

#[test]
fn note_metadata_into_note_type_binding() {
    run_note_binding_test(
        "note_metadata_into_note_type_binding",
        "pub fn binding(&self) -> Felt {
        note::metadata_into_note_type(Word::from([Felt::new(0).unwrap(); 4])).inner
    }",
    );
}

#[test]
fn note_metadata_into_tag_binding() {
    run_note_binding_test(
        "note_metadata_into_tag_binding",
        "pub fn binding(&self) -> Felt {
        note::metadata_into_tag(Word::from([Felt::new(0).unwrap(); 4])).inner
    }",
    );
}

#[test]
fn note_find_attachment_idx_binding() {
    run_note_binding_test(
        "note_find_attachment_idx_binding",
        "pub fn binding(&self) -> u32 {
        note::find_attachment_idx(
            Felt::new(1).unwrap(),
            Word::from([Felt::new(0).unwrap(); 4]),
        )
        .unwrap_or(0)
    }",
    );
}

/// The generic attachment helpers use public advice primitives now that the protocol's raw
/// loaders are private. Exercise authentication and length checks without a transaction kernel.
#[test]
fn note_attachment_preimages() {
    use miden_core::{Felt, crypto::hash::Poseidon2};
    use miden_processor::{ExecutionOptions, FastProcessor, StackInputs, advice::AdviceInputs};

    use crate::end_to_end::support::default_host_with_core_lib;

    for (loader, max_words) in [
        ("write_attachment_commitments_to_memory", 4),
        ("write_attachment_to_memory", 256),
    ] {
        let source = format!(
            r#"(k0: Felt, k1: Felt, k2: Felt, k3: Felt) -> Felt {{
                let words = note::{loader}(Word::new([k0, k1, k2, k3]));
                for (i, word) in words.iter().enumerate() {{
                    for j in 0..4 {{
                        assert_eq!(word[j], Felt::from_u32((i * 4 + j + 1) as u32));
                    }}
                }}
                Felt::from_u32(words.len() as u32)
            }}"#
        );
        let mut test = CompilerTestBuilder::rust_fn_body_with_sdk_without_protocol(
            format!("note_preimages_{loader}"),
            &source,
            WasmTranslationConfig::default(),
            [],
        )
        .build();
        let package = test.compile_package();
        let program = package.unwrap_program();

        // Cover empty input, both sponge padding cases, the protocol maximum, malformed word
        // lengths, excessive lengths, and a preimage stored under the wrong digest.
        for (num_elements, wrong_digest, succeeds) in [
            (0, false, true),
            (4, false, true),
            (8, false, true),
            (max_words * 4, false, true),
            (3, false, false),
            ((max_words + 1) * 4, false, false),
            (4, true, false),
        ] {
            let values = (1..=num_elements)
                .map(|value| Felt::new_unchecked(value as u64))
                .collect::<Vec<_>>();
            let commitment = if wrong_digest {
                Poseidon2::hash_elements(&[Felt::new_unchecked(99); 4])
            } else {
                Poseidon2::hash_elements(&values)
            };
            let result = FastProcessor::new_with_options(
                StackInputs::new(commitment.as_elements()).unwrap(),
                AdviceInputs::default().with_map([(commitment, values)]),
                ExecutionOptions::default(),
            )
            .expect("test processor should initialize")
            .execute_sync(&program, &mut default_host_with_core_lib());
            if succeeds {
                let output = result.unwrap_or_else(|error| {
                    panic!("{loader} rejected {num_elements} elements: {error}")
                });
                assert_eq!(
                    output.stack.get_num_elements(1),
                    &[Felt::new_unchecked((num_elements / 4) as u64)],
                );
            } else {
                assert!(
                    result.is_err(),
                    "{loader} accepted invalid preimage ({num_elements} elements, wrong digest: \
                     {wrong_digest})",
                );
            }
        }
    }
}
