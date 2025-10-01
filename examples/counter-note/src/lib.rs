// Do not link against libstd (i.e. anything defined in `std::`)
#![no_std]

// However, we could still use some standard library types while
// remaining no-std compatible, if we uncommented the following lines:
//
// extern crate alloc;
// use alloc::vec::Vec;

// Global allocator to use heap memory in no-std environment
#[global_allocator]
static ALLOC: miden::BumpAlloc = miden::BumpAlloc::new();

// Required for no-std crates
#[cfg(not(test))]
#[panic_handler]
fn my_panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}

use miden::*;

use crate::bindings::miden::counter_contract::counter;

#[note_script]
fn run(_arg: Word) {
    let initial_value = counter::get_count();
    counter::increment_count();
    let expected_value = initial_value + Felt::from_u32(1);
    let final_value = counter::get_count();
    assert_eq(final_value, expected_value);
}
