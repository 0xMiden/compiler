//! Note script that calls a counter contract through foreign procedure invocation.

#![no_std]
#![feature(alloc_error_handler)]

use miden::*;

use crate::bindings::CounterContract;

/// Input for a note that checks a foreign counter account.
#[note]
struct CounterCaller {
    /// Account id of the counter contract to invoke through FPI.
    counter_account_id: AccountId,
}

#[note]
impl CounterCaller {
    /// Checks that the foreign counter account currently stores `1`.
    #[note_script]
    pub fn run(self, _arg: Word) {
        let count_acc = CounterContract::from_account(self.counter_account_id);
        let count = count_acc.get_count();
        assert_eq(count, felt!(42));
    }
}
