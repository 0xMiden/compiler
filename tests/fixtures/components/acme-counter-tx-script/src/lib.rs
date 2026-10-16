//! Transaction script which reads a user-written Miden Assembly counter component and checks the
//! values against the ones the host expects.
//!
//! The counter is a path dependency on a MASM project with no WIT: `#[account(counter::Counter)]`
//! reaches its procedures through the interface the SDK synthesizes from its package manifest.
//! `get_count_word` returns a word, so the script pins the order in which a user MASM callee's
//! multi-element result comes back.

#![no_std]
#![feature(alloc_error_handler)]

use miden::*;

/// Native account of the transaction script: exposes the procedures of the MASM `counter`
/// component.
#[account(counter::Counter)]
struct CounterAccount;

/// The values the account's counter is expected to report, transported via the `TX_SCRIPT_ARGS`
/// word.
#[derive(FromFeltRepr, ToFeltRepr)]
pub struct TxScriptArgs {
    /// The expected `get_count` result.
    pub count: Felt,
    /// The expected `get_count_word` result.
    pub count_word: Word,
}

/// Reads the count and the count word and asserts each equals the expected one.
#[tx_script]
fn run(args: TxScriptArgs, account: &mut CounterAccount) {
    assert_eq(account.get_count(), args.count);
    let word = account.get_count_word();
    assert_eq(word[0], args.count_word[0]);
    assert_eq(word[1], args.count_word[1]);
    assert_eq(word[2], args.count_word[2]);
    assert_eq(word[3], args.count_word[3]);
}
