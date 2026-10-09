//! Rust transaction scripts exchanging multi-element values with the standard (MASM) components.
//!
//! Each getter script calls a standard getter whose result occupies several stack elements and
//! asserts every field against the values the host configured the account with, which pins the
//! order in which a MASM callee's results come back. The role script passes an account id
//! parameter, which pins the order in which its felts reach a MASM callee. One getter script reads
//! a foreign faucet through foreign procedure invocation, which pins the same order across the
//! FPI boundary.

use miden_core::Felt;
use miden_field_repr::{FromFeltRepr, ToFeltRepr};
use miden_protocol::{
    account::{AccountId, auth::AuthScheme},
    asset::TokenSymbol,
};
use miden_standards::account::access::{Ownable2Step, RoleBasedAccessControl};
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

/// Host-side mirror of `TxScriptArgs` in `tests/fixtures/components/std-faucet-config-fpi-tx-script`.
#[derive(FromFeltRepr, ToFeltRepr)]
struct FaucetConfigFpiArgs {
    faucet_suffix: miden_field::Felt,
    faucet_prefix: miden_field::Felt,
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

/// Host-side mirror of `TxScriptArgs` in `tests/fixtures/components/std-rbac-tx-script`.
#[derive(FromFeltRepr, ToFeltRepr)]
struct RoleArgs {
    role: miden_field::Felt,
    account_suffix: miden_field::Felt,
    account_prefix: miden_field::Felt,
    expected: miden_field::Felt,
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

/// A Rust transaction script running on a wallet reads the four-field `token-config` record of a
/// standard fungible faucet through foreign procedure invocation and finds every field where the
/// faucet stores it.
#[test]
pub fn std_faucet_get_token_config_through_fpi_returns_the_configured_fields() {
    let tx_script_package =
        compile_rust_package("../fixtures/components/std-faucet-config-fpi-tx-script", true);

    // Distinct values for every field, so a swapped pair cannot pass; the decimals are
    // `miden-testing`'s default faucet decimals.
    let (max_supply, supply, decimals) = (1_000_000_000u64, 12_345u64, 10u8);
    let symbol = Felt::from(TokenSymbol::new("TEST").unwrap());
    let mut builder = MockChain::builder();
    let faucet_id = builder
        .add_existing_basic_faucet(auth(), "TEST", max_supply, Some(supply))
        .unwrap()
        .id();
    let wallet_id = builder.add_existing_wallet(auth()).unwrap().id();
    let mut chain = builder.build().unwrap();
    chain.prove_next_block().unwrap();

    let args = FaucetConfigFpiArgs {
        faucet_suffix: to_field_felt(faucet_id.suffix()),
        faucet_prefix: to_field_felt(faucet_id.prefix().as_felt()),
        supply: to_field_felt(Felt::new_unchecked(supply)),
        max_supply: to_field_felt(Felt::new_unchecked(max_supply)),
        decimals,
        symbol: to_field_felt(symbol),
    };
    let mock_tx_builder = chain
        .build_transaction(wallet_id)
        .foreign_accounts([chain.get_foreign_account_inputs(faucet_id).unwrap()])
        .tx_script(transaction_script_from_package(&tx_script_package));
    let mock_tx = apply_script_args(mock_tx_builder, &args).build().unwrap();
    let measurements = execute_tx_measurements(&mut chain, mock_tx);
    expect!["4976"].assert_eq(tx_script_processing_cycles(&measurements));
}

/// A Rust transaction script reads the owner of a standard `ownable2step` component as a
/// `miden::AccountId` (the core `account-id`) and finds the owner's suffix and prefix where the
/// component stores them.
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

/// A Rust transaction script passes a `miden::AccountId` to the standard `rbac` component's
/// `has_role` and gets the right answer for a role member and a non-member, so the account id
/// reaches the MASM callee suffix first, as the component expects.
#[test]
pub fn std_rbac_has_role_takes_an_account_id_parameter() {
    let tx_script_package = compile_rust_package("../fixtures/components/std-rbac-tx-script", true);

    let mut builder = MockChain::builder();
    let member = builder.add_existing_wallet(auth()).unwrap().id();
    let non_member = builder.add_existing_wallet(auth()).unwrap().id();
    let rbac = RoleBasedAccessControl::with_admins([member]).unwrap();
    let account_id = builder
        .add_existing_account_from_components(auth(), [rbac.into()])
        .unwrap()
        .id();
    let mut chain = builder.build().unwrap();
    chain.prove_next_block().unwrap();

    let admin_role: Felt = RoleBasedAccessControl::admin_role().into();
    let mut cycles = Vec::new();
    for (queried, expected) in [(member, Felt::ONE), (non_member, Felt::ZERO)] {
        let args = RoleArgs {
            role: to_field_felt(admin_role),
            account_suffix: to_field_felt(queried.suffix()),
            account_prefix: to_field_felt(queried.prefix().as_felt()),
            expected: to_field_felt(expected),
        };
        let mock_tx_builder = chain
            .build_transaction(account_id)
            .tx_script(transaction_script_from_package(&tx_script_package));
        let mock_tx = apply_script_args(mock_tx_builder, &args).build().unwrap();
        let measurements = execute_tx_measurements(&mut chain, mock_tx);
        cycles.push(tx_script_processing_cycles(&measurements).to_string());
    }
    let [member_cycles, non_member_cycles]: [String; 2] =
        cycles.try_into().expect("one measurement per query");
    expect!["1365"].assert_eq(&member_cycles);
    expect!["1270"].assert_eq(&non_member_cycles);
}
