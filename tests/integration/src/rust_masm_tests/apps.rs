use std::collections::VecDeque;

use expect_test::expect_file;
use midenc_debug::{Executor, PopFromStack, PushToStack};
use midenc_frontend_wasm::WasmTranslationConfig;
use midenc_hir::Felt;
use proptest::{prelude::*, test_runner::TestRunner};

use crate::{cargo_proj::project, CompilerTest, CompilerTestBuilder};

#[test]
fn function_call_hir2() {
    let name = "function_call_hir2";
    let cargo_proj = project(name)
        .file(
            "Cargo.toml",
            format!(
                r#"
                [package]
                name = "{name}"
                version = "0.0.1"
                edition = "2021"
                authors = []

                [lib]
                crate-type = ["cdylib"]

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
            )
            .as_str(),
        )
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
    test.expect_ir2(expect_file![format!("../../expected/{artifact_name}.hir")]);
}
