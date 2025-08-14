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

use bindings::exports::miden::cross_ctx_account_word_arg::*;

bindings::export!(MyFoo with_types_in bindings);

mod bindings;

use miden::*;

struct MyFoo;

impl foo::Guest for MyFoo {
    fn process_word(
        input1: Word,
        input2: Word,
        input3: Word,
        felt1: Felt,
        felt2: Felt,
        felt3: Felt,
        felt4: Felt,
    ) -> Felt {
        // Use weighted sum to encode the order of elements. Different weights ensure different
        // results if elements are reordered during the flattening
        let sum1 = input1.inner.0 * felt!(1)
            + input1.inner.1 * felt!(2)
            + input1.inner.2 * felt!(4)
            + input1.inner.3 * felt!(8);

        let sum2 = input2.inner.0 * felt!(16)
            + input2.inner.1 * felt!(32)
            + input2.inner.2 * felt!(64)
            + input2.inner.3 * felt!(128);

        let sum3 = input3.inner.0 * felt!(256)
            + input3.inner.1 * felt!(512)
            + input3.inner.2 * felt!(1024)
            + input3.inner.3 * felt!(2048);

        let felt_sum = felt1 * felt!(4096) + felt2 * felt!(8192) + felt3 * felt!(16384);

        let felt4_sum = felt4 + felt!(1);

        sum1 + sum2 + sum3 + felt_sum + felt4_sum
    }
}
