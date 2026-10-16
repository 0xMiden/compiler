//! Transaction script which reads a user-written Miden Assembly counter component and checks the
//! values against the ones the host expects.
//!
//! The counter is a path dependency on a MASM project with no WIT: `#[account(counter::Counter)]`
//! reaches its procedures through the interface the SDK synthesizes from its package manifest.
//! `get_count_word` returns a word, so the script pins the order in which a user MASM callee's
//! multi-element result comes back; `wide` takes four words, the 16 parameter felts a call can
//! pass, and its result pins the order in which they arrive.

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

/// Reads the count and the count word and asserts each equals the expected one, then asserts the
/// result of `wide`.
#[tx_script]
fn run(args: TxScriptArgs, account: &mut CounterAccount) {
    assert_eq(account.get_count(), args.count);
    let word = account.get_count_word();
    assert_eq(word[0], args.count_word[0]);
    assert_eq(word[1], args.count_word[1]);
    assert_eq(word[2], args.count_word[2]);
    assert_eq(word[3], args.count_word[3]);
    // `a[0] + b[1] + c[2] + d[3]`: reversing the words or their elements gives 1234 instead.
    let wide = account.wide(
        Word::new([felt!(1), felt!(2), felt!(3), felt!(4)]),
        Word::new([felt!(10), felt!(20), felt!(30), felt!(40)]),
        Word::new([felt!(100), felt!(200), felt!(300), felt!(400)]),
        Word::new([felt!(1000), felt!(2000), felt!(3000), felt!(4000)]),
    );
    assert_eq(wide, felt!(4321));
}
