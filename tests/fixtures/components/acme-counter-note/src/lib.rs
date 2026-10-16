//! Note which bumps the count of a user-written Miden Assembly counter component.
//!
//! The counter is a path dependency on a MASM project with no WIT: `#[account(counter::Counter)]`
//! reaches its procedures through the interface the SDK synthesizes from its package manifest.

#![no_std]
#![feature(alloc_error_handler)]

use miden::*;

use crate::bindings::acme::counter::counter::Delta;

/// Native account of the note: exposes the procedures of the MASM `counter` component.
#[account(counter::Counter)]
struct CounterAccount;

/// Storage-less note driving the counter.
#[note]
struct CounterNote;

#[note]
impl CounterNote {
    /// Increments the count, then doubles it and adds 5.
    #[note_script]
    pub fn run(self, _arg: Word, account: &mut CounterAccount) {
        account.increment();
        account.add_to_count(Delta {
            add: felt!(5),
            mul: felt!(2),
        });
    }
}
