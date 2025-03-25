use std::collections::VecDeque;

use expect_test::expect_file;
use midenc_debug::{Executor, PopFromStack, PushToStack};
use midenc_frontend_wasm::WasmTranslationConfig;
use midenc_hir::Felt;
use proptest::{prelude::*, test_runner::TestRunner};

use crate::{
    cargo_proj::project,
    compiler_test::{sdk_alloc_crate_path, sdk_crate_path},
    CompilerTest, CompilerTestBuilder,
};

fn cargo_toml(name: &str) -> String {
    let sdk_alloc_path = sdk_alloc_crate_path();
    let sdk_path = sdk_crate_path();
    format!(
        r#"
                [package]
                name = "{name}"
                version = "0.0.1"
                edition = "2021"
                authors = []

                [lib]
                crate-type = ["cdylib"]

                [dependencies]
                miden-sdk-alloc = {{ path = "{sdk_alloc_path}" }}
                miden = {{ path = "{sdk_path}" }}

                [profile.release]
                # optimize the output for size
                opt-level = "z"
                panic = "abort"

                [profile.dev]
                panic = "abort"
                opt-level = 1
                debug-assertions = true
                overflow-checks = false
                debug = true
            "#,
        sdk_alloc_path = sdk_alloc_path.display(),
        sdk_path = sdk_path.display()
    )
}

#[test]
fn function_call_hir2() {
    let name = "function_call_hir2";
    let cargo_proj = project(name)
        .file("Cargo.toml", &cargo_toml(name))
        .file(
            "src/lib.rs",
            r#"
                #![no_std]

                // Global allocator to use heap memory in no-std environment
                // #[global_allocator]
                // static ALLOC: miden::BumpAlloc = miden::BumpAlloc::new();

                // Required for no-std crates
                #[panic_handler]
                fn my_panic(_info: &core::panic::PanicInfo) -> ! {
                    loop {}
                }

                // use miden::Felt;

                #[no_mangle]
                #[inline(never)]
                pub fn add(a: u32, b: u32) -> u32 {
                    a + b
                }

                #[no_mangle]
                pub fn entrypoint(a: u32, b: u32) -> u32 {
                    add(a, b)
                }
            "#,
        )
        .build();
    let mut test = CompilerTestBuilder::rust_source_cargo_miden(
        cargo_proj.root(),
        WasmTranslationConfig::default(),
        [],
    )
    .build();

    let artifact_name = name;
    test.expect_wasm(expect_file![format!("../../expected/{artifact_name}.wat")]);
    test.expect_ir(expect_file![format!("../../expected/{artifact_name}.hir")]);
}

#[test]
fn mem_intrinsics_heap_base() {
    let name = "mem_intrinsics_heap_base";
    let cargo_proj = project(name)
        .file("Cargo.toml", &cargo_toml(name))
        .file(
            "src/lib.rs",
            r#"
                #![no_std]

                // Global allocator to use heap memory in no-std environment
                #[global_allocator]
                static ALLOC: miden_sdk_alloc::BumpAlloc = miden_sdk_alloc::BumpAlloc::new();

                // Required for no-std crates
                #[panic_handler]
                fn my_panic(_info: &core::panic::PanicInfo) -> ! {
                    loop {}
                }

                extern crate alloc;
                use alloc::{vec, vec::Vec};

                #[no_mangle]
                pub fn entrypoint(a: u32) -> Vec<u32> {
                    vec![a*2]
                }
            "#,
        )
        .build();
    let mut test = CompilerTestBuilder::rust_source_cargo_miden(
        cargo_proj.root(),
        WasmTranslationConfig::default(),
        [],
    )
    .build();

    let artifact_name = name;
    test.expect_wasm(expect_file![format!("../../expected/{artifact_name}.wat")]);
    test.expect_ir(expect_file![format!("../../expected/{artifact_name}.hir")]);
}

#[test]
fn felt_intrinsics() {
    let name = "felt_intrinsics";
    let cargo_proj = project(name)
        .file("Cargo.toml", &cargo_toml(name))
        .file(
            "src/lib.rs",
            r#"
                #![no_std]

                // Required for no-std crates
                #[panic_handler]
                fn my_panic(_info: &core::panic::PanicInfo) -> ! {
                    loop {}
                }

                // Global allocator to use heap memory in no-std environment
                #[global_allocator]
                static ALLOC: miden::BumpAlloc = miden::BumpAlloc::new();

                use miden::*;

                #[no_mangle]
                pub fn entrypoint(a: Felt, b: Felt) -> Felt {
                   a / (a * b - a + b)
                }
            "#,
        )
        .build();
    let mut test = CompilerTestBuilder::rust_source_cargo_miden(
        cargo_proj.root(),
        WasmTranslationConfig::default(),
        [],
    )
    .build();

    let artifact_name = name;
    test.expect_wasm(expect_file![format!("../../expected/{artifact_name}.wat")]);
    test.expect_ir(expect_file![format!("../../expected/{artifact_name}.hir")]);
}
