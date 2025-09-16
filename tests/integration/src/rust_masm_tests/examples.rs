use std::{borrow::Borrow, collections::VecDeque, sync::Arc};

use miden_core::utils::{Deserializable, Serializable};
use miden_mast_package::{Package, SectionId};
use miden_objects::account::AccountComponentMetadata;
use midenc_debug::{Executor, ToMidenRepr};
use midenc_expect_test::{expect, expect_file};
use midenc_frontend_wasm::WasmTranslationConfig;
use midenc_hir::{
    interner::Symbol, Felt, FunctionIdent, Ident, Immediate, Op, SourceSpan, SymbolTable,
};
use prop::test_runner::{Config, TestRunner};
use proptest::prelude::*;

use crate::{cargo_proj::project, CompilerTest, CompilerTestBuilder};

#[test]
fn storage_example() {
    let config = WasmTranslationConfig::default();
    let mut test =
        CompilerTest::rust_source_cargo_miden("../../examples/storage-example", config, []);

    test.expect_wasm(expect_file!["../../expected/examples/storage_example.wat"]);
    test.expect_ir(expect_file!["../../expected/examples/storage_example.hir"]);
    test.expect_masm(expect_file!["../../expected/examples/storage_example.masm"]);
    let package = test.compiled_package();
    let account_component_metadata_bytes = package
        .as_ref()
        .sections
        .iter()
        .find_map(|s| {
            if s.id == SectionId::ACCOUNT_COMPONENT_METADATA {
                Some(s.data.borrow())
            } else {
                None
            }
        })
        .unwrap();
    let toml = AccountComponentMetadata::read_from_bytes(account_component_metadata_bytes)
        .unwrap()
        .as_toml()
        .unwrap();
    expect![[r#"
        name = "storage-example"
        description = "A simple example of a Miden account storage API"
        version = "0.1.0"
        supported-types = ["RegularAccountUpdatableCode"]

        [[storage]]
        name = "owner_public_key"
        description = "test value"
        slot = 0
        type = "auth::rpo_falcon512::pub_key"

        [[storage]]
        name = "asset_qty_map"
        description = "test map"
        slot = 1
        values = []
    "#]]
    .assert_eq(&toml);
}

#[test]
fn fibonacci() {
    fn expected_fib(n: u32) -> u32 {
        let mut a = 0;
        let mut b = 1;
        for _ in 0..n {
            let c = a + b;
            a = b;
            b = c;
        }
        a
    }

    let config = WasmTranslationConfig::default();
    let mut test = CompilerTest::rust_source_cargo_miden("../../examples/fibonacci", config, []);
    test.expect_wasm(expect_file!["../../expected/examples/fib.wat"]);
    test.expect_ir(expect_file!["../../expected/examples/fib.hir"]);
    test.expect_masm(expect_file!["../../expected/examples/fib.masm"]);
    let package = test.compiled_package();

    // Run the Rust and compiled MASM code against a bunch of random inputs and compare the results
    TestRunner::default()
        .run(&(1u32..30), move |a| {
            let rust_out = expected_fib(a);
            let args = a.to_felts();
            let exec = Executor::for_package(&package, args, &test.session)
                .map_err(|err| TestCaseError::fail(err.to_string()))?;
            let output: u32 = exec.execute_into(&package.unwrap_program(), &test.session);
            dbg!(output);
            prop_assert_eq!(rust_out, output);
            Ok(())
        })
        .unwrap_or_else(|err| panic!("{err}"));
}

#[test]
fn collatz() {
    let _ = env_logger::Builder::from_env("MIDENC_TRACE")
        .format_timestamp(None)
        .is_test(true)
        .try_init();

    fn expected(mut n: u32) -> u32 {
        let mut steps = 0;
        while n != 1 {
            if n.is_multiple_of(2) {
                n /= 2;
            } else {
                n = 3 * n + 1;
            }
            steps += 1;
        }
        steps
    }

    let config = WasmTranslationConfig::default();
    let mut test = CompilerTest::rust_source_cargo_miden("../../examples/collatz", config, []);
    let artifact_name = "collatz";
    test.expect_wasm(expect_file![format!("../../expected/{artifact_name}.wat")]);
    test.expect_ir(expect_file![format!("../../expected/{artifact_name}.hir")]);
    test.expect_masm(expect_file![format!("../../expected/{artifact_name}.masm")]);
    let package = test.compiled_package();

    // Run the Rust and compiled MASM code against a bunch of random inputs and compare the results
    TestRunner::new(Config::with_cases(4))
        .run(&(1u32..30), move |a| {
            let rust_out = expected(a);
            let args = a.to_felts();
            let exec = Executor::for_package(&package, args, &test.session)
                .map_err(|err| TestCaseError::fail(err.to_string()))?;
            let output: u32 = exec.execute_into(&package.unwrap_program(), &test.session);
            dbg!(output);
            prop_assert_eq!(rust_out, output);
            Ok(())
        })
        .unwrap_or_else(|err| {
            panic!("{err}");
        });
}

#[test]
fn is_prime() {
    let _ = env_logger::Builder::from_env("MIDENC_TRACE")
        .format_timestamp(None)
        .is_test(true)
        .try_init();

    fn expected(n: u32) -> bool {
        if n <= 1 {
            return false;
        }
        if n <= 3 {
            return true;
        }
        if n.is_multiple_of(2) || n.is_multiple_of(3) {
            return false;
        }
        let mut i = 5;
        while i * i <= n {
            if n.is_multiple_of(i) || n.is_multiple_of(i + 2) {
                return false;
            }
            i += 6;
        }
        true
    }

    let config = WasmTranslationConfig::default();
    let mut test = CompilerTest::rust_source_cargo_miden("../../examples/is-prime", config, []);
    let artifact_name = "is_prime";
    test.expect_wasm(expect_file![format!("../../expected/{artifact_name}.wat")]);
    test.expect_ir(expect_file![format!("../../expected/{artifact_name}.hir")]);
    test.expect_masm(expect_file![format!("../../expected/{artifact_name}.masm")]);
    let package = test.compiled_package();
    let hir = test.hir();

    println!("{}", hir.borrow().as_operation());

    // Run the Rust and compiled MASM code against a bunch of random inputs and compare the results
    TestRunner::new(Config::with_cases(100))
        .run(&(1u32..30), move |a| {
            let rust_out = expected(a);

            // Test the IR
            let mut evaluator =
                midenc_hir_eval::HirEvaluator::new(hir.borrow().as_operation().context_rc());
            let op = hir
                .borrow()
                .symbol_manager()
                .lookup_symbol_ref(
                    &midenc_hir::SymbolPath::new([
                        midenc_hir::SymbolNameComponent::Component("is_prime".into()),
                        midenc_hir::SymbolNameComponent::Leaf("entrypoint".into()),
                    ])
                    .unwrap(),
                )
                .unwrap();
            let result = evaluator
                .eval(&op.borrow(), [midenc_hir_eval::Value::Immediate((a as i32).into())])
                .unwrap_or_else(|err| panic!("{err}"));
            let midenc_hir_eval::Value::Immediate(Immediate::I32(result)) = result[0] else {
                //return Err(TestCaseError::fail(format!(
                panic!("expected i32 immediate for input {a}, got {:?}", result[0]);
                //)));
            };
            prop_assert_eq!(rust_out as i32, result);

            let args = a.to_felts();
            let exec = Executor::for_package(&package, args, &test.session)
                .map_err(|err| TestCaseError::fail(err.to_string()))?;
            let output: u32 = exec.execute_into(&package.unwrap_program(), &test.session);
            dbg!(output);
            prop_assert_eq!(rust_out as u32, output);
            Ok(())
        })
        .unwrap_or_else(|err| {
            panic!("{err}");
        });
}

#[test]
fn counter_contract() {
    let config = WasmTranslationConfig::default();
    let mut builder_release = CompilerTestBuilder::rust_source_cargo_miden(
        "../../examples/counter-contract",
        config.clone(),
        [],
    );
    builder_release.with_release(true);
    let mut test_release = builder_release.build();
    test_release.expect_wasm(expect_file!["../../expected/examples/counter.wat"]);
    test_release.expect_ir(expect_file!["../../expected/examples/counter.hir"]);
    test_release.expect_masm(expect_file!["../../expected/examples/counter.masm"]);
    let package = test_release.compiled_package();
    let account_component_metadata_bytes = package
        .as_ref()
        .sections
        .iter()
        .find_map(|s| {
            if s.id == SectionId::ACCOUNT_COMPONENT_METADATA {
                Some(s.data.borrow())
            } else {
                None
            }
        })
        .unwrap();
    let toml = AccountComponentMetadata::read_from_bytes(account_component_metadata_bytes)
        .unwrap()
        .as_toml()
        .unwrap();
    expect![[r#"
        name = "counter-contract"
        description = "A simple example of a Miden counter contract using the Account Storage API"
        version = "0.1.0"
        supported-types = ["RegularAccountUpdatableCode"]

        [[storage]]
        name = "count_map"
        description = "counter contract storage map"
        slot = 0
        values = []
    "#]]
    .assert_eq(&toml);
}

#[test]
fn counter_contract_debug_build() {
    // This build checks the dev profile build compilation for counter-contract
    // see https://github.com/0xMiden/compiler/issues/510
    let config = WasmTranslationConfig::default();
    let mut builder =
        CompilerTestBuilder::rust_source_cargo_miden("../../examples/counter-contract", config, []);
    builder.with_release(false);
    let mut test = builder.build();
    let package = test.compiled_package();
}

#[test]
fn counter_note() {
    // build and check counter-note
    let config = WasmTranslationConfig::default();
    let builder =
        CompilerTestBuilder::rust_source_cargo_miden("../../examples/counter-note", config, []);

    let mut test = builder.build();

    test.expect_wasm(expect_file!["../../expected/examples/counter_note.wat"]);
    test.expect_ir(expect_file!["../../expected/examples/counter_note.hir"]);
    test.expect_masm(expect_file!["../../expected/examples/counter_note.masm"]);
    let package = test.compiled_package();
    assert!(package.is_program(), "expected program");

    // TODO: uncomment after the testing environment implemented (node, devnet, etc.)
    //
    // let mut exec = Executor::new(vec![]);
    // for dep_path in test.dependencies {
    //     let account_package =
    //         Arc::new(Package::read_from_bytes(&std::fs::read(dep_path).unwrap()).unwrap());
    //     exec.dependency_resolver_mut()
    //         .add(account_package.digest(), account_package.into());
    // }
    // exec.with_dependencies(&package.manifest.dependencies).unwrap();
    // let trace = exec.execute(&package.unwrap_program(), &test.session);
}

#[test]
fn basic_wallet_and_p2id() {
    let config = WasmTranslationConfig::default();
    let mut test =
        CompilerTest::rust_source_cargo_miden("../../examples/basic-wallet", config.clone(), []);
    test.expect_wasm(expect_file![format!("../../expected/examples/basic_wallet.wat")]);
    test.expect_ir(expect_file![format!("../../expected/examples/basic_wallet.hir")]);
    test.expect_masm(expect_file![format!("../../expected/examples/basic_wallet.masm")]);
    let account_package = test.compiled_package();
    assert!(account_package.is_library(), "expected library");

    let mut test = CompilerTest::rust_source_cargo_miden(
        "../../examples/basic-wallet-tx-script",
        config.clone(),
        [],
    );
    test.expect_wasm(expect_file![format!("../../expected/examples/basic_wallet_tx_script.wat")]);
    test.expect_ir(expect_file![format!("../../expected/examples/basic_wallet_tx_script.hir")]);
    test.expect_masm(expect_file![format!("../../expected/examples/basic_wallet_tx_script.masm")]);
    let package = test.compiled_package();
    assert!(package.is_program(), "expected program");

    let mut test = CompilerTest::rust_source_cargo_miden("../../examples/p2id-note", config, []);
    test.expect_wasm(expect_file![format!("../../expected/examples/p2id.wat")]);
    test.expect_ir(expect_file![format!("../../expected/examples/p2id.hir")]);
    test.expect_masm(expect_file![format!("../../expected/examples/p2id.masm")]);
    let note_package = test.compiled_package();
    assert!(note_package.is_program(), "expected program");
}

#[test]
fn auth_component_no_auth() {
    let config = WasmTranslationConfig::default();
    let mut test =
        CompilerTest::rust_source_cargo_miden("../../examples/auth-component-no-auth", config, []);
    test.expect_wasm(expect_file![format!("../../expected/examples/auth_component_no_auth.wat")]);
    test.expect_ir(expect_file![format!("../../expected/examples/auth_component_no_auth.hir")]);
    test.expect_masm(expect_file![format!("../../expected/examples/auth_component_no_auth.masm")]);
    let auth_comp_package = test.compiled_package();
    let lib = auth_comp_package.unwrap_library();
    let expected_module = "miden:base/authentication-component@1.0.0";
    let expected_function = "auth__procedure";
    let exports = lib
        .exports()
        .map(|e| format!("{}::{}", e.name.module, e.name.name.as_str()))
        .collect::<Vec<_>>();
    // dbg!(&exports);
    assert!(
        lib.exports().any(|export| {
            export.name.module.to_string() == expected_module
                && export.name.name.as_str() == expected_function
        }),
        "expected one of the exports to contain module '{expected_module}' and function \
         '{expected_function}'"
    );

    // Test that the package loads
    let bytes = auth_comp_package.to_bytes();
    let _loaded_package = miden_mast_package::Package::read_from_bytes(&bytes).unwrap();
}

#[test]
fn auth_component_rpo_falcon512() {
    let config = WasmTranslationConfig::default();
    let mut test = CompilerTest::rust_source_cargo_miden(
        "../../examples/auth-component-rpo-falcon512",
        config,
        [],
    );
    test.expect_wasm(expect_file![format!(
        "../../expected/examples/auth_component_rpo_falcon512.wat"
    )]);
    test.expect_ir(expect_file![format!(
        "../../expected/examples/auth_component_rpo_falcon512.hir"
    )]);
    test.expect_masm(expect_file![format!(
        "../../expected/examples/auth_component_rpo_falcon512.masm"
    )]);
    let auth_comp_package = test.compiled_package();
    let lib = auth_comp_package.unwrap_library();
    let expected_module = "miden:base/authentication-component@1.0.0";
    let expected_function = "auth__procedure";
    assert!(
        lib.exports().any(|export| {
            export.name.module.to_string() == expected_module
                && export.name.name.as_str() == expected_function
        }),
        "expected one of the exports to contain module '{expected_module}' and function \
         '{expected_function}'"
    );

    // Test that the package loads
    let bytes = auth_comp_package.to_bytes();
    let _loaded_package = miden_mast_package::Package::read_from_bytes(&bytes).unwrap();
}
