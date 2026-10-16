//! A user-written Miden Assembly account component linked from Rust with no hand-written WIT.
//!
//! The fixture `tests/fixtures/components/acme-counter-masm` is a MASM project with a user's
//! names (package `counter`, namespace `acme::counter::counter`). The tests prove that building
//! it with `midenc` yields an account-component package whose manifest keeps the role attributes
//! and the typed signatures, that the WIT interface synthesized from that manifest offers every
//! account procedure (a record parameter and a word result included) and leaves the auth
//! procedure out, and that a Rust note and a Rust transaction script depending on the project by
//! path compile to calls of the component's procedures.

use std::path::Path;

use miden_mast_package::{Package, TargetType};
use midenc_expect_test::{expect, expect_file};
use midenc_frontend_wasm::WasmTranslationConfig;
use midenc_integration_test_support::{
    compile_masm_project, find_export, testing::toolchain::sysroot,
};
use midenc_package_interface::PackageInterface;

use crate::CompilerTest;

/// The fixture MASM project, relative to this crate.
const COUNTER_PROJECT: &str = "../fixtures/components/acme-counter-masm";

/// The fixture's `[lib].namespace`, the module of its procedures.
const COUNTER_NAMESPACE: &str = "acme::counter::counter";

/// Builds the fixture MASM project against the toolchain.
fn counter_package() -> std::sync::Arc<Package> {
    compile_masm_project(Path::new(COUNTER_PROJECT), &sysroot())
}

/// The midenc build of the MASM project is an account component whose manifest exports carry the
/// role attributes and the typed signatures written in the source.
#[test]
fn user_masm_component_builds_as_an_account_component() {
    let package = counter_package();
    assert_eq!(package.kind, TargetType::AccountComponent);
    assert_eq!(&*package.name, "counter");

    let mut signatures = String::new();
    for leaf in [
        "increment",
        "get_count",
        "add_to_count",
        "get_count_word",
        "wide",
        "split",
        "four_words",
        "fifteen_to_word",
        "auth_incr_nonce",
    ] {
        let export = find_export(&package, COUNTER_NAMESPACE, leaf);
        let signature = export
            .signature
            .as_ref()
            .unwrap_or_else(|| panic!("`{leaf}` must carry its typed signature"));
        let roles = ["account_procedure", "auth_script"]
            .into_iter()
            .filter(|role| export.attributes.has(role))
            .collect::<Vec<_>>()
            .join(" ");
        signatures.push_str(&format!("{leaf}: @{roles} {signature}\n"));
    }
    expect![[r#"
        increment: @account_procedure extern "component-model" fn()
        get_count: @account_procedure extern "component-model" fn() -> felt
        add_to_count: @account_procedure extern "component-model" fn(struct Delta {add : felt, mul : felt})
        get_count_word: @account_procedure extern "component-model" fn() -> [felt; 4]
        wide: @account_procedure extern "component-model" fn([felt; 4], [felt; 4], [felt; 4], [felt; 4]) -> felt
        split: @account_procedure extern "component-model" fn(felt) -> felt, felt
        four_words: @account_procedure extern "component-model" fn() -> [felt; 4], [felt; 4], [felt; 4], [felt; 4]
        fifteen_to_word: @account_procedure extern "component-model" fn([felt; 4], [felt; 4], [felt; 4], felt, felt, felt) -> [felt; 4]
        auth_incr_nonce: @auth_script extern "component-model" fn([felt; 4])
    "#]].assert_eq(&signatures);
}

/// The WIT interface synthesized from the package manifest matches its snapshot: package
/// `acme:counter`, interface `counter`, the eight account procedures with their export paths, the
/// `delta` record, the `word` result, the four-word `wide`, the `tuple` results of `split` and
/// `four_words`, and the 15 parameter felts of `fifteen_to_word`, and the auth procedure left out.
#[test]
fn user_masm_component_interface_is_synthesized_from_the_manifest() {
    let package = counter_package();
    let generated = midenc_package_wit::generate(&PackageInterface::from_package(&package))
        .unwrap_or_else(|err| panic!("the counter component must have an interface: {err}"));
    assert_eq!(generated.package_id, "acme:counter@0.1.0");
    assert_eq!(generated.interface, "counter");
    assert_eq!(
        generated
            .skipped
            .iter()
            .map(|skipped| skipped.path.as_str())
            .collect::<Vec<_>>(),
        ["acme::counter::counter::auth_incr_nonce"]
    );
    expect_file!["../../../fixtures/components/acme-counter-masm/expected.wit"]
        .assert_eq(&generated.wit);
}

/// Compiles a Rust fixture depending on the MASM project and returns its lowered MASM.
fn consumer_masm(fixture: &str) -> String {
    let mut test = CompilerTest::rust_source_cargo_miden(
        format!("../fixtures/components/{fixture}"),
        WasmTranslationConfig::default(),
        [],
    );
    let masm = test.masm_src();
    let _package = test.compile_package();
    masm
}

/// Asserts `masm` calls each of `procedures` of the counter component.
fn assert_calls(masm: &str, procedures: &[&str]) {
    for procedure in procedures {
        // A whole-line match: `get_count` must not pass on a call of `get_count_word`.
        let call = format!("call.::{COUNTER_NAMESPACE}::{procedure}");
        assert!(masm.lines().any(|line| line.trim() == call), "expected `{call}` in:\n{masm}");
    }
}

/// A Rust note with `#[account(counter::Counter)]` compiles to calls of `increment` and
/// `add_to_count`, the latter taking the `Delta` record.
#[test]
fn user_masm_component_is_called_from_a_rust_note() {
    let masm = consumer_masm("acme-counter-note");
    assert_calls(&masm, &["increment", "add_to_count"]);
}

/// A Rust transaction script with `#[account(counter::Counter)]` compiles to calls of the
/// `get_count` and `get_count_word` getters, of `wide`, which takes 16 parameter felts, of
/// `split` and `four_words`, which return tuples, and of `fifteen_to_word`.
#[test]
fn user_masm_component_is_called_from_a_rust_tx_script() {
    let masm = consumer_masm("acme-counter-tx-script");
    assert_calls(
        &masm,
        &["get_count", "get_count_word", "wide", "split", "four_words", "fifteen_to_word"],
    );
}
