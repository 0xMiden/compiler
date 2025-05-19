use std::{collections::VecDeque, sync::Arc};

use expect_test::{expect, expect_file};
use miden_core::utils::Deserializable;
use miden_mast_package::Package;
use miden_objects::account::AccountComponentMetadata;
use midenc_debug::{Executor, ToMidenRepr};
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
    let account_component_metadata_bytes =
        package.as_ref().account_component_metadata_bytes.clone().unwrap();
    let toml = AccountComponentMetadata::read_from_bytes(&account_component_metadata_bytes)
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
    let mut test = CompilerTest::rust_source_cargo_miden(
        "../../examples/fibonacci",
        config,
        ["--entrypoint=fibonacci::entrypoint".into()],
    );
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
            if n % 2 == 0 {
                n /= 2;
            } else {
                n = 3 * n + 1;
            }
            steps += 1;
        }
        steps
    }

    let config = WasmTranslationConfig::default();
    let mut test = CompilerTest::rust_source_cargo_miden(
        "../../examples/collatz",
        config,
        ["--entrypoint=collatz::entrypoint".into()],
    );
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
        if n % 2 == 0 || n % 3 == 0 {
            return false;
        }
        let mut i = 5;
        while i * i <= n {
            if n % i == 0 || n % (i + 2) == 0 {
                return false;
            }
            i += 6;
        }
        true
    }

    let config = WasmTranslationConfig::default();
    let mut test = CompilerTest::rust_source_cargo_miden(
        "../../examples/is-prime",
        config,
        ["--entrypoint=is_prime::entrypoint".into()],
    );
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
    let mut test =
        CompilerTest::rust_source_cargo_miden("../../examples/counter-contract", config, []);

    test.expect_wasm(expect_file!["../../expected/examples/counter.wat"]);
    test.expect_ir(expect_file!["../../expected/examples/counter.hir"]);
    test.expect_masm(expect_file!["../../expected/examples/counter.masm"]);
    let package = test.compiled_package();
    let account_component_metadata_bytes =
        package.as_ref().account_component_metadata_bytes.clone().unwrap();
    let toml = AccountComponentMetadata::read_from_bytes(&account_component_metadata_bytes)
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
fn counter_note() {
    let config = WasmTranslationConfig::default();
    let mut builder = CompilerTestBuilder::rust_source_cargo_miden(
        "../../examples/counter-note",
        config,
        // TODO: Why? detected `--targe rollup` should include them.
        ["-l".into(), "std".into(), "-l".into(), "base".into()],
    );
    // TODO: how would a user set it? Hard-code for now?
    builder.with_entrypoint(FunctionIdent {
        module: Ident::new(Symbol::intern("miden:base/note-script@1.0.0"), SourceSpan::default()),
        function: Ident::new(Symbol::intern("note-script"), SourceSpan::default()),
    });

    let mut test = builder.build();

    test.expect_wasm(expect_file!["../../expected/examples/counter_note.wat"]);
    test.expect_ir(expect_file!["../../expected/examples/counter_note.hir"]);
    test.expect_masm(expect_file!["../../expected/examples/counter_note.masm"]);
    let package = test.compiled_package();
    assert!(package.is_program(), "expected program");

    // TODO: uncomment after https://github.com/0xMiden/compiler/pull/508 is merged
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
