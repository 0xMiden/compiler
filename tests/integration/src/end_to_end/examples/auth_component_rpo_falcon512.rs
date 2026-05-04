use miden_core::serde::{Deserializable, Serializable};
use midenc_frontend_wasm::WasmTranslationConfig;

use crate::{CompilerTest, assert_helpers::assert_unique_protocol_export};

#[test]
fn auth_component_rpo_falcon512() {
    let config = WasmTranslationConfig::default();
    let mut test = CompilerTest::rust_source_cargo_miden(
        "../../examples/auth-component-rpo-falcon512",
        config,
        [],
    );
    let auth_comp_package = test.compile_package();
    assert!(auth_comp_package.is_library());
    assert_unique_protocol_export(auth_comp_package.as_ref(), "auth_script", "check-signature");

    // Test that the package loads
    let bytes = auth_comp_package.to_bytes();
    let _loaded_package = miden_mast_package::Package::read_from_bytes(&bytes).unwrap();
}
