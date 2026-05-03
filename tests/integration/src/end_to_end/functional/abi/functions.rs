use midenc_frontend_wasm::WasmTranslationConfig;

use crate::{
    CompilerTestBuilder, cargo_proj::project, end_to_end::functional::support::cargo_toml,
};

#[test]
fn function_call() {
    let name = "function_call_hir2";
    let cargo_proj = project(name)
        .file("Cargo.toml", &cargo_toml(name))
        .file(
            "src/lib.rs",
            r#"
                #![no_std]
                #![feature(alloc_error_handler)]

                // Global allocator to use heap memory in no-std environment
                // #[global_allocator]
                // static ALLOC: miden::BumpAlloc = miden::BumpAlloc::new();

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

                // use miden::Felt;

                #[unsafe(no_mangle)]
                #[inline(never)]
                pub fn add(a: u32, b: u32) -> u32 {
                    a + b
                }

                #[unsafe(no_mangle)]
                pub fn entrypoint(a: u32, b: u32) -> u32 {
                    add(a, b)
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
