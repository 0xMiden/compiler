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

use bindings::exports::miden::cross_ctx_account_word::*;

bindings::export!(MyFoo with_types_in bindings);

mod bindings;

use miden::{felt, Felt, Word};

struct MyFoo;

impl foo::Guest for MyFoo {
    fn process_word(input: Word) -> Word {
        // Add 1 to each element
        let result = Word::new([
            input.inner.0 + felt!(1),
            input.inner.1 + felt!(1),
            input.inner.2 + felt!(1),
            input.inner.3 + felt!(1),
        ]);

        result
    }

    fn process_another_word(input: Word) -> Word {
        // Add 2 to each element
        let result = Word::new([
            input.inner.0 + felt!(2),
            input.inner.1 + felt!(2),
            input.inner.2 + felt!(2),
            input.inner.3 + felt!(2),
        ]);

        result
    }

    fn process_felt(input: Felt) -> Felt {
        input + felt!(3)
    }
}

