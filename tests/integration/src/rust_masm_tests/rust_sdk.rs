use std::{collections::BTreeMap, env, path::PathBuf, sync::Arc};

use expect_test::expect_file;
use miden_core::{crypto::hash::RpoDigest, utils::Deserializable};
use miden_mast_package::Package;
use midenc_debug::Executor;
use midenc_frontend_wasm2::WasmTranslationConfig;
use midenc_hir::{interner::Symbol, FunctionIdent, Ident, SourceSpan};

use crate::{
    cargo_proj::project, compiler_test::sdk_crate_path, CompilerTest, CompilerTestBuilder,
};

#[test]
#[ignore = "requires component translation"]
fn account() {
    let artifact_name = "miden_sdk_account_test";
    let mut test = CompilerTest::rust_source_cargo_lib(
        "../rust-apps-wasm/rust-sdk/account-test",
        artifact_name,
        true,
        None,
        None,
    );
    test.expect_wasm(expect_file![format!(
        "../../expected/rust_sdk_account_test/{artifact_name}.wat"
    )]);
    test.expect_ir(expect_file![format!(
        "../../expected/rust_sdk_account_test/{artifact_name}.hir"
    )]);
    // test.expect_masm(expect_file![format!(
    //     "../../expected/rust_sdk_account_test/{artifact_name}.masm"
    // )]);
}

#[test]
#[ignore = "requires component translation"]
fn rust_sdk_basic_wallet() {
    let _ = env_logger::builder().is_test(true).try_init();
    let config = WasmTranslationConfig::default();
    let mut test = CompilerTest::rust_source_cargo_miden(
        "../rust-apps-wasm/rust-sdk/basic-wallet",
        config,
        [],
    );
    let artifact_name = test.artifact_name().to_string();
    test.expect_wasm(expect_file![format!("../../expected/rust_sdk/{artifact_name}.wat")]);
    test.expect_ir(expect_file![format!("../../expected/rust_sdk/{artifact_name}.hir")]);
    assert!(
        test.compile_wasm_to_masm_program().is_err(),
        "expected to fail until the lifting/lowering of the heap-allocated data is supported"
    );
    // test.expect_masm(expect_file![format!("../../expected/rust_sdk/{artifact_name}.masm")]);
    // let package = test.compiled_package();
    // let lib = package.unwrap_library();
    // let expected_module = "#anon::miden:basic-wallet/basic-wallet@1.0.0";
    // let expected_function = "receive-asset";
    // let exports = lib
    //     .exports()
    //     .filter(|e| !e.module.to_string().starts_with("intrinsics"))
    //     .map(|e| format!("{}::{}", e.module, e.name.as_str()))
    //     .collect::<Vec<_>>();
    // dbg!(&exports);
    // assert!(lib.exports().any(|export| {
    //     export.module.to_string() == expected_module && export.name.as_str() == expected_function
    // }));
}

#[ignore = "until lifting/lowering of the heap-allocated data is supported"]
#[test]
fn rust_sdk_p2id_note_script() {
    // Build basic-wallet package
    let args: Vec<String> = [
        "cargo",
        "miden",
        "build",
        "--manifest-path",
        "../rust-apps-wasm/rust-sdk/basic-wallet/Cargo.toml",
        "--release",
        // Use the target dir of this test's cargo project to avoid issues running tests in parallel
        // i.e. avoid using the same target dir as the basic-wallet test (see above)
        "--target-dir",
        "../rust-apps-wasm/rust-sdk/p2id-note/target",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    dbg!(env::current_dir().unwrap().display());
    let outputs = cargo_miden::run(args.into_iter(), cargo_miden::OutputType::Masm)
        .expect("Failed to compile the basic-wallet package");
    let masp_path: PathBuf = outputs.first().unwrap().clone();
    dbg!(&masp_path);

    //
    // let masp = Package::read_from_file(masp_path.clone()).unwrap();
    // let basic_wallet_lib = match masp.mast {
    //     midenc_codegen_masm::MastArtifact::Executable(arc) => panic!("expected library"),
    //     midenc_codegen_masm::MastArtifact::Library(arc) => arc.clone(),
    // };
    // let mut masl_path = masp_path.clone();
    // masl_path.set_extension("masl");
    // basic_wallet_lib.write_to_file(masl_path.clone()).unwrap();

    let _ = env_logger::builder().is_test(true).try_init();

    let config = WasmTranslationConfig::default();
    let mut test = CompilerTest::rust_source_cargo_miden(
        "../rust-apps-wasm/rust-sdk/p2id-note",
        config,
        [
            "-l".into(),
            "std".into(),
            "-l".into(),
            "base".into(),
            "--link-library".into(),
            masp_path.into_os_string().into_string().unwrap().into(),
        ],
    );
    let artifact_name = test.artifact_name().to_string();
    test.expect_wasm(expect_file![format!("../../expected/rust_sdk/{artifact_name}.wat")]);
    test.expect_ir(expect_file![format!("../../expected/rust_sdk/{artifact_name}.hir")]);
    test.expect_masm(expect_file![format!("../../expected/rust_sdk/{artifact_name}.masm")]);
}

#[test]
#[ignore = "requires component translation"]
fn rust_sdk_cross_ctx_account() {
    let config = WasmTranslationConfig::default();
    let mut test = CompilerTest::rust_source_cargo_miden(
        "../rust-apps-wasm/rust-sdk/cross-ctx-account",
        config,
        [],
    );
    let artifact_name = test.artifact_name().to_string();
    test.expect_wasm(expect_file![format!("../../expected/rust_sdk/{artifact_name}.wat")]);
    test.expect_ir(expect_file![format!("../../expected/rust_sdk/{artifact_name}.hir")]);

    // TODO: uncomment after `unsupported parent operation for 'hir.yield': 'hir.index_switch'`
    // error is fixed
    //
    // test.expect_masm(expect_file![format!("../../expected/rust_sdk/{artifact_name}.masm")]);
    // let package = test.compiled_package();
    // let lib = package.unwrap_library();
    // let expected_module = "#anon::miden:cross-ctx-account/foo@1.0.0";
    // let expected_function = "process-felt";
    // let exports = lib
    //     .exports()
    //     .filter(|e| !e.module.to_string().starts_with("intrinsics"))
    //     .map(|e| format!("{}::{}", e.module, e.name.as_str()))
    //     .collect::<Vec<_>>();
    // dbg!(&exports);
    // assert!(lib.exports().any(|export| {
    //     export.module.to_string() == expected_module && export.name.as_str() == expected_function
    // }));
}

#[test]
#[ignore = "requires component translation"]
fn rust_sdk_cross_ctx_note() {
    // Build cross-ctx-account package
    let args: Vec<String> = [
        "cargo",
        "miden",
        "build",
        "--manifest-path",
        "../rust-apps-wasm/rust-sdk/cross-ctx-account/Cargo.toml",
        "--lib",
        "--release",
        // Use the target dir of this test's cargo project to avoid issues running tests in parallel
        // i.e. avoid using the same target dir as the basic-wallet test (see above)
        "--target-dir",
        "../rust-apps-wasm/rust-sdk/cross-ctx-note/target",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    // dbg!(env::current_dir().unwrap().display());
    let outputs = cargo_miden::run(args.into_iter(), cargo_miden::OutputType::Masm)
        .expect("Failed to compile the cross-ctx-account package for cross-ctx-note");
    let masp_path: PathBuf = outputs.first().unwrap().clone();
    // dbg!(&masp_path);

    let _ = env_logger::builder().is_test(true).try_init();

    let config = WasmTranslationConfig::default();

    let mut builder = CompilerTestBuilder::rust_source_cargo_miden(
        "../rust-apps-wasm/rust-sdk/cross-ctx-note",
        config,
        [
            "-l".into(),
            "std".into(),
            "-l".into(),
            "base".into(),
            "--link-library".into(),
            masp_path.clone().into_os_string().into_string().unwrap().into(),
        ],
    );
    builder.with_entrypoint(FunctionIdent {
        // module: Ident::new(Symbol::intern("miden:base/note-script@1.0.0"), SourceSpan::default()),
        module: Ident::new(Symbol::intern("cross_ctx_note"), SourceSpan::default()),
        // function: Ident::new(Symbol::intern("note-script"), SourceSpan::default()),
        function: Ident::new(
            Symbol::intern("miden:base/note-script@1.0.0#note-script"),
            SourceSpan::default(),
        ),
    });
    let mut test = builder.build();
    let artifact_name = test.artifact_name().to_string();
    test.expect_wasm(expect_file![format!("../../expected/rust_sdk/{artifact_name}.wat")]);
    test.expect_ir(expect_file![format!("../../expected/rust_sdk/{artifact_name}.hir")]);
    test.expect_masm(expect_file![format!("../../expected/rust_sdk/{artifact_name}.masm")]);

    // Run it in the VM (output is checked via assert_eq in the note code)
    let package = test.compiled_package();

    let mut exec = Executor::new(vec![]);
    let account_package =
        Arc::new(Package::read_from_bytes(&std::fs::read(masp_path).unwrap()).unwrap());

    exec.dependency_resolver_mut()
        .add(account_package.digest(), account_package.into());
    exec.with_dependencies(&package.manifest.dependencies).unwrap();

    let trace = exec.execute(&package.unwrap_program(), &test.session);
}

#[test]
fn pure_rust_hir2() {
    let _ = env_logger::builder().is_test(true).try_init();
    let config = WasmTranslationConfig::default();
    let mut test =
        CompilerTest::rust_source_cargo_miden("../rust-apps-wasm/rust-sdk/add", config, []);
    let artifact_name = test.artifact_name().to_string();
    test.expect_wasm(expect_file![format!("../../expected/rust_sdk/{artifact_name}.wat")]);
    test.expect_ir(expect_file![format!("../../expected/rust_sdk/{artifact_name}.hir")]);
}
