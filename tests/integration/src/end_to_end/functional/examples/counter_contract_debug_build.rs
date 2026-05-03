use midenc_frontend_wasm::WasmTranslationConfig;

use crate::CompilerTestBuilder;

#[test]
fn counter_contract_debug_build() {
    // This build checks the dev profile build compilation for counter-contract
    // see https://github.com/0xMiden/compiler/issues/510
    let config = WasmTranslationConfig::default();
    let mut builder =
        CompilerTestBuilder::rust_source_cargo_miden("../../examples/counter-contract", config, []);
    builder.with_release(false);
    let mut test = builder.build();
    let _package = test.compile_package();
}
