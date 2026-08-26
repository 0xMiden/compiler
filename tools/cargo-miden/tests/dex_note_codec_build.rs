//! Integration test for DEX note schema and codec package sections.

use std::env;

use cargo_miden::run;
use miden_mast_package::Package;
use midenc_frontend_wasm_metadata::{
    package_note_codec_section_id, package_note_storage_schema_section_id,
};
use midenc_integration_test_support::{
    example_build_lock, wasm_target_is_installed, workspace_root,
};
use wit_component::DecodedWasm;
use wit_parser::WorldItem;

use crate::utils::{RestoreEnvironment, current_dir_lock};

#[test]
fn dex_note_build_embeds_schema_and_wasi_only_codec_component() {
    if !wasm_target_is_installed() {
        eprintln!("skipping DEX note codec build test: wasm32-wasip2 is not installed");
        return;
    }
    // The command reads the process working directory, so serialize cwd changes.
    let _cwd_lock = current_dir_lock();
    let _ = midenc_log::Builder::from_env("MIDENC_TRACE")
        .is_test(true)
        .format_timestamp(None)
        .try_init();

    // Clear the outer override so the nested example build uses its own target layout.
    let _restore_environment = RestoreEnvironment::new(["CARGO_TARGET_DIR"]);
    unsafe {
        env::remove_var("CARGO_TARGET_DIR");
    }

    let workspace = workspace_root();
    let note_dir = workspace.join("examples/dex-note");
    env::set_current_dir(&note_dir).unwrap();
    let result = {
        let _build_lock = example_build_lock(&workspace);
        run(["cargo", "miden", "build", "--release"].into_iter().map(str::to_owned))
    };

    let output = result
        .expect("cargo miden build for dex-note failed")
        .expect("expected BuildCommandOutput")
        .unwrap_build_output();
    assert_eq!(output.len(), 1, "expected one dex-note package artifact, got {output:?}");
    let package = Package::deserialize_from_file(&output[0])
        .expect("failed to read the built dex-note package");

    let schema_id = package_note_storage_schema_section_id();
    assert!(
        package.sections.iter().any(|section| section.id == schema_id),
        "dex-note package has no note storage schema section"
    );

    let codec_id = package_note_codec_section_id();
    let codec = package
        .sections
        .iter()
        .find(|section| section.id == codec_id)
        .expect("dex-note package has no note codec section");
    assert_note_codec_component(codec.data.as_ref());
}

/// Verifies the sandbox and versioned interface exported by a note codec component.
fn assert_note_codec_component(component: &[u8]) {
    let DecodedWasm::Component(resolve, world_id) =
        wit_component::decode(component).expect("note codec section is not valid component bytes")
    else {
        panic!("note codec section is not a component");
    };
    let world = &resolve.worlds[world_id];
    for (key, _) in world.imports.iter() {
        let name = resolve.name_world_key(key);
        assert!(name.starts_with("wasi:"), "unexpected non-WASI import `{name}`");
    }
    assert_eq!(world.exports.len(), 1, "unexpected note codec exports: {:#?}", world.exports);

    let interface = world
        .exports
        .values()
        .find_map(|item| {
            let WorldItem::Interface { id, .. } = item else {
                return None;
            };
            let interface = &resolve.interfaces[*id];
            let package_id = interface.package?;
            let package = &resolve.packages[package_id].name;
            (interface.name.as_deref() == Some("codec")
                && package.namespace == "miden"
                && package.name == "note-codec"
                && package.version.as_ref().is_some_and(|version| version.to_string() == "1.0.0"))
            .then_some(interface)
        })
        .expect("component does not export `miden:note-codec/codec@1.0.0`");
    assert_eq!(
        interface.functions.keys().map(String::as_str).collect::<Vec<_>>(),
        ["supported-types", "parse", "display", "validate"]
    );
}
