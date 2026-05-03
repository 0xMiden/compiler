use std::collections::VecDeque;

use miden_debug::Executor;
use midenc_expect_test::expect_file;
use midenc_frontend_wasm::WasmTranslationConfig;
use midenc_hir::Felt;
use proptest::{prelude::*, test_runner::TestRunner};

use crate::{
    CompilerTest, CompilerTestBuilder,
    cargo_proj::project,
    compiler_test::{sdk_alloc_crate_path, sdk_crate_path},
    rust_pipeline::functional::support::cargo_toml,
};

#[test]
fn heap_base() {
    let name = "mem_intrinsics_heap_base";
    let cargo_proj = project(name)
        .file("Cargo.toml", &cargo_toml(name))
        .file(
            "src/lib.rs",
            r#"
                #![no_std]
                #![feature(alloc_error_handler)]

                // Global allocator to use heap memory in no-std environment
                #[global_allocator]
                static ALLOC: miden_sdk_alloc::BumpAlloc = miden_sdk_alloc::BumpAlloc::new();

                // Required for no-std crates
                #[panic_handler]
                fn my_panic(_info: &core::panic::PanicInfo) -> ! {
                    loop {}
                }

                // Required for no-std crates
                #[alloc_error_handler]
                fn my_alloc_error(_info: core::alloc::Layout) -> ! {
                    loop {}
                }

                extern crate alloc;
                use alloc::{vec, vec::Vec};

                #[unsafe(no_mangle)]
                pub fn entrypoint(a: u32) -> Vec<u32> {
                    vec![a*2]
                }
            "#,
        )
        .build();
    let _ = CompilerTestBuilder::rust_source_cargo_miden(
        cargo_proj.root(),
        WasmTranslationConfig::default(),
        [],
    )
    .build()
    .compile_package();
}
