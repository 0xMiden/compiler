use std::{fs, path::Path, sync::Arc};

use miden_assembly::ast::types::{FunctionType, Type};
use miden_core::serde::Serializable;
use miden_mast_package::{Package, PackageExport, ProcedureExport, QualifiedProcedureName};
use miden_protocol::note::NoteScript;
use midenc_frontend_wasm::WasmTranslationConfig;

use crate::{
    CompilerTest, CompilerTestBuilder,
    cargo_proj::project,
    compiler_test::{sdk_alloc_crate_path, sdk_crate_path},
    testing::executor_with_std,
};

mod base;
mod build_script;
mod canonabi;
mod macros;
mod note_script_root;
mod stdlib;

/// Rebuilds an executable program from a compiled note-script package for direct execution tests.
pub(crate) fn note_script_program(
    package: Arc<miden_mast_package::Package>,
) -> Arc<miden_mast_package::Package> {
    let note_script =
        NoteScript::from_package(&package).expect("compiled package should contain a note script");
    let entrypoint_id = note_script.entrypoint();
    let entrypoint = package.manifest.exports().find(|export| matches!(export.as_procedure(), Some(p) if p.node.is_some_and(|node| node == entrypoint_id))).unwrap();
    package
        .make_executable(&QualifiedProcedureName::from(entrypoint.path()))
        .map(Arc::new)
        .unwrap()
}

fn find_manifest_procedure<'a>(
    package: &'a miden_mast_package::Package,
    description: &str,
    mut predicate: impl FnMut(&str) -> bool,
) -> &'a ProcedureExport {
    let matches = package
        .manifest
        .exports()
        .filter_map(|export| export.as_procedure())
        .filter(|export| predicate(export.path.as_ref().as_str()))
        .collect::<Vec<_>>();
    assert_eq!(
        matches.len(),
        1,
        "expected exactly one manifest procedure matching {description}, got {:?}",
        package
            .manifest
            .exports()
            .filter_map(|export| match export {
                PackageExport::Procedure(export) => Some(export.path.as_ref().as_str().to_string()),
                PackageExport::Constant(_) | PackageExport::Type(_) => None,
            })
            .collect::<Vec<_>>(),
    );
    matches[0]
}

fn assert_export_signature<'a>(
    function: &'a ProcedureExport,
    expected_params: &[&str],
    expected_result: &str,
) -> &'a FunctionType {
    let signature = function.signature.as_ref().expect("procedure export should have a signature");
    let params = signature.params.iter().map(ToString::to_string).collect::<Vec<_>>();
    let expected_params = expected_params.iter().map(|param| param.to_string()).collect::<Vec<_>>();
    assert_eq!(params, expected_params);

    let result = match signature.results.as_slice() {
        [] => "void".to_string(),
        [result] => result.to_string(),
        results => {
            format!("({})", results.iter().map(ToString::to_string).collect::<Vec<_>>().join(", "))
        }
    };
    assert_eq!(result, expected_result);
    signature
}

fn assert_struct_field_types(ty: &Type, expected_fields: &[&str]) {
    let Type::Struct(struct_ty) = ty else {
        panic!("expected struct type, got {ty:?}");
    };
    let actual_fields =
        struct_ty.fields().iter().map(|field| field.ty.to_string()).collect::<Vec<_>>();
    let expected_fields = expected_fields.iter().map(|ty| ty.to_string()).collect::<Vec<_>>();
    assert_eq!(actual_fields, expected_fields);
}

fn assert_component_export_signatures_match_wit(package: &miden_mast_package::Package) {
    let component_export =
        find_manifest_procedure(package, "component export process-mixed", |name| {
            name.starts_with("::\"miden:cross-ctx-account-word/foo@1.0.0\"::")
                && name.ends_with("::\"process-mixed\"")
        });
    assert_eq!(
        component_export
            .signature
            .as_ref()
            .expect("component export should have a signature")
            .calling_convention()
            .as_str(),
        "component-model",
    );
    let felt_struct = "struct miden:base/core-types@1.0.0/felt {\n    inner : felt}";
    let compact_felt_struct = "struct miden:base/core-types@1.0.0/felt {inner : felt}";
    let mixed_struct = format!(
        concat!(
            "struct mixed-struct {{f : u64, a : {felt_struct}, b : u32, ",
            "c : {felt_struct}, d : u8, e : i1, g : u16}}"
        ),
        felt_struct = felt_struct
    );
    let signature = assert_export_signature(component_export, &[&mixed_struct], &mixed_struct);
    assert_struct_field_types(
        &signature.params[0],
        &["u64", compact_felt_struct, "u32", compact_felt_struct, "u8", "i1", "u16"],
    );
    assert_struct_field_types(
        &signature.results[0],
        &["u64", compact_felt_struct, "u32", compact_felt_struct, "u8", "i1", "u16"],
    );
}

/// Creates a generated workspace containing the existing basic-wallet/swapp-note FPI pair.
#[track_caller]
fn fpi_package_cache_regression_project() -> crate::Project {
    let original_swapp_note_source = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../fixtures/components/swapp-note/src/lib.rs"
    ));
    let swapp_note_mutation = "        let offered_asset = &note_assets[0];";
    assert_eq!(
        original_swapp_note_source.matches(swapp_note_mutation).count(),
        1,
        "the swapp-note fixture mutation must match exactly once"
    );
    let swapp_note_source = original_swapp_note_source.replacen(
        swapp_note_mutation,
        "        let offered_asset = &note_assets[0];\n        let foreign_wallet = \
         Wallet::new(self.creator);\n        foreign_wallet.receive_asset(*offered_asset);",
        1,
    );
    basic_wallet_swapp_note_project("fpi_package_cache_stale_root", &swapp_note_source, None)
}

/// Creates a generated workspace with the basic-wallet/swapp-note pair and optional build script.
#[track_caller]
fn basic_wallet_swapp_note_project(
    name: &str,
    swapp_note_source: &str,
    swapp_note_build_script: Option<&str>,
) -> crate::Project {
    let sdk_path = sdk_crate_path();
    let workspace_manifest = r#"
[workspace]
members = ["basic-wallet", "swapp-note"]
resolver = "3"

[profile.release]
opt-level = "z"
panic = "abort"
debug = false
"#;
    let basic_wallet_cargo = format!(
        r#"
cargo-features = ["trim-paths"]

[package]
name = "basic_wallet"
version = "0.1.0"
edition = "2024"

[lib]
crate-type = ["cdylib"]

[dependencies]
miden = {{ path = "{}" }}
"#,
        sdk_path.display(),
    );
    let swapp_note_cargo = format!(
        r#"
cargo-features = ["trim-paths"]

[package]
name = "swapp-note"
version = "0.1.0"
edition = "2024"

[lib]
crate-type = ["cdylib"]

[dependencies]
miden = {{ path = "{}" }}

[package.metadata.miden]
project-kind = "note-script"

[package.metadata.component]
package = "miden:swapp-note"

[package.metadata.miden.dependencies]
"miden:basic-wallet" = {{ path = "../basic-wallet" }}
"#,
        sdk_path.display(),
    );
    let swapp_note_miden_manifest = r#"
[package]
name = "swapp-note"
version = "0.1.0"

[lib]
kind = "note"
namespace = "miden:swapp-note/miden-swapp-note@0.1.0"
path = "src/lib.rs"

[dependencies]
basic-wallet = { path = "../basic-wallet" }
"#;
    let mut builder = project(name)
        .file("Cargo.toml", workspace_manifest)
        .file(
            ".cargo/config.toml",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../examples/basic-wallet/.cargo/config.toml"
            )),
        )
        .file("basic-wallet/Cargo.toml", &basic_wallet_cargo)
        .file(
            "basic-wallet/miden-project.toml",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../examples/basic-wallet/miden-project.toml"
            )),
        )
        .file(
            "basic-wallet/src/lib.rs",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../examples/basic-wallet/src/lib.rs"
            )),
        )
        .file("swapp-note/Cargo.toml", &swapp_note_cargo)
        .file("swapp-note/miden-project.toml", swapp_note_miden_manifest)
        .file("swapp-note/src/lib.rs", swapp_note_source);
    if let Some(build_script) = swapp_note_build_script {
        builder = builder.file("swapp-note/build.rs", build_script);
    }
    builder.build()
}

/// Reads the named dependency package from a compiled consumer's filesystem cache.
fn read_cached_dependency_package(test: &CompilerTest, package_name: &str) -> Package {
    let cache_dir = test
        .session
        .filesystem_package_cache_dir()
        .expect("a Cargo Miden project must have a filesystem package cache");
    let path = fs::read_dir(&cache_dir)
        .unwrap_or_else(|err| {
            panic!("failed to read package cache '{}': {err}", cache_dir.display())
        })
        .map(|entry| entry.expect("failed to read an entry from the package cache").path())
        .find(|path| {
            path.file_stem().is_some_and(|stem| stem == package_name)
                && path
                    .extension()
                    .is_some_and(|extension| extension.eq_ignore_ascii_case(Package::EXTENSION))
        })
        .unwrap_or_else(|| {
            panic!("failed to find package '{package_name}' in cache '{}'", cache_dir.display())
        });
    let bytes = fs::read(&path)
        .unwrap_or_else(|err| panic!("failed to read cached package '{}': {err}", path.display()));
    Package::read_from_bytes_unchecked(&bytes)
        .unwrap_or_else(|err| panic!("failed to decode cached package '{}': {err}", path.display()))
}

/// Returns the canonical integer representation of a procedure digest's field elements.
fn procedure_digest_felts(digest: &miden_core::Word) -> [u64; 4] {
    let elements = digest.as_elements();
    [
        elements[0].as_canonical_u64(),
        elements[1].as_canonical_u64(),
        elements[2].as_canonical_u64(),
        elements[3].as_canonical_u64(),
    ]
}

/// Returns true when MASM reconstructs every felt in `digest` from its decimal `u32` limbs.
///
/// This intentionally follows the current lowering shape for `u64` immediates. If codegen changes
/// that sequence, this helper can report a stale-root failure even when the embedded digest is
/// current, so update the recognizer alongside such a lowering change.
fn masm_contains_procedure_digest(masm: &str, digest: &miden_core::Word) -> bool {
    let pattern = procedure_digest_felts(digest)
        .into_iter()
        .rev()
        .map(|felt| {
            let low = felt as u32;
            let high = (felt >> u32::BITS) as u32;
            format!("push.{high} push.{low} swap.1 mul.4294967296 add")
        })
        .collect::<Vec<_>>()
        .join(" ");
    let normalized_masm = masm.split_whitespace().collect::<Vec<_>>().join(" ");
    normalized_masm.contains(&pattern)
}

/// Returns true when Wasm constructs every felt of `digest` for an FPI call.
///
/// The four root elements remain consecutive among the module's `i64.const` instructions even
/// when checked felt construction inserts control flow between them.
fn wat_contains_procedure_digest(wat: &str, digest: &miden_core::Word) -> bool {
    let tokens = wat.split_whitespace().collect::<Vec<_>>();
    let constants = tokens
        .windows(2)
        .filter(|tokens| tokens[0].trim_matches(['(', ')']) == "i64.const")
        .filter_map(|tokens| tokens[1].trim_matches(['(', ')']).parse::<i64>().ok())
        .collect::<Vec<_>>();
    let expected = procedure_digest_felts(digest).map(|felt| felt as i64);
    constants.windows(expected.len()).any(|window| window == expected)
}

/// Builds `consumer` directly with one prepopulated package cache and returns its Wasm as WAT.
fn build_consumer_wat_with_package_cache(
    consumer: &Path,
    cargo_target_dir: &Path,
    package_cache_dir: &Path,
) -> String {
    let output = std::process::Command::new("cargo")
        .args(["build", "--release", "--locked", "--manifest-path"])
        .arg(consumer.join("Cargo.toml"))
        .env("CARGO_TARGET_DIR", cargo_target_dir)
        .env("MIDENC_PACKAGE_CACHE", package_cache_dir)
        .env("RUSTFLAGS", "--cfg miden -C target-feature=+bulk-memory,+wide-arithmetic")
        .env_remove("CARGO_ENCODED_RUSTFLAGS")
        .current_dir(consumer)
        .output()
        .expect("failed to spawn Cargo for the option_env isolation fixture");
    assert!(
        output.status.success(),
        "option_env isolation fixture failed to build:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let wasm_path = cargo_target_dir.join("wasm32-wasip2/release/swapp_note.wasm");
    let wasm = fs::read(&wasm_path).unwrap_or_else(|err| {
        panic!("failed to read fixture Wasm '{}': {err}", wasm_path.display())
    });
    midenc_frontend_wasm::wasm_to_wat(&wasm).expect("failed to print fixture Wasm")
}

fn component_namespace(name: &str) -> String {
    let package = name.replace('_', "-");
    format!("miden:{package}/miden-{package}@0.0.1")
}

#[test]
fn rust_sdk_swapp_note_bindings() {
    let name = "rust_sdk_swapp_note_bindings";
    let namespace = component_namespace(name);
    let sdk_path = sdk_crate_path();
    let sdk_alloc_path = sdk_alloc_crate_path();
    let miden_project_toml = format!(
        r#"
        [package]
        name = "{name}"
        version = "0.0.1"

        [lib]
        kind = "note"
        namespace = "{namespace}"
        path = "src/lib.rs"
        "#
    );
    let cargo_toml = format!(
        r#"
[package]
name = "{name}"
version = "0.0.1"
edition = "2024"
authors = []

[lib]
crate-type = ["cdylib"]

[dependencies]
miden-sdk-alloc = {{ path = "{sdk_alloc_path}" }}
miden = {{ path = "{sdk_path}" }}

[profile.release]
opt-level = "z"
panic = "abort"
debug = false
"#,
        name = name,
        sdk_path = sdk_path.display(),
        sdk_alloc_path = sdk_alloc_path.display(),
    );

    let lib_rs = r#"#![no_std]
#![feature(alloc_error_handler)]

use miden::*;

#[note]
struct Note;

#[note]
impl Note {
    #[note_script]
    pub fn run(self, _arg: Word) {
        let sender = active_note::get_sender();
        let script_root = active_note::get_script_root();
        let serial_number = active_note::get_serial_number();
        let asset_key = Word::from([Felt::new(0).unwrap(); 4]);
        let asset_value = active_account::get_asset(asset_key);

        assert_eq!(sender.prefix, sender.prefix);
        assert_eq!(sender.suffix, sender.suffix);
        assert_eq!(script_root, script_root);
        assert_eq!(serial_number, serial_number);
        assert_eq!(asset_value, asset_value);
    }
}
"#;

    let cargo_proj = project(name)
        .file("miden-project.toml", &miden_project_toml)
        .file("Cargo.toml", &cargo_toml)
        .file("src/lib.rs", lib_rs)
        .build();

    let mut test = CompilerTestBuilder::rust_source_cargo_miden(
        cargo_proj.root(),
        WasmTranslationConfig::default(),
        [],
    )
    .build();

    // Ensure the crate compiles all the way to a package, exercising the bindings.
    test.compile_package();
}

/// Regression test for https://github.com/0xMiden/compiler/issues/831
///
/// Previously, compilation could panic during MASM codegen with:
/// `invalid stack offset for movup: 16 is out of range`.
#[test]
#[ignore = "https://github.com/0xMiden/compiler/issues/1120"]
fn rust_sdk_invalid_stack_offset_movup_16_issue_831() {
    let config = WasmTranslationConfig::default();
    let mut test = CompilerTest::rust_source_cargo_miden(
        "../fixtures/components/issue-invalid-stack-offset-movup",
        config,
        [],
    );

    // Ensure the crate compiles all the way to a package. This previously triggered the #831
    // panic in MASM codegen.
    let _package = test.compile_package();
}

#[test]
fn rust_sdk_cross_ctx_account_and_note() {
    let config = WasmTranslationConfig::default();
    let mut test = CompilerTest::rust_source_cargo_miden(
        "../fixtures/components/cross-ctx-account",
        config.clone(),
        [],
    );
    let account_package = test.compile_package();
    assert!(account_package.is_library());
    let exports = account_package
        .manifest
        .exports()
        .filter(|e| !e.path().as_ref().as_str().starts_with("intrinsics"))
        .map(|e| e.path().as_ref().as_str().to_string())
        .collect::<Vec<_>>();
    assert!(
        !account_package.manifest.exports().any(|export| export
            .path()
            .as_ref()
            .as_str()
            .starts_with("intrinsics")),
        "expected no intrinsics in the exports"
    );
    let expected_module_prefix = "::\"miden:cross-ctx-account/";
    let expected_function_suffix = "\"process-felt\"";
    assert!(
        exports.iter().any(|export| export.starts_with(expected_module_prefix)
            && export.ends_with(expected_function_suffix)),
        "expected one of the exports to start with '{expected_module_prefix}' and end with \
         '{expected_function_suffix}', got exports: {exports:?}"
    );
    // Test that the package loads
    let bytes = account_package.to_bytes();
    let loaded_package = miden_mast_package::Package::read_from_bytes_unchecked(&bytes).unwrap();
    assert_eq!(&account_package.manifest, &loaded_package.manifest);

    // Build counter note
    let builder = CompilerTestBuilder::rust_source_cargo_miden(
        "../fixtures/components/cross-ctx-note",
        config,
        [],
    );

    let mut test = builder.build();
    let package = test.compile_package();
    assert!(package.is_library());
    let program = note_script_program(package);
    let mut exec = executor_with_std(vec![]);
    exec.with_package(account_package).expect("failed to add account package");
    let _trace = exec.execute(program, test.session.source_manager.clone());
}

#[test]
fn rust_sdk_cross_ctx_account_and_note_word() {
    let config = WasmTranslationConfig::default();
    let mut test = CompilerTest::rust_source_cargo_miden(
        "../fixtures/components/cross-ctx-account-word",
        config.clone(),
        [],
    );
    let account_package = test.compile_package();
    assert!(account_package.is_library());
    assert_component_export_signatures_match_wit(account_package.as_ref());
    let expected_module_prefix = "::\"miden:cross-ctx-account-word/";
    let expected_function_suffix = "\"process-word\"";
    let exports = account_package
        .manifest
        .exports()
        .filter(|e| !e.path().as_ref().as_str().starts_with("intrinsics"))
        .map(|e| e.path().as_ref().as_str().to_string())
        .collect::<Vec<_>>();
    // dbg!(&exports);
    assert!(
        exports.iter().any(|export| export.starts_with(expected_module_prefix)
            && export.ends_with(expected_function_suffix)),
        "expected one of the exports to start with '{expected_module_prefix}' and end with \
         '{expected_function_suffix}', got exports: {exports:?}"
    );
    // Test that the package loads
    let bytes = account_package.to_bytes();
    let _loaded_package = miden_mast_package::Package::read_from_bytes_unchecked(&bytes).unwrap();

    // Build counter note
    let builder = CompilerTestBuilder::rust_source_cargo_miden(
        "../fixtures/components/cross-ctx-note-word",
        config,
        [],
    );

    let mut test = builder.build();
    let package = test.compile_package();
    assert!(package.is_library());
    let program = note_script_program(package.clone());
    let mut exec = executor_with_std(vec![]);
    exec.with_package(account_package).expect("failed to add account package");
    let _trace = exec.execute(program, test.session.source_manager.clone());
}

/// Regression test for https://github.com/0xMiden/compiler/issues/1257
///
/// Compiling the same account project several times must produce byte-identical package
/// artifacts. A non-deterministic build changes the package digest between compilations, so a
/// dependent package (e.g. a note script) records a dependency digest that no longer matches the
/// account package loaded into the executor's dependency resolver.
// This test compares the *MASM source* across five builds as well as the package digest, so that
// a digest mismatch can be localized ("identical MASM source, so the divergence is introduced at
// assembly").
//
// It was parked for the length of the pipeline redesign, because a Cargo fixture compiles through
// `RUST_FRONTEND`, whose route was `[package.assembled]` alone: the root target was built by
// recursing with its own `Session` and `Context`, so `masm.lowered` was never published to this
// run. The root arm now runs the shared WebAssembly tail in this process and publishes it. The
// MASM leg was deliberately never weakened to a digest-only comparison in the meantime, because
// it is what tells a future reader whether a non-determinism was introduced before or during
// assembly.
#[test]
fn rust_sdk_account_package_build_is_deterministic() {
    let config = WasmTranslationConfig::default();
    let mut baseline: Option<(miden_core::Word, Vec<u8>, String)> = None;
    for run in 0..5 {
        let mut test = CompilerTest::rust_source_cargo_miden(
            "../fixtures/components/cross-ctx-account-word",
            config.clone(),
            [],
        );
        let masm_src = test.masm_src();
        let package = test.compile_package();
        let digest = package.digest();
        let bytes = package.to_bytes();
        let Some((first_digest, first_bytes, first_masm)) = baseline.as_ref() else {
            baseline = Some((digest, bytes, masm_src));
            continue;
        };
        assert!(
            *first_masm == masm_src,
            "MASM source of compilation #{run} differs from compilation #0"
        );
        assert_eq!(
            *first_digest, digest,
            "MAST digest of compilation #{run} differs from compilation #0 (identical MASM \
             source, so the divergence is introduced at assembly)"
        );
        let first_mismatch = first_bytes
            .iter()
            .zip(bytes.iter())
            .position(|(first, current)| first != current);
        assert!(
            first_bytes.len() == bytes.len() && first_mismatch.is_none(),
            "package bytes of compilation #{run} differ from compilation #0: lengths {} vs {}, \
             first mismatch at offset {first_mismatch:?}",
            first_bytes.len(),
            bytes.len(),
        );
    }
}

/// A dependency package rewrite must invalidate the FPI roots embedded by `include_bytes!`.
#[test]
fn rust_sdk_fpi_reexpands_after_dependency_package_changes() {
    let project = fpi_package_cache_regression_project();
    let consumer = project.root().join("swapp-note");
    let dependency_source = project.root().join("basic-wallet/src/lib.rs");
    let dependency_cargo_manifest = project.root().join("basic-wallet/Cargo.toml");
    let dependency_miden_manifest = project.root().join("basic-wallet/miden-project.toml");
    let config = WasmTranslationConfig::default();

    let mut first_build = CompilerTest::rust_source_cargo_miden(&consumer, config.clone(), []);
    let first_masm = first_build.masm_src();
    let first_cache = first_build.session.filesystem_package_cache_dir().unwrap();
    let first_dependency = read_cached_dependency_package(&first_build, "basic-wallet");
    let first_export =
        find_manifest_procedure(&first_dependency, "basic-wallet receive-asset export", |path| {
            path.ends_with("::\"receive-asset\"")
        });
    let first_root = first_export.digest;
    assert!(
        masm_contains_procedure_digest(&first_masm, &first_root),
        "consumer MASM does not contain the first receive-asset root {:?}",
        procedure_digest_felts(&first_root),
    );

    let original_source = fs::read_to_string(&dependency_source).unwrap();
    assert_eq!(
        original_source.matches("        self.add_asset(asset);").count(),
        1,
        "fixture mutation must match exactly once"
    );
    let changed_source = original_source.replacen(
        "        self.add_asset(asset);",
        "        self.add_asset(asset);\n        self.remove_asset(asset);\n        \
         self.add_asset(asset);",
        1,
    );
    fs::write(&dependency_source, changed_source).unwrap();

    let mut second_build = CompilerTest::rust_source_cargo_miden(&consumer, config.clone(), []);
    let second_masm = second_build.masm_src();
    let second_cache = second_build.session.filesystem_package_cache_dir().unwrap();
    let second_dependency = read_cached_dependency_package(&second_build, "basic-wallet");
    let second_export =
        find_manifest_procedure(&second_dependency, "basic-wallet receive-asset export", |path| {
            path.ends_with("::\"receive-asset\"")
        });
    let second_root = second_export.digest;

    assert_eq!(first_cache, second_cache, "both builds must exercise the same cache path");
    assert_ne!(first_root, second_root, "the dependency implementation must change its root");
    assert_ne!(first_masm, second_masm, "the consumer must be recompiled with the new root");
    assert!(
        !masm_contains_procedure_digest(&second_masm, &first_root),
        "consumer MASM still contains the stale receive-asset root {:?}",
        procedure_digest_felts(&first_root),
    );
    assert!(
        masm_contains_procedure_digest(&second_masm, &second_root),
        "consumer MASM does not contain the new receive-asset root {:?}",
        procedure_digest_felts(&second_root),
    );

    for manifest_path in [&dependency_cargo_manifest, &dependency_miden_manifest] {
        let original_manifest = fs::read_to_string(manifest_path).unwrap();
        assert_eq!(
            original_manifest.matches("version = \"0.1.0\"").count(),
            1,
            "expected exactly one package-version field in {}",
            manifest_path.display()
        );
        let mut changed_manifest =
            original_manifest.replacen("version = \"0.1.0\"", "version = \"0.1.1\"", 1);
        if manifest_path == &dependency_miden_manifest {
            assert_eq!(
                changed_manifest.matches("@0.1.0").count(),
                1,
                "expected exactly one namespace version in {}",
                manifest_path.display()
            );
            changed_manifest = changed_manifest.replacen("@0.1.0", "@0.1.1", 1);
        }
        fs::write(manifest_path, changed_manifest).unwrap();
    }

    let mut third_build = CompilerTest::rust_source_cargo_miden(&consumer, config, []);
    let third_masm = third_build.masm_src();
    let third_cache = third_build.session.filesystem_package_cache_dir().unwrap();
    let third_dependency = read_cached_dependency_package(&third_build, "basic-wallet");
    let third_export =
        find_manifest_procedure(&third_dependency, "basic-wallet receive-asset export", |path| {
            path.ends_with("::\"receive-asset\"")
        });
    let third_root = third_export.digest;

    assert_ne!(second_cache, third_cache, "manifest changes must rotate the cache path");
    assert!(
        !second_cache.exists(),
        "the obsolete fingerprint directory must be pruned after rotation: {}",
        second_cache.display()
    );
    for stale_root in [first_root, second_root] {
        if stale_root != third_root {
            assert!(
                !masm_contains_procedure_digest(&third_masm, &stale_root),
                "consumer MASM still contains stale receive-asset root {:?}",
                procedure_digest_felts(&stale_root),
            );
        }
    }
    assert!(
        masm_contains_procedure_digest(&third_masm, &third_root),
        "consumer MASM does not contain the post-rotation receive-asset root {:?}",
        procedure_digest_felts(&third_root),
    );
}

/// Changing only `MIDENC_PACKAGE_CACHE` must re-expand FPI roots in an unchanged consumer.
#[test]
fn rust_sdk_fpi_reexpands_after_only_package_cache_env_changes() {
    let project = fpi_package_cache_regression_project();
    let dependency = project.root().join("basic-wallet");
    let consumer = project.root().join("swapp-note");
    let dependency_source = dependency.join("src/lib.rs");
    let config = WasmTranslationConfig::default();

    let mut first_dependency_build =
        CompilerTest::rust_source_cargo_miden(&dependency, config.clone(), []);
    let first_package = first_dependency_build.compile_package();
    let first_root = find_manifest_procedure(
        &first_package,
        "original basic-wallet receive-asset export",
        |path| path.ends_with("::\"receive-asset\""),
    )
    .digest;

    let original_source = fs::read_to_string(&dependency_source).unwrap();
    assert_eq!(
        original_source.matches("        self.add_asset(asset);").count(),
        1,
        "fixture mutation must match exactly once"
    );
    let changed_source = original_source.replacen(
        "        self.add_asset(asset);",
        "        self.add_asset(asset);\n        self.remove_asset(asset);\n        \
         self.add_asset(asset);",
        1,
    );
    fs::write(&dependency_source, changed_source).unwrap();

    let mut second_dependency_build =
        CompilerTest::rust_source_cargo_miden(&dependency, config, []);
    let second_package = second_dependency_build.compile_package();
    let second_root = find_manifest_procedure(
        &second_package,
        "changed basic-wallet receive-asset export",
        |path| path.ends_with("::\"receive-asset\""),
    )
    .digest;
    assert_ne!(first_root, second_root, "the prepopulated packages must embed different roots");

    let first_cache = project.root().join("option-env-cache-a");
    let second_cache = project.root().join("option-env-cache-b");
    for cache in [&first_cache, &second_cache] {
        fs::create_dir_all(cache).unwrap();
    }
    first_package
        .write_masp_file(&first_cache)
        .expect("failed to prepopulate the first package cache");
    second_package
        .write_masp_file(&second_cache)
        .expect("failed to prepopulate the second package cache");

    let cargo_target_dir = project.root().join("option-env-cargo-target");
    if cargo_target_dir.exists() {
        fs::remove_dir_all(&cargo_target_dir).unwrap();
    }

    // Both Cargo invocations have identical arguments, sources, manifests, generated WIT, target
    // directory, and flags. The cache environment value is the sole changed build input.
    let first_wat =
        build_consumer_wat_with_package_cache(&consumer, &cargo_target_dir, &first_cache);
    let second_wat =
        build_consumer_wat_with_package_cache(&consumer, &cargo_target_dir, &second_cache);

    assert!(
        wat_contains_procedure_digest(&first_wat, &first_root),
        "the first consumer build did not embed its cache's receive-asset root"
    );
    assert!(
        !wat_contains_procedure_digest(&first_wat, &second_root),
        "the first consumer build unexpectedly embedded the second cache's root"
    );
    assert!(
        wat_contains_procedure_digest(&second_wat, &second_root),
        "changing MIDENC_PACKAGE_CACHE did not re-expand the consumer with the second root"
    );
    assert!(
        !wat_contains_procedure_digest(&second_wat, &first_root),
        "the second consumer build retained the stale root from the first cache"
    );
}

#[test]
fn rust_sdk_cross_ctx_word_arg_account_and_note() {
    let config = WasmTranslationConfig::default();
    let mut test = CompilerTest::rust_source_cargo_miden(
        "../fixtures/components/cross-ctx-account-word-arg",
        config.clone(),
        [],
    );
    let account_package = test.compile_package();
    assert!(account_package.is_library());
    let expected_module_prefix = "::\"miden:cross-ctx-account-word-arg/";
    let expected_function_suffix = "\"process-word\"";
    let exports = account_package
        .manifest
        .exports()
        .filter(|e| !e.path().as_ref().as_str().starts_with("intrinsics"))
        .map(|e| e.path().as_ref().as_str().to_string())
        .collect::<Vec<_>>();
    assert!(
        exports.iter().any(|export| export.starts_with(expected_module_prefix)
            && export.ends_with(expected_function_suffix)),
        "expected one of the exports to start with '{expected_module_prefix}' and end with \
         '{expected_function_suffix}', got exports: {exports:?}"
    );

    // Build counter note
    let builder = CompilerTestBuilder::rust_source_cargo_miden(
        "../fixtures/components/cross-ctx-note-word-arg",
        config,
        [],
    );
    let mut test = builder.build();
    let package = test.compile_package();
    assert!(package.is_library());
    let program = note_script_program(package.clone());
    let mut exec = executor_with_std(vec![]);
    exec.with_package(account_package).expect("failed to add account package");
    let _trace = exec.execute(program, test.session.source_manager.clone());
}
