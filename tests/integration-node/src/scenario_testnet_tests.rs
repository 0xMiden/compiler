//! Rust SDK tests that require testnet connection

use std::{env, sync::Arc};

use miden_core::{utils::Deserializable, Felt};
use miden_integration_tests::CompilerTestBuilder;
use miden_mast_package::Package;
use midenc_frontend_wasm::WasmTranslationConfig;

use crate::scenario::Scenario;

/// This test demonstrates the use of the testnet integration test infrastructure
#[ignore]
#[test]
fn rust_sdk_counter_testnet_example() {
    let mut scenario = Scenario::default();

    let target_dir = scenario.temp_dir().child("target");

    // Build counter package
    let mut args: Vec<String> = [
        "cargo",
        "miden",
        "build",
        "--manifest-path",
        "../../examples/counter-contract/Cargo.toml",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();

    // Use a new, temporary target directory to avoid conflict with other tests that compile the
    // counter example projects in parallel, but share it for both crates so that we don't recompile
    // dependencies needlessly
    args.push("--target-dir".to_string());
    args.push(target_dir.to_string_lossy().into_owned());

    dbg!(env::current_dir().unwrap().display());

    let outputs = cargo_miden::run(args.into_iter(), cargo_miden::OutputType::Masm)
        .expect("Failed to compile the counter account package for counter-note");
    let masp_path = outputs.unwrap().unwrap_build_output().into_artifact_path();

    dbg!(&masp_path);

    let _ = env_logger::builder().is_test(true).try_init();

    let config = WasmTranslationConfig::default();

    let mut builder =
        CompilerTestBuilder::rust_source_cargo_miden("../../examples/counter-note", config, []);
    builder.with_target_dir(&target_dir);
    let mut test = builder.build();
    let note_package = test.compiled_package();

    let account_package =
        Arc::new(Package::read_from_bytes(&std::fs::read(masp_path).unwrap()).unwrap());

    let key = [Felt::new(0), Felt::new(0), Felt::new(0), Felt::new(0)];
    let expected = [Felt::new(0), Felt::new(0), Felt::new(0), Felt::new(1)];
    scenario
        .create_account("example", account_package)
        .then()
        .create_note(note_package, "example", "example")
        .then()
        .submit_transaction("example")
        .assert_account_storage_map_entry_eq("example", 0, key, expected);

    scenario.run().unwrap();
}
