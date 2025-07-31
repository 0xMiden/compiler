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

bindings::export!(BasicWalletTxScript with_types_in bindings);

mod bindings;

use bindings::exports::miden::base::script::Guest;
use miden::*;

struct BasicWalletTxScript;

impl Guest for BasicWalletTxScript {
    fn script(_arg: Word) {}
}
