use std::{
    collections::{BTreeMap, HashSet},
    path::PathBuf,
    str::FromStr,
};

use expect_test::expect_file;
use miden_core::crypto::hash::RpoDigest;
use midenc_frontend_wasm::WasmTranslationConfig;
use midenc_hir::{InterfaceFunctionIdent, InterfaceIdent, Symbol};

use crate::CompilerTest;

#[test]
fn sdk() {
    let test = CompilerTest::rust_source_cargo_component(
        "../rust-apps-wasm/wit-sdk/sdk",
        Default::default(),
    );
    let artifact_name = test.artifact_name();
    test.expect_wasm(expect_file![format!(
        "../../expected/wit_sdk_basic_wallet/{artifact_name}.wat"
    )]);
}

#[test]
fn sdk_basic_wallet() {
    let interface_output_note = InterfaceIdent::from_full_ident("miden:base/output-note@1.0.0");
    let create_note_ident = InterfaceFunctionIdent {
        interface: interface_output_note,
        function: Symbol::intern("create"),
    };
    let interface_account = InterfaceIdent::from_full_ident("miden:base/account@1.0.0");
    let add_asset_ident = InterfaceFunctionIdent {
        interface: interface_account,
        function: Symbol::intern("add-asset"),
    };
    let remove_asset_ident = InterfaceFunctionIdent {
        interface: interface_account,
        function: Symbol::intern("remove-asset"),
    };
    let output_note_add_asset_ident = InterfaceFunctionIdent {
        interface: interface_output_note,
        function: Symbol::intern("add-asset"),
    };
    let expected_imports: HashSet<InterfaceFunctionIdent> = [
        create_note_ident,
        remove_asset_ident,
        add_asset_ident,
        output_note_add_asset_ident,
    ]
    .into_iter()
    .collect();
    let config = WasmTranslationConfig::default();
    let mut test =
        CompilerTest::rust_source_cargo_component("../rust-apps-wasm/wit-sdk/basic-wallet", config);
    let artifact_name = test.artifact_name();
    test.expect_wasm(expect_file![format!(
        "../../expected/wit_sdk_basic_wallet/{artifact_name}.wat"
    )]);
    test.expect_ir(expect_file![format!(
        "../../expected/wit_sdk_basic_wallet/{artifact_name}.hir"
    )]);
    let ir = test.hir().unwrap_component();
    for import in ir.imports().values() {
        assert!(expected_imports.contains(&import.unwrap_canon_abi_import().interface_function));
    }
}

#[test]
fn sdk_basic_wallet_p2id_note() {
    let interface_account = InterfaceIdent::from_full_ident("miden:base/account@1.0.0");
    let basic_wallet = InterfaceIdent::from_full_ident("miden:basic-wallet/basic-wallet@1.0.0");
    let core_types = InterfaceIdent::from_full_ident("miden:base/core-types@1.0.0");
    let note = InterfaceIdent::from_full_ident("miden:base/note@1.0.0");
    let expected_imports: HashSet<InterfaceFunctionIdent> = [
        InterfaceFunctionIdent {
            interface: interface_account,
            function: Symbol::intern("get-id"),
        },
        InterfaceFunctionIdent {
            interface: core_types,
            function: Symbol::intern("account-id-from-felt"),
        },
        InterfaceFunctionIdent {
            interface: basic_wallet,
            function: Symbol::intern("receive-asset"),
        },
        InterfaceFunctionIdent {
            interface: note,
            function: Symbol::intern("get-assets"),
        },
        InterfaceFunctionIdent {
            interface: note,
            function: Symbol::intern("get-inputs"),
        },
    ]
    .into_iter()
    .collect();
    let config = WasmTranslationConfig::default();
    let mut test =
        CompilerTest::rust_source_cargo_component("../rust-apps-wasm/wit-sdk/p2id-note", config);
    let artifact_name = test.artifact_name();
    test.expect_wasm(expect_file![format!(
        "../../expected/wit_sdk_basic_wallet/{artifact_name}.wat"
    )]);
    test.expect_ir(expect_file![format!(
        "../../expected/wit_sdk_basic_wallet/{artifact_name}.hir"
    )]);
    let ir = test.hir().unwrap_component();
    for import in ir.imports().values() {
        let canon_abi_import = import.unwrap_canon_abi_import();
        assert!(expected_imports.contains(&canon_abi_import.interface_function));
        if ["get-assets", "get-inputs"]
            .contains(&canon_abi_import.interface_function.function.as_str())
        {
            assert!(canon_abi_import.options.realloc.is_some());
        }
    }
}
