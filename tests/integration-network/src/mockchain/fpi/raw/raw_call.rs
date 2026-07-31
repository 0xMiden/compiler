//! Foreign procedure invocation tests for the raw SDK binding.

use std::sync::Arc;

use miden_client::{
    Word,
    account::{AccountComponent, component::BasicWallet},
    note::NoteTag,
    transaction::RawOutputNote,
};
use miden_mast_package::Package;
use miden_protocol::{
    account::{AccountBuilder, AccountComponentMetadata, AccountType, auth::AuthScheme},
    crypto::rand::RandomCoin,
};
use miden_standards::{
    account::auth::NoAuth, code_builder::CodeBuilder, testing::note::NoteBuilder,
};
use miden_testing::{AccountState, Auth, MockChain};
use midenc_integration_test_support::{compiler_test::sdk_crate_path, project};

use super::super::super::support::{
    compile_rust_package, execute_tx, note_script_root, to_core_felts,
};

/// MASM module name used by the raw FPI callee account component.
const RAW_CALLEE_MODULE: &str = "raw_fpi_callee";
/// Raw FPI callee procedure with the logical signature `() -> Felt`.
const NO_ARG_TO_FELT_PROC: &str = "no_arg_to_felt";
/// Raw FPI callee procedure with the logical signature `Word -> Felt`.
const WORD_TO_FELT_PROC: &str = "word_to_felt";
/// Raw FPI callee procedure with the logical signature `(16 felts) -> 16 felts`.
const SIXTEEN_FELTS_TO_SIXTEEN_FELTS_PROC: &str = "sixteen_felts_to_sixteen_felts";

/// Deploys a MASM account and consumes a note which calls a `() -> Felt` raw FPI procedure.
#[test]
pub fn no_arg_to_felt() {
    execute_raw_fpi_case("raw_no_arg_to_felt", NO_ARG_TO_FELT_PROC, raw_note_source_no_arg_to_felt);
}

/// Deploys a MASM account and consumes a note which calls a `Word -> Felt` raw FPI procedure.
#[test]
pub fn word_to_felt() {
    execute_raw_fpi_case("raw_word_to_felt", WORD_TO_FELT_PROC, raw_note_source_word_to_felt);
}

/// Deploys a MASM account and consumes a note which calls a full-slot raw FPI procedure.
#[test]
pub fn sixteen_felts_to_sixteen_felts() {
    execute_raw_fpi_case(
        "raw_sixteen_felts_to_sixteen_felts",
        SIXTEEN_FELTS_TO_SIXTEEN_FELTS_PROC,
        raw_note_source_sixteen_felts_to_sixteen_felts,
    );
}

/// Builds and executes a raw FPI note test against the requested MASM procedure.
fn execute_raw_fpi_case(
    test_name: &str,
    procedure_name: &str,
    note_source_for: fn(Word) -> String,
) {
    let (raw_component, procedure_root) = build_raw_fpi_component(procedure_name);
    let note_package = build_raw_fpi_note_package(test_name, &note_source_for(procedure_root));
    execute_raw_fpi_note(raw_component, note_package);
}

/// Builds the raw FPI callee account component and returns the selected procedure root.
fn build_raw_fpi_component(procedure_name: &str) -> (AccountComponent, Word) {
    let component_code = CodeBuilder::default()
        .compile_component_code(RAW_CALLEE_MODULE, RAW_CALLEE_ACCOUNT_SOURCE)
        .expect("failed to compile raw FPI callee account component");
    let procedure_path = format!("{RAW_CALLEE_MODULE}::{procedure_name}");
    let procedure_root = component_code
        .as_library()
        .get_procedure_root_by_path(procedure_path.as_str())
        .expect("failed to resolve raw FPI callee procedure root");
    let raw_component = AccountComponent::new(
        component_code,
        vec![],
        AccountComponentMetadata::new(RAW_CALLEE_MODULE),
    )
    .expect("failed to build raw FPI callee account component");

    (raw_component, procedure_root)
}

/// Builds the isolated note project for a raw FPI SDK binding test.
fn build_raw_fpi_note_package(test_name: &str, note_source: &str) -> Arc<Package> {
    let name = test_name.replace('_', "-");
    let note_name = format!("{name}-note");
    let note_package = format!("miden:{note_name}");

    let note_project = project(&note_name)
        .file(
            "miden-project.toml",
            &raw_fpi_note_miden_project_toml(&note_name, &note_package),
        )
        .file("Cargo.toml", &raw_fpi_note_cargo_toml(&note_name, &note_package))
        .file("src/lib.rs", note_source)
        .build();

    compile_rust_package(note_project.root(), true)
}

/// Returns the generated note project manifest for a raw FPI SDK binding test.
fn raw_fpi_note_miden_project_toml(note_name: &str, note_package: &str) -> String {
    let namespace = format!("{note_package}/miden-{note_name}@0.0.1");
    format!(
        r#"
[package]
name = "{note_name}"
version = "0.0.1"

[lib]
kind = "note"
namespace = "{namespace}"
path = "src/lib.rs"

[dependencies]
miden-core = "*"
miden-protocol = "*"
"#
    )
}

/// Deploys the raw callee account and consumes a note that calls it through FPI.
fn execute_raw_fpi_note(raw_component: AccountComponent, note_package: Arc<Package>) {
    let mut builder = MockChain::builder();
    let foreign_account = AccountBuilder::new([0_u8; 32])
        .account_type(AccountType::Public)
        .with_auth_component(NoAuth)
        .with_component(BasicWallet)
        .with_component(raw_component)
        .build_existing()
        .expect("failed to build raw FPI callee account");
    builder
        .add_account(foreign_account.clone())
        .expect("failed to add raw FPI callee account to mock chain builder");

    let caller_builder = AccountBuilder::new([1_u8; 32])
        .account_type(AccountType::Public)
        .with_component(BasicWallet);
    let caller_account = builder
        .add_account_from_builder(
            Auth::BasicAuth {
                auth_scheme: AuthScheme::Falcon512Poseidon2,
            },
            caller_builder,
            AccountState::Exists,
        )
        .expect("failed to add raw FPI caller account to mock chain builder");

    let rng = RandomCoin::new(note_script_root(note_package.as_ref()));
    let caller_note = NoteBuilder::new(caller_account.id(), rng)
        .package((*note_package).clone())
        .note_storage(to_core_felts(&foreign_account.id()))
        .unwrap()
        .tag(NoteTag::with_account_target(caller_account.id()).into())
        .build()
        .unwrap();
    builder.add_output_note(RawOutputNote::Full(caller_note.clone()));

    let mut chain = builder.build().expect("failed to build mock chain");
    chain.prove_next_block().unwrap();
    chain.prove_next_block().unwrap();

    let foreign_account_inputs = chain.get_foreign_account_inputs(foreign_account.id()).unwrap();
    let tx_context_builder = chain
        .build_tx_context(caller_account, &[caller_note.id()], &[])
        .unwrap()
        .foreign_accounts([foreign_account_inputs]);
    execute_tx(&mut chain, tx_context_builder);
}

/// Returns the generated note manifest for a raw FPI SDK binding test.
fn raw_fpi_note_cargo_toml(note_name: &str, note_package: &str) -> String {
    let sdk_path = sdk_crate_path();
    format!(
        r#"
[package]
name = "{note_name}"
version = "0.0.1"
edition = "2024"
authors = []

[lib]
crate-type = ["cdylib"]

[dependencies]
miden = {{ path = "{sdk_path}" }}

[package.metadata.miden]
project-kind = "note-script"

[package.metadata.component]
package = "{note_package}"

[profile.release]
opt-level = "z"
panic = "abort"
debug = false

[profile.dev]
panic = "abort"
opt-level = 1
debug-assertions = true
overflow-checks = false
debug = false
"#,
        sdk_path = sdk_path.display(),
        note_name = note_name,
        note_package = note_package,
    )
}

/// Builds the note source for a `() -> Felt` raw FPI call.
fn raw_note_source_no_arg_to_felt(procedure_root: Word) -> String {
    raw_note_source(
        procedure_root,
        "Checks that the raw SDK binding supports no user inputs and one meaningful output.",
        "tx::ForeignProcedureInputs::new([])",
        one_felt_output(101),
    )
}

/// Builds the note source for a `Word -> Felt` raw FPI call.
fn raw_note_source_word_to_felt(procedure_root: Word) -> String {
    raw_note_source(
        procedure_root,
        "Checks that the raw SDK binding forwards one flattened word input.",
        r#"{
            let key = Word::new([felt!(11), felt!(22), felt!(33), felt!(44)]);
            tx::ForeignProcedureInputs::new([key[0], key[1], key[2], key[3]])
        }"#,
        one_felt_output(202),
    )
}

/// Builds the note source for a `(16 felts) -> 16 felts` raw FPI call.
fn raw_note_source_sixteen_felts_to_sixteen_felts(procedure_root: Word) -> String {
    raw_note_source(
        procedure_root,
        "Checks that the raw SDK binding forwards and returns all supported raw slots.",
        r#"tx::ForeignProcedureInputs::new([
            felt!(1), felt!(2), felt!(3), felt!(4),
            felt!(5), felt!(6), felt!(7), felt!(8),
            felt!(9), felt!(10), felt!(11), felt!(12),
            felt!(13), felt!(14), felt!(15), felt!(16),
        ])"#,
        [17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32],
    )
}

/// Builds the note source that calls the raw SDK FPI binding.
fn raw_note_source(
    procedure_root: Word,
    note_doc: &str,
    inputs_source: &str,
    expected_outputs: [u64; 16],
) -> String {
    let root = procedure_root.iter().map(|felt| felt.as_canonical_u64()).collect::<Vec<_>>();
    let output_assertions = output_assertions(expected_outputs);

    format!(
        r#"
#![no_std]
#![feature(alloc_error_handler)]

use miden::*;

/// Note script input containing the raw foreign account id.
#[note]
struct RawFpiCaller {{
    /// Account id of the raw MASM account to invoke through FPI.
    foreign_account_id: AccountId,
}}

#[note]
impl RawFpiCaller {{
    /// {note_doc}
    #[note_script]
    pub fn run(self, _arg: Word) {{
        let procedure_root = Word::new([
            felt!({root_0}),
            felt!({root_1}),
            felt!({root_2}),
            felt!({root_3}),
        ]);
        let inputs = {inputs_source};
        let outputs = tx::execute_foreign_procedure(
            self.foreign_account_id,
            procedure_root,
            inputs,
        );

{output_assertions}
    }}
}}
"#,
        note_doc = note_doc,
        root_0 = root[0],
        root_1 = root[1],
        root_2 = root[2],
        root_3 = root[3],
        inputs_source = inputs_source,
        output_assertions = output_assertions,
    )
}

/// Returns expected raw FPI output slots with only the first felt populated.
fn one_felt_output(value: u64) -> [u64; 16] {
    let mut outputs = [0; 16];
    outputs[0] = value;
    outputs
}

/// Builds note-source assertions for all raw output slots.
fn output_assertions(expected_outputs: [u64; 16]) -> String {
    let mut assertions = String::new();
    for (index, expected_output) in expected_outputs.into_iter().enumerate() {
        assertions.push_str(&format!(
            "        assert_eq(outputs.get({index}), felt!({expected_output}));\n"
        ));
    }
    assertions
}

/// MASM account component used as a raw FPI callee.
const RAW_CALLEE_ACCOUNT_SOURCE: &str = r#"
use miden::core::sys

#! Logical signature: () -> Felt
#! The executor ABI still uses 16 raw input slots and 16 raw output slots.
#!
#! Inputs:  []
#! Outputs: [101]
pub proc no_arg_to_felt
    push.[0, 0, 0, 0]     assert_eqw.err="raw FPI callee: 0th input word is incorrect"
    push.[0, 0, 0, 0]     assert_eqw.err="raw FPI callee: 1st input word is incorrect"
    push.[0, 0, 0, 0]     assert_eqw.err="raw FPI callee: 2nd input word is incorrect"
    push.[0, 0, 0, 0]     assert_eqw.err="raw FPI callee: 3rd input word is incorrect"

    push.[0, 0, 0, 0] push.[0, 0, 0, 0]
    push.[0, 0, 0, 0] push.[0, 0, 0, 101]
    exec.sys::truncate_stack
end

#! Logical signature: Word -> Felt
#! The executor ABI still uses 16 raw input slots and 16 raw output slots.
#!
#! Inputs:  [Word(11, 22, 33, 44)]
#! Outputs: [202]
pub proc word_to_felt
    push.[44, 33, 22, 11] assert_eqw.err="raw FPI callee: 0th input word is incorrect"
    push.[0, 0, 0, 0]     assert_eqw.err="raw FPI callee: 1st input word is incorrect"
    push.[0, 0, 0, 0]     assert_eqw.err="raw FPI callee: 2nd input word is incorrect"
    push.[0, 0, 0, 0]     assert_eqw.err="raw FPI callee: 3rd input word is incorrect"

    push.[0, 0, 0, 0] push.[0, 0, 0, 0]
    push.[0, 0, 0, 0] push.[0, 0, 0, 202]
    exec.sys::truncate_stack
end

#! Logical signature: (16 felts) -> 16 felts
#! The executor ABI is fully populated for this call.
#!
#! Inputs:  [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16]
#! Outputs: [17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32]
pub proc sixteen_felts_to_sixteen_felts
    push.[4, 3, 2, 1]     assert_eqw.err="raw FPI callee: 0th input word is incorrect"
    push.[8, 7, 6, 5]     assert_eqw.err="raw FPI callee: 1st input word is incorrect"
    push.[12, 11, 10, 9]  assert_eqw.err="raw FPI callee: 2nd input word is incorrect"
    push.[16, 15, 14, 13] assert_eqw.err="raw FPI callee: 3rd input word is incorrect"

    push.[32, 31, 30, 29] push.[28, 27, 26, 25]
    push.[24, 23, 22, 21] push.[20, 19, 18, 17]
    exec.sys::truncate_stack
end
"#;
