//! Integration tests for felt representation serialization/deserialization.
//!
//! These tests verify the round-trip correctness of serializing data off-chain,
//! passing it to on-chain code where it's deserialized and re-serialized,
//! then deserializing the result off-chain and comparing to the original.

#![cfg(test)]

mod offchain;
mod onchain;

extern crate alloc;

use std::{fs, path::PathBuf};

use miden_integration_tests::CompilerTest;
use midenc_frontend_wasm::WasmTranslationConfig;
use temp_dir::TempDir;

/// Get the path to the felt-repr/onchain crate
fn felt_repr_onchain_path() -> PathBuf {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    PathBuf::from(manifest_dir).parent().unwrap().join("onchain")
}

/// Get the path to the stdlib-sys crate
fn stdlib_sys_path() -> PathBuf {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    PathBuf::from(manifest_dir)
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("sdk")
        .join("stdlib-sys")
}

/// Get the path to the sdk-alloc crate
fn sdk_alloc_path() -> PathBuf {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    PathBuf::from(manifest_dir)
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("sdk")
        .join("alloc")
}

/// Build a compiler test with felt-repr-onchain dependency.
///
/// The `temp_dir` must be kept alive for the duration of the test to prevent cleanup.
fn build_felt_repr_test(
    temp_dir: &TempDir,
    name: &str,
    fn_body: &str,
    config: WasmTranslationConfig,
) -> CompilerTest {
    let felt_repr_onchain = felt_repr_onchain_path();
    let stdlib_sys = stdlib_sys_path();
    let sdk_alloc = sdk_alloc_path();

    let cargo_toml = format!(
        r#"cargo-features = ["trim-paths"]

[package]
name = "{name}"
version = "0.0.1"
edition = "2021"
authors = []

[dependencies]
miden-sdk-alloc = {{ path = "{sdk_alloc}" }}
miden-stdlib-sys = {{ path = "{stdlib_sys}" }}
miden-felt-repr-onchain = {{ path = "{felt_repr_onchain}" }}

[lib]
crate-type = ["cdylib"]

[profile.release]
panic = "abort"
opt-level = "z"
debug = false
trim-paths = ["diagnostics", "object"]

[workspace]
"#,
        sdk_alloc = sdk_alloc.display(),
        stdlib_sys = stdlib_sys.display(),
        felt_repr_onchain = felt_repr_onchain.display(),
    );

    let lib_rs = format!(
        r#"#![no_std]
#![no_main]
#![allow(unused_imports)]

#[panic_handler]
fn my_panic(_info: &core::panic::PanicInfo) -> ! {{
    core::arch::wasm32::unreachable()
}}

#[global_allocator]
static ALLOC: miden_sdk_alloc::BumpAlloc = miden_sdk_alloc::BumpAlloc::new();

extern crate miden_stdlib_sys;
use miden_stdlib_sys::{{*, intrinsics}};

extern crate alloc;
use alloc::vec::Vec;

#[no_mangle]
#[allow(improper_ctypes_definitions)]
pub extern "C" fn entrypoint{fn_body}
"#
    );

    let project_dir = temp_dir.path().to_path_buf();
    let src_dir = project_dir.join("src");
    fs::create_dir_all(&src_dir).expect("failed to create src directory");
    fs::write(project_dir.join("Cargo.toml"), cargo_toml).expect("failed to write Cargo.toml");
    fs::write(src_dir.join("lib.rs"), lib_rs).expect("failed to write lib.rs");

    // Use --test-harness to enable proper advice stack handling
    CompilerTest::rust_source_cargo_miden(project_dir, config, ["--test-harness".into()])
}
