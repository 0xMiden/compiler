//! Integration test for the account storage schema and codec sections of an account component.

use std::env;

use cargo_miden::run;
use miden_mast_package::Package;
use midenc_frontend_wasm_metadata::{
    package_account_storage_schema_section_id, package_codec_section_id,
};
use midenc_integration_test_support::{
    example_build_lock, wasm_target_is_installed, workspace_root,
};

use crate::{
    dex_note_codec_build::assert_codec_component,
    utils::{RestoreEnvironment, current_dir_lock},
};

#[test]
fn storage_example_build_embeds_schema_and_wasi_only_codec_component() {
    if !wasm_target_is_installed() {
        eprintln!("skipping storage-example codec build test: wasm32-wasip2 is not installed");
        return;
    }
    // The command reads the process working directory, so serialize cwd changes.
    let _cwd_lock = current_dir_lock();
    let _ = midenc_log::Builder::from_env("MIDENC_TRACE")
        .is_test(true)
        .format_timestamp(None)
        .try_init();

    // Clear the outer override. The example package then uses its own target layout.
    let _restore_environment = RestoreEnvironment::new(["CARGO_TARGET_DIR"]);
    unsafe {
        env::remove_var("CARGO_TARGET_DIR");
    }

    let workspace = workspace_root();
    let component_dir = workspace.join("examples/storage-example");
    env::set_current_dir(&component_dir).unwrap();
    let result = {
        let _build_lock = example_build_lock(&workspace);
        run(["cargo", "miden", "build", "--release"].into_iter().map(str::to_owned))
    };

    let output = result
        .expect("cargo miden build for storage-example failed")
        .expect("expected BuildCommandOutput")
        .unwrap_build_output();
    let codec_work_dir = workspace.join("examples/storage-example-codec/target/midenc.codec");
    assert!(
        codec_work_dir.join("package-cache").is_dir(),
        "the codec package cache is outside the codec crate"
    );
    assert!(
        codec_work_dir.join("cargo-target").is_dir(),
        "the codec Cargo target directory is outside the codec crate"
    );
    assert_eq!(output.len(), 1, "expected one storage-example package artifact, got {output:?}");
    let package = Package::deserialize_from_file(&output[0])
        .expect("failed to read the built storage-example package");

    let schema_id = package_account_storage_schema_section_id();
    assert!(
        package.sections.iter().any(|section| section.id == schema_id),
        "storage-example package has no account storage schema section"
    );

    let codec_id = package_codec_section_id();
    let codec = package
        .sections
        .iter()
        .find(|section| section.id == codec_id)
        .expect("storage-example package has no codec section");
    assert_codec_component(codec.data.as_ref());
}
