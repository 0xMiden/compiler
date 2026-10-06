//! Output-note sealing tests.
//!
//! These tests run the `tests/fixtures/components/output-note-seal-tx-script` transaction script
//! against a basic-wallet account, so the `output_note::seal` and `output_note::is_sealed`
//! bindings execute against the live transaction kernel.

use miden_client::account::{AccountComponent, component::InitStorageData};
use miden_core::Felt;
use miden_protocol::{
    Word, account::auth::AuthScheme, errors::tx_kernel::ERR_OUTPUT_NOTE_IS_SEALED,
};
use miden_testing::{Auth, MockChain, MockTransaction};

use super::support::{
    compile_rust_package, execute_tx, execute_tx_expect_failure, transaction_script_from_package,
};

/// Builds a mock chain with a basic-wallet account and a transaction that runs the sealing
/// script with `script_arg` against it.
fn build_seal_tx(script_arg: Word) -> (MockChain, MockTransaction) {
    // The wallet is compiled first so that its package is available to the script, which depends
    // on it.
    let wallet_package = compile_rust_package("../../examples/basic-wallet", true);
    let tx_script_package =
        compile_rust_package("../fixtures/components/output-note-seal-tx-script", true);

    let wallet_component = AccountComponent::from_package(
        wallet_package.as_ref().clone(),
        &InitStorageData::default(),
    )
    .unwrap();

    let mut builder = MockChain::builder();
    let account = builder
        .add_existing_account_from_components(
            Auth::BasicAuth {
                auth_scheme: AuthScheme::Falcon512Poseidon2,
            },
            [wallet_component],
        )
        .unwrap();
    let account_id = account.id();

    let mut chain = builder.build().unwrap();
    chain.prove_next_block().unwrap();

    let mock_tx = chain
        .build_transaction(account_id)
        .tx_script(transaction_script_from_package(&tx_script_package))
        .tx_script_args(script_arg)
        .build()
        .unwrap();

    (chain, mock_tx)
}

/// A new output note reports as unsealed, and as sealed after one or more `seal` calls.
#[test]
pub fn sealing_an_output_note_is_observable_through_is_sealed() {
    let (mut chain, mock_tx) = build_seal_tx(Word::empty());

    let executed_tx = execute_tx(&mut chain, mock_tx);
    assert_eq!(executed_tx.output_notes().num_notes(), 1);
}

/// Adding an attachment to a sealed output note aborts the transaction in the kernel.
#[test]
pub fn a_sealed_output_note_rejects_a_new_attachment() {
    let (_chain, mock_tx) =
        build_seal_tx(Word::from([Felt::ONE, Felt::ZERO, Felt::ZERO, Felt::ZERO]));

    // Match the kernel's error message, so that a panic in one of the script's own asserts (which
    // reports the fixed guest panic code) cannot satisfy the test.
    let err = execute_tx_expect_failure(mock_tx);
    let needle =
        format!("assertion failed with error message: {}", ERR_OUTPUT_NOTE_IS_SEALED.message());
    assert!(err.contains(&needle), "unexpected failure message (wanted `{needle}`): {err}");
}
