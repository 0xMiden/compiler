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

use bindings::exports::miden::cross_ctx_account::*;

bindings::export!(MyFoo with_types_in bindings);

mod bindings;

use miden::{felt, Felt};

struct MyFoo;

impl foo::Guest for MyFoo {
    fn process_felt(input: Felt) -> Felt {
        // TODO: load increment from the global variable to test rodata initialization on fresh
        // context creation
        input + felt!(3)
    }
}
