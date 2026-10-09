//! Rust transaction scripts reading multi-element results from the standard (MASM) components.
//!
//! Each script calls a standard getter whose result occupies several stack elements and asserts
//! every field against the values the host configured the account with, which pins the order in
//! which a MASM callee's results come back.

use miden_core::Felt;
use miden_field_repr::{FromFeltRepr, ToFeltRepr};
use miden_protocol::{
    account::{AccountId, auth::AuthScheme},
    asset::TokenSymbol,
};
use miden_standards::account::access::Ownable2Step;
use miden_testing::{Auth, MockChain};
use midenc_expect_test::expect;

use super::super::support::{
    apply_script_args, compile_rust_package, execute_tx_measurements, to_field_felt,
    transaction_script_from_package, tx_script_processing_cycles,
};

/// Host-side mirror of `TxScriptArgs` in `tests/fixtures/components/std-faucet-config-tx-script`.
#[derive(FromFeltRepr, ToFeltRepr)]
struct FaucetConfigArgs {
    supply: miden_field::Felt,
    max_supply: miden_field::Felt,
    decimals: u8,
    symbol: miden_field::Felt,
}

/// Host-side mirror of `TxScriptArgs` in `tests/fixtures/components/std-owner-tx-script`.
#[derive(FromFeltRepr, ToFeltRepr)]
struct OwnerArgs {
    owner_suffix: miden_field::Felt,
    owner_prefix: miden_field::Felt,
}

/// The standard authentication of the accounts the scripts run on.
fn auth() -> Auth {
    Auth::BasicAuth {
        auth_scheme: AuthScheme::Falcon512Poseidon2,
    }
}

/// A Rust transaction script reads the four-field `token-config` record of a standard fungible
/// faucet and finds every field where the faucet stores it.
#[test]
pub fn std_faucet_get_token_config_returns_the_configured_fields() {
    let tx_script_package =
        compile_rust_package("../fixtures/components/std-faucet-config-tx-script", true);

    // Distinct values for every field, so a swapped pair cannot pass; the decimals are
    // `miden-testing`'s default faucet decimals.
    let (max_supply, supply, decimals) = (1_000_000_000u64, 12_345u64, 10u8);
    let symbol = Felt::from(TokenSymbol::new("TEST").unwrap());
    let mut builder = MockChain::builder();
    let faucet_id = builder
        .add_existing_basic_faucet(auth(), "TEST", max_supply, Some(supply))
        .unwrap()
        .id();
    let mut chain = builder.build().unwrap();
    chain.prove_next_block().unwrap();

    let args = FaucetConfigArgs {
        supply: to_field_felt(Felt::new_unchecked(supply)),
        max_supply: to_field_felt(Felt::new_unchecked(max_supply)),
        decimals,
        symbol: to_field_felt(symbol),
    };
    let mock_tx_builder = chain
        .build_transaction(faucet_id)
        .tx_script(transaction_script_from_package(&tx_script_package));
    let mock_tx = apply_script_args(mock_tx_builder, &args).build().unwrap();
    let measurements = execute_tx_measurements(&mut chain, mock_tx);
    expect!["1759"].assert_eq(tx_script_processing_cycles(&measurements));
}

/// A Rust transaction script reads the two-field `account-id` record of a standard
/// `ownable2step` component and finds the owner's suffix and prefix where the component stores
/// them.
#[test]
pub fn std_ownable2step_get_owner_returns_the_configured_owner() {
    let tx_script_package =
        compile_rust_package("../fixtures/components/std-owner-tx-script", true);

    let mut builder = MockChain::builder();
    // Any other account serves as the owner.
    let owner_id: AccountId = builder.add_existing_wallet(auth()).unwrap().id();
    let account_id = builder
        .add_existing_account_from_components(auth(), [Ownable2Step::new(owner_id).into()])
        .unwrap()
        .id();
    let mut chain = builder.build().unwrap();
    chain.prove_next_block().unwrap();

    let args = OwnerArgs {
        owner_suffix: to_field_felt(owner_id.suffix()),
        owner_prefix: to_field_felt(owner_id.prefix().as_felt()),
    };
    let mock_tx_builder = chain
        .build_transaction(account_id)
        .tx_script(transaction_script_from_package(&tx_script_package));
    let mock_tx = apply_script_args(mock_tx_builder, &args).build().unwrap();
    let measurements = execute_tx_measurements(&mut chain, mock_tx);
    expect!["1458"].assert_eq(tx_script_processing_cycles(&measurements));
}
