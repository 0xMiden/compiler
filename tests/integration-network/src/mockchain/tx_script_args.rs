//! Transaction-script argument transport tests.
//!
//! Exercise both `ScriptArgs` transport modes end-to-end on a mock chain: word mode (arguments
//! packed into the `TX_SCRIPT_ARGS` word) and commitment mode (arguments passed through the
//! advice provider, hash-verified against the args word). The negative cases pin the security
//! properties one by one: word-mode padding canonicality, the in-VM hash verification itself
//! (a valid preimage under a wrong args word must fail *because of* the hash check), tamper
//! detection for values the script never reads, and commitment-preimage canonicality.

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
///
/// The entrypoint deliberately has a non-`run` name: the WIT export stays `run`, so this pins the
/// macro's `method_path` != `export_name` metadata path end-to-end.
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
fn check_args(args: WordModeArgs) {
    assert_eq(args.a, felt!(7));
    assert_eq(Felt::new(args.b as u64).unwrap(), felt!(9));
}
"#;

/// Transaction script whose variable-length arguments travel through the advice provider.
///
/// `extra[1]` and `extra[2]` are deliberately never read by the script, so tampering with them
/// can only be caught by the in-VM hash verification of the preimage.
const COMMITMENT_MODE_SOURCE: &str = r#"#![no_std]
#![feature(alloc_error_handler)]

extern crate alloc;

use alloc::vec::Vec;
use miden::*;

/// Commitment-mode script arguments: too long for the args word and variable-length.
#[derive(FromFeltRepr, ToFeltRepr)]
pub struct CommitmentModeArgs {
    pub values: Word,
    pub count: u64,
    pub extra: Vec<Felt>,
}

#[tx_script]
fn run(args: CommitmentModeArgs) {
    assert_eq(args.values[0], felt!(11));
    assert_eq(args.values[1], felt!(12));
    assert_eq(args.values[2], felt!(13));
    assert_eq(args.values[3], felt!(14));
    assert!(args.count == 5);
    assert!(args.extra.len() == 3);
    assert_eq(args.extra[0], felt!(21));
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
    extra: Vec<miden_field::Felt>,
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

/// Error code of the `#[tx_script]` wrapper's decode panic; every in-VM decode failure funnels
/// through it (see `assert_failure_contains` for re-calibration).
const DECODE_PANIC_CODE: &str = "assertion failed with error code: 10154102372021603817";

/// Asserts a failed execution's error message so an unrelated failure cannot satisfy the test.
///
/// The expected error codes are stable products of the deterministic guest build. All decode
/// failures share one code — the `#[tx_script]` wrapper's panic on a `ScriptArgs::decode` error —
/// while the in-VM hash verification fails with its own distinct stdlib code, so the
/// hash-isolation case still proves that specific mechanism. When an intentional SDK or codegen
/// change shifts a code, re-calibrate it from this assert's failure output, like an expect test.
fn assert_failure_contains(err: &str, needle: &str) {
    assert!(err.contains(needle), "unexpected failure message (wanted `{needle}`): {err}");
}

/// Word-mode arguments pass in the args word directly (no advice map) and non-zero felts in the
/// unused padding fail the transaction.
#[test]
pub fn word_mode_args() {
    let tx_script_package = compile_tx_script_fixture("word-mode-tx-script", WORD_MODE_SOURCE);
    let (mut chain, account_id) = build_chain_with_wallet_account();

    let args = WordModeArgs {
        a: miden_field::Felt::new(7).unwrap(),
        b: 9,
    };
    // Pin the mirror's encoded size so layout drift vs the fixture struct fails loudly.
    assert_eq!(<WordModeArgs as ScriptArgs>::FIXED_LEN, Some(2));
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
    let err = execute_tx_expect_failure(tx_context_builder);
    assert_failure_contains(&err, DECODE_PANIC_CODE);
}

/// Commitment-mode arguments pass through the advice provider hash-verified against the args
/// word; every way the host can lie about the preimage fails the transaction.
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
        extra: vec![
            miden_field::Felt::new(21).unwrap(),
            miden_field::Felt::new(22).unwrap(),
            miden_field::Felt::new(23).unwrap(),
        ],
    };
    // Variable-length (`Vec`) types always use commitment mode.
    assert_eq!(<CommitmentModeArgs as ScriptArgs>::FIXED_LEN, None);
    let EncodedScriptArgs::Preimage(felts) = args.encode() else {
        panic!("expected commitment mode for a variable-length encoding");
    };
    // Pin the mirror's encoded size (4 + 2 + 1 + 3 payload felts, padded to 3 words) so layout
    // drift vs the fixture struct fails loudly.
    assert_eq!(felts.len(), 12);
    let preimage = from_field_felts(&felts);
    let args_word = miden_core::crypto::hash::Poseidon2::hash_elements(&preimage);

    // Positive: the preimage travels through the advice map keyed by its hash.
    let tx_context_builder = chain
        .build_tx_context(account_id, &[], &[])
        .unwrap()
        .tx_script(transaction_script_from_package(&tx_script_package));
    let tx_context_builder = apply_script_args(tx_context_builder, &args);
    execute_tx(&mut chain, tx_context_builder);

    // Isolates the hash verification: a *valid* preimage registered under a wrong args word.
    // Everything else about the transaction is well-formed, so without the in-VM hash check the
    // script would succeed.
    let mut wrong_word_source = preimage.clone();
    wrong_word_source[0] = Felt::new_unchecked(999);
    let wrong_word = miden_core::crypto::hash::Poseidon2::hash_elements(&wrong_word_source);
    let tx_context_builder = chain
        .build_tx_context(account_id, &[], &[])
        .unwrap()
        .tx_script(transaction_script_from_package(&tx_script_package))
        .tx_script_args(wrong_word)
        .extend_advice_map([(wrong_word, preimage.clone())]);
    let err = execute_tx_expect_failure(tx_context_builder);
    assert_failure_contains(&err, "assertion failed with error code: 0");

    // Tampering with a felt the script never reads (`extra[2]`) is caught only by the hash
    // verification of the preimage.
    let mut tampered = preimage.clone();
    let extra_2 = tampered.len() - 3;
    tampered[extra_2] = Felt::new_unchecked(tampered[extra_2].as_canonical_u64() + 1);
    let tx_context_builder = chain
        .build_tx_context(account_id, &[], &[])
        .unwrap()
        .tx_script(transaction_script_from_package(&tx_script_package))
        .tx_script_args(args_word)
        .extend_advice_map([(args_word, tampered)]);
    let err = execute_tx_expect_failure(tx_context_builder);
    assert_failure_contains(&err, "assertion failed with error code: 0");

    // Canonicality: a *self-consistent* preimage (args word = its real hash) with a non-zero
    // padding felt passes the hash check and must be rejected by the decode itself.
    let mut nonzero_padding = preimage.clone();
    *nonzero_padding.last_mut().unwrap() = Felt::new_unchecked(1);
    let bad_word = miden_core::crypto::hash::Poseidon2::hash_elements(&nonzero_padding);
    let tx_context_builder = chain
        .build_tx_context(account_id, &[], &[])
        .unwrap()
        .tx_script(transaction_script_from_package(&tx_script_package))
        .tx_script_args(bad_word)
        .extend_advice_map([(bad_word, nonzero_padding)]);
    let err = execute_tx_expect_failure(tx_context_builder);
    assert_failure_contains(&err, DECODE_PANIC_CODE);

    // Canonicality: a self-consistent preimage with a whole extra all-zero word must be rejected
    // as trailing data.
    let mut extra_word = preimage.clone();
    extra_word.extend([Felt::ZERO; 4]);
    let bad_word = miden_core::crypto::hash::Poseidon2::hash_elements(&extra_word);
    let tx_context_builder = chain
        .build_tx_context(account_id, &[], &[])
        .unwrap()
        .tx_script(transaction_script_from_package(&tx_script_package))
        .tx_script_args(bad_word)
        .extend_advice_map([(bad_word, extra_word)]);
    let err = execute_tx_expect_failure(tx_context_builder);
    assert_failure_contains(&err, DECODE_PANIC_CODE);

    // Canonicality: an advice value that is not a whole number of words fails before the decode.
    let mut non_word_multiple = preimage.clone();
    non_word_multiple.push(Felt::ZERO);
    let bad_word = miden_core::crypto::hash::Poseidon2::hash_elements(&non_word_multiple);
    let tx_context_builder = chain
        .build_tx_context(account_id, &[], &[])
        .unwrap()
        .tx_script(transaction_script_from_package(&tx_script_package))
        .tx_script_args(bad_word)
        .extend_advice_map([(bad_word, non_word_multiple)]);
    let err = execute_tx_expect_failure(tx_context_builder);
    assert_failure_contains(&err, DECODE_PANIC_CODE);
}
