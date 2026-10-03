//! A Rust project binds a Miden Assembly package of its own (goal 4 of the package-bindgen
//! design).
//!
//! The fixture `tests/fixtures/masm-dependency-bindings` declares the MASM path dependency
//! `masm-dep` in its `miden-project.toml`. Its `build.rs` generates the dependency's bindings
//! from the compiled package with `miden_sdk_build_script_support::generate_bindings` and links
//! their stubs; `entrypoint` calls two of the dependency's procedures through them. The second
//! test does the same from a Rust project that is itself a dependency of the project being built.
//! The package keeps its default root module, `::"masm-dep"` — a quoted path component, which is
//! what every hyphenated package name produces — so both tests cover that spelling.

use std::{fs, sync::Arc};

use miden_core::Felt;
use miden_mast_package::Package;
use midenc_frontend_wasm::WasmTranslationConfig;

use crate::{CompilerTest, sdk::note_script_program, testing::executor_with_std};

/// `entrypoint(3, 4, 5)` is `add_pair(Pair { a: 3, b: 4 }) + double(5)`, executed on the VM with
/// the `masm-dep` package the build compiled.
#[test]
fn rust_project_calls_its_masm_dependency_through_generated_bindings() {
    let mut test = CompilerTest::rust_source_cargo_miden(
        "../fixtures/masm-dependency-bindings",
        WasmTranslationConfig::default(),
        [],
    );
    let package = test.compile_package();

    // The dependency is a package of its own, linked dynamically: the executor needs it beside
    // the toolchain's. The build published it into its package cache, which lives as long as the
    // test's session.
    let cache = test
        .session
        .filesystem_package_cache_dir()
        .expect("the package exchange must be creatable")
        .expect("a Cargo Miden project must have a filesystem package cache");
    let dependency_path = cache.join("masm-dep.masp");
    let bytes = fs::read(&dependency_path).unwrap_or_else(|err| {
        panic!("failed to read the dependency package '{}': {err}", dependency_path.display())
    });
    let dependency = Package::read_from_bytes_trusted(&bytes).unwrap_or_else(|err| {
        panic!("failed to decode the dependency package '{}': {err}", dependency_path.display())
    });

    let args = [3, 4, 5].map(Felt::new_unchecked).to_vec();
    let mut exec = executor_with_std(args);
    exec.with_package(Arc::new(dependency))
        .expect("failed to register the dependency package");
    let result: Felt = exec.execute_into(package, test.session.source_manager.clone());
    assert_eq!(result.as_canonical_u64(), 3 + 4 + 2 * 5);
}

/// The same binding, in a Rust project built as another project's *dependency*.
///
/// A root Rust project and a dependency Rust project are compiled by different routes — the
/// dependency's through a nested session with no assembler of its own — and a binding must
/// resolve on both. `components/masm-dep-note` depends on `components/masm-dep-account`, an
/// account component whose `process-felt` calls `masm-dep`; building the note builds the account
/// through the dependency route, publishing it into the build's package cache. The note script
/// is run with the packages that build produced: a stub the dependency route failed to resolve
/// would trap in `process-felt` rather than return `11 + 42`.
#[test]
fn rust_dependency_calls_its_masm_dependency_through_generated_bindings() {
    let mut test = CompilerTest::rust_source_cargo_miden(
        "../fixtures/components/masm-dep-note",
        WasmTranslationConfig::default(),
        [],
    );
    let note = test.compile_package();
    assert!(note.is_library());

    let cache = test
        .session
        .filesystem_package_cache_dir()
        .expect("the package exchange must be creatable")
        .expect("a Cargo Miden project must have a filesystem package cache");
    let staged = |name: &str| {
        let path = cache.join(name);
        let bytes = fs::read(&path).unwrap_or_else(|err| {
            panic!("failed to read the staged package '{}': {err}", path.display())
        });
        Package::read_from_bytes_trusted(&bytes).unwrap_or_else(|err| {
            panic!("failed to decode the staged package '{}': {err}", path.display())
        })
    };

    let program = note_script_program(note);
    let mut exec = executor_with_std(vec![]);
    exec.with_package(Arc::new(staged("masm-dep-account.masp")))
        .expect("failed to register the account package");
    exec.with_package(Arc::new(staged("masm-dep.masp")))
        .expect("failed to register the MASM dependency package");
    let _trace = exec.execute(program, test.session.source_manager.clone());
}
