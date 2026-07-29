//! Transaction-script argument transport tests.
//!
//! Exercise both `ScriptArgs` transport modes end-to-end on a mock chain: word mode (arguments
//! packed into the `TX_SCRIPT_ARGS` word) and commitment mode (arguments passed through the
//! advice provider, hash-verified against the args word), including the failure paths.

use std::sync::Arc;

use miden_client::account::AccountId;
use miden_core::Felt;
use miden_field_repr::{FromFeltRepr, ToFeltRepr};
use miden_mast_package::Package;
use miden_protocol::account::{AccountBuilder, AccountType, auth::AuthScheme};
use miden_testing::{AccountState, Auth, MockChain};
use miden_tx_script_args::{EncodedScriptArgs, ScriptArgs};
use midenc_integration_test_support::project;

use super::support::{
    apply_script_args, compile_rust_package, execute_tx, execute_tx_expect_failure,
    from_field_felts, from_field_word, transaction_script_from_package, tx_script_cargo_toml,
    tx_script_miden_project_toml,
};

/// Transaction script whose two-felt arguments travel in the args word directly.
const WORD_MODE_SOURCE: &str = r#"#![no_std]
#![feature(alloc_error_handler)]

use miden::*;

/// Word-mode script arguments: two felts pack directly into the args word.
#[derive(FromFeltRepr, ToFeltRepr)]
pub struct WordModeArgs {
    pub a: Felt,
    pub b: u32,
}

#[tx_script]
fn run(args: WordModeArgs) {
    assert_eq(args.a, felt!(7));
    assert_eq(Felt::new(args.b as u64).unwrap(), felt!(9));
}
"#;

/// Transaction script whose six-felt arguments travel through the advice provider.
const COMMITMENT_MODE_SOURCE: &str = r#"#![no_std]
#![feature(alloc_error_handler)]

use miden::*;

/// Commitment-mode script arguments: six felts exceed the args word.
#[derive(FromFeltRepr, ToFeltRepr)]
pub struct CommitmentModeArgs {
    pub values: Word,
    pub count: u64,
}

#[tx_script]
fn run(args: CommitmentModeArgs) {
    assert_eq(args.values[0], felt!(11));
    assert_eq(args.values[1], felt!(12));
    assert_eq(args.values[2], felt!(13));
    assert_eq(args.values[3], felt!(14));
    assert!(args.count == 5);
}
"#;

/// Host-side mirror of the generated fixture's `WordModeArgs`.
#[derive(FromFeltRepr, ToFeltRepr)]
struct WordModeArgs {
    a: miden_field::Felt,
    b: u32,
}

/// Host-side mirror of the generated fixture's `CommitmentModeArgs`.
#[derive(FromFeltRepr, ToFeltRepr)]
struct CommitmentModeArgs {
    values: miden_field::Word,
    count: u64,
}

/// Compiles a generated transaction-script project from the given source.
fn compile_tx_script_fixture(script_name: &str, source: &str) -> Arc<Package> {
    let tx_script_project = project(script_name)
        .file("miden-project.toml", &tx_script_miden_project_toml(script_name))
        .file("Cargo.toml", &tx_script_cargo_toml(script_name))
        .file("src/lib.rs", source)
        .build();
    compile_rust_package(tx_script_project.root(), true)
}

/// Builds a mock chain with a single basic-wallet account to execute transaction scripts against.
fn build_chain_with_wallet_account() -> (MockChain, AccountId) {
    let mut builder = MockChain::builder();
    let account_builder = AccountBuilder::new([7_u8; 32])
        .account_type(AccountType::Public)
        .with_component(miden_client::account::component::BasicWallet);
    let account = builder
        .add_account_from_builder(
            Auth::BasicAuth {
                auth_scheme: AuthScheme::Falcon512Poseidon2,
            },
            account_builder,
            AccountState::Exists,
        )
        .expect("failed to add wallet account to mock chain builder");
    let account_id = account.id();

    let mut chain = builder.build().expect("failed to build mock chain");
    chain.prove_next_block().unwrap();

    (chain, account_id)
}

/// Word-mode arguments pass in the args word directly (no advice map) and non-zero padding in the
/// unused felts fails the transaction.
#[test]
pub fn word_mode_args() {
    let tx_script_package = compile_tx_script_fixture("word-mode-tx-script", WORD_MODE_SOURCE);
    let (mut chain, account_id) = build_chain_with_wallet_account();

    let args = WordModeArgs {
        a: miden_field::Felt::new(7).unwrap(),
        b: 9,
    };
    let encoded = args.encode();
    assert!(
        matches!(encoded, EncodedScriptArgs::Word(_)),
        "expected word mode for a 2-felt encoding"
    );

    // Positive: the encoded args word decodes in the guest, no advice map involved.
    let tx_context_builder = chain
        .build_tx_context(account_id, &[], &[])
        .unwrap()
        .tx_script(transaction_script_from_package(&tx_script_package));
    let tx_context_builder = apply_script_args(tx_context_builder, &args);
    execute_tx(&mut chain, tx_context_builder);

    // Negative: a non-zero felt in the unused padding must fail the guest-side decode.
    let EncodedScriptArgs::Word(args_word) = args.encode() else {
        unreachable!("mode asserted above");
    };
    let mut tampered = from_field_word(args_word);
    tampered[3] = Felt::new_unchecked(5);
    let tx_context_builder = chain
        .build_tx_context(account_id, &[], &[])
        .unwrap()
        .tx_script(transaction_script_from_package(&tx_script_package))
        .tx_script_args(tampered);
    execute_tx_expect_failure(tx_context_builder);
}

/// Commitment-mode arguments pass through the advice provider hash-verified against the args
/// word, and a tampered preimage fails the transaction.
#[test]
pub fn commitment_mode_args() {
    let tx_script_package =
        compile_tx_script_fixture("commitment-mode-tx-script", COMMITMENT_MODE_SOURCE);
    let (mut chain, account_id) = build_chain_with_wallet_account();

    let args = CommitmentModeArgs {
        values: miden_field::Word::new([
            miden_field::Felt::new(11).unwrap(),
            miden_field::Felt::new(12).unwrap(),
            miden_field::Felt::new(13).unwrap(),
            miden_field::Felt::new(14).unwrap(),
        ]),
        count: 5,
    };
    let encoded = args.encode();
    assert!(
        matches!(encoded, EncodedScriptArgs::Preimage(_)),
        "expected commitment mode for a 6-felt encoding"
    );

    // Positive: the preimage travels through the advice map keyed by its hash.
    let tx_context_builder = chain
        .build_tx_context(account_id, &[], &[])
        .unwrap()
        .tx_script(transaction_script_from_package(&tx_script_package));
    let tx_context_builder = apply_script_args(tx_context_builder, &args);
    execute_tx(&mut chain, tx_context_builder);

    // Negative: a preimage that does not hash to the args word must fail the in-VM hash check.
    let EncodedScriptArgs::Preimage(felts) = args.encode() else {
        unreachable!("mode asserted above");
    };
    let preimage = from_field_felts(&felts);
    let args_word = miden_core::crypto::hash::Poseidon2::hash_elements(&preimage);
    let mut tampered = preimage;
    tampered[0] = Felt::new_unchecked(tampered[0].as_canonical_u64() + 1);
    let tx_context_builder = chain
        .build_tx_context(account_id, &[], &[])
        .unwrap()
        .tx_script(transaction_script_from_package(&tx_script_package))
        .tx_script_args(args_word)
        .extend_advice_map([(args_word, tampered)]);
    execute_tx_expect_failure(tx_context_builder);
}
