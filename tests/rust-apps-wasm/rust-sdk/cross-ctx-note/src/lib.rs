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

bindings::export!(MyNote with_types_in bindings);

mod bindings;

use bindings::{
    exports::miden::base::note_script::Guest, miden::cross_ctx_account::foo::process_felt,
};
use miden::*;

struct MyNote;

impl Guest for MyNote {
    fn note_script() {
        let input = felt!(7);
        let output = process_felt(input);
        assert_eq(output, felt!(10));
    }
}
