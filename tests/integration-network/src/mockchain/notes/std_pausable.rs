//! A Rust transaction script depending on two standard (MASM) components whose namespaces nest.
//!
//! `pausable` (`miden::standards::components::access::pausable`) and `pausable-manager`
//! (`miden::standards::components::access::pausable::manager`) are both dependencies of the
//! script, so their import stubs nest in the consumer's world.

use miden_protocol::account::auth::AuthScheme;
use miden_standards::account::access::{Authority, Pausable, PausableManager};
use miden_testing::{Auth, MockChain};
use midenc_expect_test::expect;

use super::super::support::{
    compile_rust_package, execute_tx_measurements, transaction_script_from_package,
    tx_script_processing_cycles,
};

/// A Rust transaction script reads the paused state through the standard `pausable` component,
/// pauses the account through the standard `pausable-manager` component and reads the state
/// again.
#[test]
pub fn std_pausable_and_manager_pause_the_account() {
    let tx_script_package =
        compile_rust_package("../fixtures/components/std-pausable-tx-script", true);

    let mut builder = MockChain::builder();
    // `Authority::AuthControlled` lets `pause` through once the account's auth component has
    // authenticated the transaction.
    let account_id = builder
        .add_existing_account_from_components(
            Auth::BasicAuth {
                auth_scheme: AuthScheme::Falcon512Poseidon2,
            },
            [
                Authority::AuthControlled.into(),
                Pausable::unpaused().into(),
                PausableManager.into(),
            ],
        )
        .unwrap()
        .id();
    let mut chain = builder.build().unwrap();
    chain.prove_next_block().unwrap();

    let mock_tx = chain
        .build_transaction(account_id)
        .tx_script(transaction_script_from_package(&tx_script_package))
        .build()
        .unwrap();
    let measurements = execute_tx_measurements(&mut chain, mock_tx);
    expect!["2359"].assert_eq(tx_script_processing_cycles(&measurements));
}
