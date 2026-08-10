//! Integration test for DEX note schema and codec package sections.

use std::env;

use cargo_miden::run;
use miden_mast_package::{Package, SectionId};
use midenc_frontend_wasm_metadata::{
    PACKAGE_NOTE_CODEC_SECTION_ID, PACKAGE_NOTE_STORAGE_SCHEMA_SECTION_ID,
};
use wit_component::DecodedWasm;
use wit_parser::WorldItem;

use crate::utils::{current_dir_lock, workspace_root};

#[test]
fn dex_note_build_embeds_schema_and_zero_import_codec_component() {
    if !wasm_target_is_installed() {
        eprintln!("skipping DEX note codec build test: wasm32-unknown-unknown is not installed");
        return;
    }
    let _cwd_lock = current_dir_lock();
    let _ = midenc_log::Builder::from_env("MIDENC_TRACE")
        .is_test(true)
        .format_timestamp(None)
        .try_init();

    let restore_target_dir = env::var_os("CARGO_TARGET_DIR");
    unsafe {
        env::remove_var("CARGO_TARGET_DIR");
    }

    let note_dir = workspace_root().join("examples/dex-note");
    env::set_current_dir(&note_dir).unwrap();
    let result = run(["cargo", "miden", "build", "--release"].into_iter().map(str::to_owned));

    match restore_target_dir {
        Some(value) => unsafe { env::set_var("CARGO_TARGET_DIR", value) },
        None => unsafe { env::remove_var("CARGO_TARGET_DIR") },
    }

    let output = result
        .expect("cargo miden build for dex-note failed")
        .expect("expected BuildCommandOutput")
        .unwrap_build_output();
    assert_eq!(output.len(), 1, "expected one dex-note package artifact, got {output:?}");
    let package = Package::deserialize_from_file(&output[0])
        .expect("failed to read the built dex-note package");

    let schema_id = SectionId::custom(PACKAGE_NOTE_STORAGE_SCHEMA_SECTION_ID).unwrap();
    assert!(
        package.sections.iter().any(|section| section.id == schema_id),
        "dex-note package has no note storage schema section"
    );

    let codec_id = SectionId::custom(PACKAGE_NOTE_CODEC_SECTION_ID).unwrap();
    let codec = package
        .sections
        .iter()
        .find(|section| section.id == codec_id)
        .expect("dex-note package has no note codec section");
    assert_note_codec_component(codec.data.as_ref());
}

/// Returns true when rustup reports the codec component target as installed.
fn wasm_target_is_installed() -> bool {
    let output = match std::process::Command::new("rustup").args(["target", "list"]).output() {
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

/// Verifies the sandbox and versioned interface exported by a note codec component.
fn assert_note_codec_component(component: &[u8]) {
    let DecodedWasm::Component(resolve, world_id) =
        wit_component::decode(component).expect("note codec section is not valid component bytes")
    else {
        panic!("note codec section is not a component");
    };
    let world = &resolve.worlds[world_id];
    assert!(world.imports.is_empty(), "note codec imports: {:#?}", world.imports);
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
