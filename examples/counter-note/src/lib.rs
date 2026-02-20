// Do not link against libstd (i.e. anything defined in `std::`)
#![no_std]
#![feature(alloc_error_handler)]

// However, we could still use some standard library types while
// remaining no-std compatible, if we uncommented the following lines:
//
// extern crate alloc;
// use alloc::vec::Vec;

use miden::*;

use crate::bindings::miden::counter_contract::counter_contract;

#[note]
struct CounterNote;

#[note]
impl CounterNote {
    #[note_script]
    pub fn run(self, _arg: Word) {
        let initial_value = counter_contract::get_count();
        counter_contract::increment_count();
        let expected_value = initial_value + felt!(1);
        let final_value = counter_contract::get_count();
        assert_eq(final_value, expected_value);
    }
}
