//! Transaction script which reads a user-written Miden Assembly counter component and checks the
//! values against the ones the host expects.
//!
//! The counter is a path dependency on a MASM project with no WIT: `#[account(counter::Counter)]`
//! reaches its procedures through the interface the SDK synthesizes from its package manifest.
//! `get_count_word` returns a word, so the script pins the order in which a user MASM callee's
//! multi-element result comes back; `wide` takes four words, the 16 parameter felts a call can
//! pass, and its result pins the order in which they arrive. `split` returns a tuple, whose order
//! the script checks; `four_words` returns the 16 result felts a call can return, and
//! `fifteen_to_word` takes 15 parameter felts and returns a word through a result pointer, the
//! most a call with that pointer can pass.

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
/// results of `wide`, `split`, `four_words` and `fifteen_to_word`.
#[tx_script]
fn run(args: TxScriptArgs, account: &mut CounterAccount) {
    assert_eq(account.get_count(), args.count);
    assert_word(account.get_count_word(), args.count_word);
    // Each felt is weighted by its position, so reversing the words, their elements or all 16
    // felts gives another sum.
    let wide = account.wide(
        Word::new([felt!(1), felt!(2), felt!(3), felt!(4)]),
        Word::new([felt!(10), felt!(20), felt!(30), felt!(40)]),
        Word::new([felt!(100), felt!(200), felt!(300), felt!(400)]),
        Word::new([felt!(1000), felt!(2000), felt!(3000), felt!(4000)]),
    );
    assert_eq(wide, felt!(27140));
    let (low, high) = account.split(args.count);
    assert_eq(low, args.count);
    assert_eq(high, low + felt!(1));
    let (w0, w1, w2, w3) = account.four_words();
    assert_word(w0, Word::new([felt!(1), felt!(2), felt!(3), felt!(4)]));
    assert_word(w1, Word::new([felt!(5), felt!(6), felt!(7), felt!(8)]));
    assert_word(w2, Word::new([felt!(9), felt!(10), felt!(11), felt!(12)]));
    assert_word(w3, Word::new([felt!(13), felt!(14), felt!(15), felt!(16)]));
    let fifteen = account.fifteen_to_word(
        Word::new([felt!(1), felt!(2), felt!(3), felt!(4)]),
        Word::new([felt!(10), felt!(20), felt!(30), felt!(40)]),
        Word::new([felt!(100), felt!(200), felt!(300), felt!(400)]),
        felt!(1000),
        felt!(2000),
        felt!(3000),
    );
    assert_word(fifteen, Word::new([felt!(1000), felt!(2000), felt!(3000), felt!(2700)]));
}

/// Asserts every element of `actual` equals the one of `expected` at the same index.
fn assert_word(actual: Word, expected: Word) {
    for index in 0..4 {
        assert_eq(actual[index], expected[index]);
    }
}
