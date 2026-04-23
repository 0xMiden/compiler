use std::{collections::BTreeMap, env, path::PathBuf};

use miden_core::{
    Felt, Word,
    program::Program,
    serde::{Deserializable, Serializable},
};
use miden_protocol::{
    account::{AccountComponentMetadata, component::InitStorageData},
    note::NoteScript,
};
use midenc_expect_test::expect_file;
use midenc_frontend_wasm::WasmTranslationConfig;
use midenc_hir::{FunctionIdent, Ident, SourceSpan, interner::Symbol};
use midenc_session::STDLIB;

use crate::{
    CompilerTest, CompilerTestBuilder,
    cargo_proj::project,
    compiler_test::{sdk_alloc_crate_path, sdk_crate_path},
    testing::{self, executor_with_std},
};

mod base;
mod macros;
mod stdlib;

/// Rebuilds an executable program from a compiled note-script package for direct execution tests.
fn note_script_program(package: &miden_mast_package::Package) -> Program {
    let note_script =
        NoteScript::from_package(package).expect("compiled package should contain a note script");
    Program::new(note_script.mast(), note_script.entrypoint())
}

#[test]
fn rust_sdk_swapp_note_bindings() {
    let name = "rust_sdk_swapp_note_bindings";
    let sdk_path = sdk_crate_path();
    let sdk_alloc_path = sdk_alloc_crate_path();
    let component_package = format!("miden:{}", name.replace('_', "-"));
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

[package.metadata.component]
package = "{component_package}"

[package.metadata.miden]
project-kind = "note-script"

[profile.release]
opt-level = "z"
panic = "abort"
debug = false
"#,
        name = name,
        sdk_path = sdk_path.display(),
        sdk_alloc_path = sdk_alloc_path.display(),
        component_package = component_package,
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
        let balance = active_account::get_balance(sender);

        assert_eq!(sender.prefix, sender.prefix);
        assert_eq!(sender.suffix, sender.suffix);
        assert_eq!(script_root, script_root);
        assert_eq!(serial_number, serial_number);
        assert_eq!(balance, balance);
    }
}
"#;

    let cargo_proj =
        project(name).file("Cargo.toml", &cargo_toml).file("src/lib.rs", lib_rs).build();

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
fn rust_sdk_invalid_stack_offset_movup_16_issue_831() {
    let config = WasmTranslationConfig::default();
    let mut test = CompilerTest::rust_source_cargo_miden(
        "../rust-apps-wasm/rust-sdk/issue-invalid-stack-offset-movup",
        config,
        [],
    );

    // Ensure the crate compiles all the way to a package. This previously triggered the #831
    // panic in MASM codegen.
    let package = test.compile_package();
}

/// Regression test for https://github.com/0xMiden/compiler/issues/1084
///
/// Packaging an account component with many branch-producing operations in one function currently
/// panics in `SimplifySwitchFallbackOverlap`.
#[test]
fn rust_sdk_switch_fallback_overlap_issue_1084() {
    let name = "rust_sdk_switch_fallback_overlap_issue";
    let component_package = format!("miden:{}", name.replace('_', "-"));
    let cargo_toml = format!(
        r#"
[package]
name = "{name}"
version = "0.0.1"
edition = "2021"
authors = []

[lib]
crate-type = ["cdylib"]

[dependencies]
miden = "=0.12.0"

[package.metadata.component]
package = "{component_package}"

[package.metadata.miden]
project-kind = "account"
supported-types = ["RegularAccountImmutableCode"]
"#,
        name = name,
        component_package = component_package,
    );

    let lib_rs = r#"#![no_std]
#![feature(alloc_error_handler)]

use miden::*;

const PHASE_ACTIVE: u64 = 2;
const PHASE_REVEAL: u64 = 3;
const CELL_HIT: u64 = 6;
const CELL_MISS: u64 = 7;
const TOTAL_SHIP_CELLS: u64 = 17;
const GRID_SIZE: u64 = 10;

#[component]
struct TestAccount {
    #[storage(description = "game config")]
    game_config: StorageValue<Word>,

    #[storage(description = "opponent info")]
    opponent: StorageValue<Word>,

    #[storage(description = "board cells")]
    my_board: StorageMap<Word, Felt>,
}

#[component]
impl TestAccount {
    pub fn process_shot(&mut self, row: Felt, col: Felt, turn: Felt) -> Felt {
        let config: Word = self.game_config.get();
        assert!(config[2].as_canonical_u64() == PHASE_ACTIVE, "a1");
        assert!(turn.as_canonical_u64() == config[3].as_canonical_u64(), "a2");
        assert!(row.as_canonical_u64() < GRID_SIZE, "a3");
        assert!(col.as_canonical_u64() < GRID_SIZE, "a4");

        let key = Word::from([felt!(0), felt!(0), row, col]);
        let cell: Felt = self.my_board.get(key);
        let cell_val = cell.as_canonical_u64();
        assert!(cell_val != CELL_HIT, "a5");
        assert!(cell_val != CELL_MISS, "a6");

        let opp: Word = self.opponent.get();
        let ships_hit_count = opp[2].as_canonical_u64();
        let total_shots = opp[3].as_canonical_u64();

        let is_hit = cell_val >= 1 && cell_val <= 5;
        let result: u64 = if is_hit { 1 } else { 0 };
        let new_cell: u64 = if is_hit { CELL_HIT } else { CELL_MISS };
        let new_hit_count: u64 = if is_hit {
            ships_hit_count + 1
        } else {
            ships_hit_count
        };

        self.my_board.set(key, Felt::new(new_cell));
        self.opponent.set(Word::from([
            opp[0],
            opp[1],
            Felt::new(new_hit_count),
            Felt::new(total_shots + 1),
        ]));

        let game_over: u64 = if new_hit_count == TOTAL_SHIP_CELLS { 1 } else { 0 };
        let new_phase: u64 = if game_over == 1 {
            PHASE_REVEAL
        } else {
            PHASE_ACTIVE
        };

        self.game_config.set(Word::from([
            Felt::new(GRID_SIZE),
            Felt::new(TOTAL_SHIP_CELLS),
            Felt::new(new_phase),
            Felt::new(config[3].as_canonical_u64() + 2),
        ]));

        Felt::new(result * 2 + game_over)
    }
}
"#;

    let cargo_proj =
        project(name).file("Cargo.toml", &cargo_toml).file("src/lib.rs", lib_rs).build();

    let mut test = CompilerTestBuilder::rust_source_cargo_miden(
        cargo_proj.root(),
        WasmTranslationConfig::default(),
        [],
    )
    .build();

    let account_package = test.compile_package();
    assert!(account_package.is_library());
}

#[test]
fn rust_sdk_cross_ctx_account_and_note() {
    let config = WasmTranslationConfig::default();
    let mut test = CompilerTest::rust_source_cargo_miden(
        "../rust-apps-wasm/rust-sdk/cross-ctx-account",
        config.clone(),
        [],
    );
    let account_package = test.compile_package();
    assert!(account_package.is_library());
    let lib = account_package.mast.clone();
    let exports = lib
        .exports()
        .filter(|e| !e.path().as_ref().as_str().starts_with("intrinsics"))
        .map(|e| e.path().as_ref().as_str().to_string())
        .collect::<Vec<_>>();
    assert!(
        !lib.exports()
            .any(|export| export.path().as_ref().as_str().starts_with("intrinsics")),
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
    let loaded_package = miden_mast_package::Package::read_from_bytes(&bytes).unwrap();

    // Build counter note
    let builder = CompilerTestBuilder::rust_source_cargo_miden(
        "../rust-apps-wasm/rust-sdk/cross-ctx-note",
        config,
        [],
    );

    let mut test = builder.build();
    let package = test.compile_package();
    assert!(package.is_library());
    let program = note_script_program(package.as_ref());
    let mut exec = executor_with_std(vec![], None);
    exec.dependency_resolver_mut()
        .insert(*account_package.mast.digest(), account_package.mast.clone());
    exec.with_dependencies(package.manifest.dependencies())
        .expect("failed to add package dependencies");
    let trace = exec.execute(&program, test.session.source_manager.clone());
}

#[test]
fn rust_sdk_cross_ctx_account_and_note_word() {
    let config = WasmTranslationConfig::default();
    let mut test = CompilerTest::rust_source_cargo_miden(
        "../rust-apps-wasm/rust-sdk/cross-ctx-account-word",
        config.clone(),
        [],
    );
    let account_package = test.compile_package();
    assert!(account_package.is_library());
    let lib = account_package.mast.clone();
    let expected_module_prefix = "::\"miden:cross-ctx-account-word/";
    let expected_function_suffix = "\"process-word\"";
    let exports = lib
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
    let loaded_package = miden_mast_package::Package::read_from_bytes(&bytes).unwrap();

    // Build counter note
    let builder = CompilerTestBuilder::rust_source_cargo_miden(
        "../rust-apps-wasm/rust-sdk/cross-ctx-note-word",
        config,
        [],
    );

    let mut test = builder.build();
    let package = test.compile_package();
    assert!(package.is_library());
    let program = note_script_program(package.as_ref());
    let mut exec = executor_with_std(vec![], None);
    exec.dependency_resolver_mut()
        .insert(*account_package.mast.digest(), account_package.mast.clone());
    exec.with_dependencies(package.manifest.dependencies())
        .expect("failed to add package dependencies");
    let trace = exec.execute(&program, test.session.source_manager.clone());
}

#[test]
fn rust_sdk_cross_ctx_word_arg_account_and_note() {
    let config = WasmTranslationConfig::default();
    let mut test = CompilerTest::rust_source_cargo_miden(
        "../rust-apps-wasm/rust-sdk/cross-ctx-account-word-arg",
        config.clone(),
        [],
    );
    let account_package = test.compile_package();

    assert!(account_package.is_library());
    let lib = account_package.mast.clone();
    let expected_module_prefix = "::\"miden:cross-ctx-account-word-arg/";
    let expected_function_suffix = "\"process-word\"";
    let exports = lib
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
        "../rust-apps-wasm/rust-sdk/cross-ctx-note-word-arg",
        config,
        [],
    );
    let mut test = builder.build();
    let package = test.compile_package();
    assert!(package.is_library());
    let program = note_script_program(package.as_ref());
    let mut exec = executor_with_std(vec![], None);
    exec.dependency_resolver_mut()
        .insert(*account_package.mast.digest(), account_package.mast.clone());
    exec.with_dependencies(package.manifest.dependencies())
        .expect("failed to add package dependencies");
    let trace = exec.execute(&program, test.session.source_manager.clone());
}
