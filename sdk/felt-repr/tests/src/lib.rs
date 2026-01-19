//! Integration tests for felt representation serialization/deserialization.
//!
//! These tests verify the round-trip correctness of serializing data off-chain,
//! passing it to on-chain code where it's deserialized and re-serialized,
//! then deserializing the result off-chain and comparing to the original.

#![cfg(test)]

mod offchain;
mod onchain;

extern crate alloc;

use std::path::PathBuf;

use miden_integration_tests::{CompilerTest, project};
use midenc_frontend_wasm::WasmTranslationConfig;

/// Get the path to the `miden-felt-repr` crate.
fn felt_repr_path() -> PathBuf {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    PathBuf::from(manifest_dir).parent().unwrap().join("repr")
}

/// Get the path to the `miden-stdlib-sys` crate.
fn stdlib_sys_path() -> PathBuf {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    PathBuf::from(manifest_dir)
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("stdlib-sys")
}

/// Get the path to the `miden-sdk-alloc` crate.
fn sdk_alloc_path() -> PathBuf {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    PathBuf::from(manifest_dir).parent().unwrap().parent().unwrap().join("alloc")
}

/// Build a [`CompilerTest`] with a `miden-felt-repr-onchain` dependency.
///
/// The test crate is generated on disk via [`project`], which places it under the Cargo target
/// directory to reuse build artifacts across test runs.
fn build_felt_repr_test(name: &str, fn_body: &str, config: WasmTranslationConfig) -> CompilerTest {
    let felt_repr = felt_repr_path();
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
miden-felt-repr = {{ path = "{felt_repr}" }}

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
        felt_repr = felt_repr.display(),
    );

    let lib_rs = format!(
        r#"#![no_std]
#![feature(alloc_error_handler)]
#![no_main]
#![allow(unused_imports)]

#[panic_handler]
fn my_panic(_info: &core::panic::PanicInfo) -> ! {{
    core::arch::wasm32::unreachable()
}}

// Required for no-std crates
#[cfg(not(test))]
#[alloc_error_handler]
fn my_alloc_error(_info: core::alloc::Layout) -> ! {{
    loop {{}}
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

    let cargo_proj = project(name)
        .file("Cargo.toml", &cargo_toml)
        .file("src/lib.rs", &lib_rs)
        .build();

    CompilerTest::rust_source_cargo_miden(cargo_proj.root(), config, ["--test-harness".into()])
}
