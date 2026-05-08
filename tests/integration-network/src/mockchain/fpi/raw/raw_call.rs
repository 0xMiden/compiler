//! Foreign procedure invocation tests for the raw SDK binding.

use std::sync::Arc;

use miden_client::{
    account::{AccountComponent, component::BasicWallet},
    note::NoteTag,
    transaction::RawOutputNote,
};
use miden_mast_package::Package;
use miden_protocol::{
    account::{
        AccountBuilder, AccountComponentMetadata, AccountStorageMode, AccountType, auth::AuthScheme,
    },
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

/// Deploys a MASM account and consumes a note which calls it through the raw SDK FPI binding.
#[test]
pub fn raw_call() {
    let component_code = CodeBuilder::default()
        .compile_component_code("raw_fpi_callee", RAW_CALLEE_ACCOUNT_SOURCE)
        .expect("failed to compile raw FPI callee account component");
    let procedure_root = component_code
        .as_library()
        .get_procedure_root_by_path("raw_fpi_callee::assert_inputs_correctness")
        .expect("failed to resolve raw FPI callee procedure root");
    let raw_component = AccountComponent::new(
        component_code,
        vec![],
        AccountComponentMetadata::new("raw_fpi_callee", [AccountType::RegularAccountUpdatableCode]),
    )
    .expect("failed to build raw FPI callee account component");

    let note_package = build_raw_fpi_note_package("raw_call", &raw_note_source(procedure_root));
    execute_raw_fpi_note(raw_component, note_package);
}

/// Builds the isolated note project for the raw FPI SDK binding test.
fn build_raw_fpi_note_package(test_name: &str, note_source: &str) -> Arc<Package> {
    let name = test_name.replace('_', "-");
    let note_name = format!("{name}-note");
    let note_package = format!("miden:{note_name}");

    let note_project = project(&note_name)
        .file("Cargo.toml", &raw_fpi_note_cargo_toml(&note_name, &note_package))
        .file("src/lib.rs", note_source)
        .build();

    compile_rust_package(note_project.root(), true)
}

/// Deploys the raw callee account and consumes a note that calls it through FPI.
fn execute_raw_fpi_note(raw_component: AccountComponent, note_package: Arc<Package>) {
    let mut builder = MockChain::builder();
    let foreign_account = AccountBuilder::new([0_u8; 32])
        .account_type(AccountType::RegularAccountUpdatableCode)
        .storage_mode(AccountStorageMode::Public)
        .with_auth_component(NoAuth)
        .with_component(BasicWallet)
        .with_component(raw_component)
        .build_existing()
        .expect("failed to build raw FPI callee account");
    builder
        .add_account(foreign_account.clone())
        .expect("failed to add raw FPI callee account to mock chain builder");

    let caller_builder = AccountBuilder::new([1_u8; 32])
        .account_type(AccountType::RegularAccountUpdatableCode)
        .storage_mode(AccountStorageMode::Public)
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

/// Builds the note source that calls the raw SDK FPI binding.
fn raw_note_source(procedure_root: miden_client::Word) -> String {
    let root = procedure_root.iter().map(|felt| felt.as_canonical_u64()).collect::<Vec<_>>();

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
    /// Checks that the raw SDK binding forwards inputs, zero-pads them, and returns outputs.
    #[note_script]
    pub fn run(self, _arg: Word) {{
        let procedure_root = Word::new([
            felt!({root_0}),
            felt!({root_1}),
            felt!({root_2}),
            felt!({root_3}),
        ]);
        let inputs = tx::ForeignProcedureInputs::new([
            felt!(1), felt!(2), felt!(3), felt!(4),
            felt!(5), felt!(6),
        ]);
        let outputs = tx::execute_foreign_procedure(
            self.foreign_account_id,
            procedure_root,
            inputs,
        );

        assert_eq(outputs.get(0), felt!(17));
        assert_eq(outputs.get(1), felt!(18));
        assert_eq(outputs.get(2), felt!(19));
        assert_eq(outputs.get(3), felt!(20));
        assert_eq(outputs.get(4), felt!(21));
        assert_eq(outputs.get(5), felt!(22));
        assert_eq(outputs.get(6), felt!(23));
        assert_eq(outputs.get(7), felt!(24));
        assert_eq(outputs.get(8), felt!(25));
        assert_eq(outputs.get(9), felt!(26));
        assert_eq(outputs.get(10), felt!(27));
        assert_eq(outputs.get(11), felt!(28));
        assert_eq(outputs.get(12), felt!(29));
        assert_eq(outputs.get(13), felt!(30));
        assert_eq(outputs.get(14), felt!(31));
        assert_eq(outputs.get(15), felt!(32));
    }}
}}
"#,
        root_0 = root[0],
        root_1 = root[1],
        root_2 = root[2],
        root_3 = root[3],
    )
}

/// MASM account component used as a raw FPI callee.
const RAW_CALLEE_ACCOUNT_SOURCE: &str = r#"
use miden::core::sys

#! Validates the 16 raw input felts and returns 16 raw output felts.
#!
#! Inputs:  [1, 2, 3, 4, 5, 6, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]
#! Outputs: [17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32]
pub proc assert_inputs_correctness
    push.[4, 3, 2, 1]     assert_eqw.err="raw FPI callee: 0th input word is incorrect"
    push.[0, 0, 6, 5]     assert_eqw.err="raw FPI callee: 1st input word is incorrect"
    push.[0, 0, 0, 0]     assert_eqw.err="raw FPI callee: 2nd input word is incorrect"
    push.[0, 0, 0, 0]     assert_eqw.err="raw FPI callee: 3rd input word is incorrect"

    push.[32, 31, 30, 29] push.[28, 27, 26, 25]
    push.[24, 23, 22, 21] push.[20, 19, 18, 17]
    exec.sys::truncate_stack
end
"#;
