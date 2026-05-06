//! Foreign procedure invocation tests on a mock chain.

use miden_client::{
    account::{AccountComponent, component::InitStorageData},
    note::NoteTag,
    transaction::RawOutputNote,
};
use miden_protocol::{
    account::{AccountBuilder, AccountStorageMode, AccountType, auth::AuthScheme},
    crypto::rand::RandomCoin,
};
use miden_standards::testing::note::NoteBuilder;
use miden_testing::{AccountState, Auth, MockChain};

use super::support::{
    COUNTER_CONTRACT_STORAGE_KEY, assert_counter_storage,
    build_existing_counter_account_builder_with_auth_package, compile_rust_package,
    counter_storage_slot_name, execute_tx, note_script_root, to_core_felts,
};

/// Deploys a counter contract and consumes a note which reads it through FPI.
#[test]
pub fn counter_caller_note_reads_counter_through_fpi() {
    let counter_package = compile_rust_package("../../examples/counter-contract", true);
    let caller_note_package = compile_rust_package("../../examples/counter-caller", true);
    let no_auth_auth_component =
        compile_rust_package("../../examples/auth-component-no-auth", true);

    let counter_storage_slot = counter_storage_slot_name();
    let counter_component = {
        let mut init_storage_data = InitStorageData::default();
        init_storage_data
            .insert_map_entry(counter_storage_slot.clone(), COUNTER_CONTRACT_STORAGE_KEY, 42_u64)
            .unwrap();
        AccountComponent::from_package(&counter_package, &init_storage_data).unwrap()
    };

    let mut builder = MockChain::builder();
    let counter_account = build_existing_counter_account_builder_with_auth_package(
        counter_component,
        no_auth_auth_component,
        vec![],
        [0_u8; 32],
    )
    .build_existing()
    .expect("failed to build counter account");
    builder
        .add_account(counter_account.clone())
        .expect("failed to add counter account to mock chain builder");

    let caller_builder = AccountBuilder::new([1_u8; 32])
        .account_type(AccountType::RegularAccountUpdatableCode)
        .storage_mode(AccountStorageMode::Public)
        .with_component(miden_client::account::component::BasicWallet);
    let caller_account = builder
        .add_account_from_builder(
            Auth::BasicAuth {
                auth_scheme: AuthScheme::Falcon512Poseidon2,
            },
            caller_builder,
            AccountState::Exists,
        )
        .expect("failed to add caller account to mock chain builder");

    let rng = RandomCoin::new(note_script_root(caller_note_package.as_ref()));
    let caller_note = NoteBuilder::new(caller_account.id(), rng)
        .package((*caller_note_package).clone())
        .note_storage(to_core_felts(&counter_account.id()))
        .unwrap()
        .tag(NoteTag::with_account_target(caller_account.id()).into())
        .build()
        .unwrap();
    builder.add_output_note(RawOutputNote::Full(caller_note.clone()));

    let mut chain = builder.build().expect("failed to build mock chain");
    chain.prove_next_block().unwrap();
    chain.prove_next_block().unwrap();

    assert_counter_storage(
        chain.committed_account(counter_account.id()).unwrap().storage(),
        &counter_storage_slot,
        42,
    );

    let foreign_account_inputs = chain.get_foreign_account_inputs(counter_account.id()).unwrap();
    let tx_context_builder = chain
        .build_tx_context(caller_account.clone(), &[caller_note.id()], &[])
        .unwrap()
        .foreign_accounts([foreign_account_inputs]);
    execute_tx(&mut chain, tx_context_builder);

    assert_counter_storage(
        chain.committed_account(counter_account.id()).unwrap().storage(),
        &counter_storage_slot,
        42,
    );
}
