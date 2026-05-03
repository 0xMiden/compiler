use midenc_frontend_wasm::WasmTranslationConfig;

use crate::{
    CompilerTestBuilder, cargo_proj::project, end_to_end::functional::support::cargo_toml,
};

#[test]
fn basic_felt_arithmetic() {
    let name = "felt_intrinsics";
    let cargo_proj = project(name)
        .file("Cargo.toml", &cargo_toml(name))
        .file(
            "src/lib.rs",
            r#"
                #![no_std]
                #![feature(alloc_error_handler)]

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

                // Global allocator to use heap memory in no-std environment
                #[global_allocator]
                static ALLOC: miden::BumpAlloc = miden::BumpAlloc::new();

                use miden::*;

                #[unsafe(no_mangle)]
                pub fn entrypoint(a: Felt, b: Felt) -> Felt {
                   a / (a * b - a + b)
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
