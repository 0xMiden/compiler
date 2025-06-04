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

use bindings::{exports::miden::base::note_script::Guest, miden::cross_ctx_account_word::foo::*};
use miden::*;

struct MyNote;

impl Guest for MyNote {
    fn note_script() {
        // Create a test word with values [2, 3, 4, 5]
        let input = Word {
            inner: (felt!(2), felt!(3), felt!(4), felt!(5)),
        };

        // Call process_word which should add 1 to each element
        let output = process_word(input.clone());

        // Verify the result is [3, 4, 5, 6]
        assert_eq(output.inner.0, felt!(3));
        assert_eq(output.inner.1, felt!(4));
        assert_eq(output.inner.2, felt!(5));
        assert_eq(output.inner.3, felt!(6));

        // Call process_another_word which should add 2 to each element
        let output = process_another_word(input);

        // Verify the result is [4, 5, 6, 7]
        assert_eq(output.inner.0, felt!(4));
        assert_eq(output.inner.1, felt!(5));
        assert_eq(output.inner.2, felt!(6));
        assert_eq(output.inner.3, felt!(7));

        let felt_input = felt!(2);
        let felt_output = process_felt(felt_input);
        assert_eq(felt_output, felt!(5));
    }
}
