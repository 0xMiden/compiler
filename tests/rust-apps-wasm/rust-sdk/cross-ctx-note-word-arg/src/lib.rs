// Do not link against libstd (i.e. anything defined in `std::`)
#![no_std]
#![feature(alloc_error_handler)]

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

// Required for no-std crates
#[cfg(not(test))]
#[alloc_error_handler]
fn my_alloc_error(_info: core::alloc::Layout) -> ! {
    loop {}
}

miden::generate!();
bindings::export!(MyNote);

use bindings::{
    exports::miden::base::note_script::Guest, miden::cross_ctx_account_word_arg::foo::process_word,
};
use miden::*;

struct MyNote;

impl Guest for MyNote {
    fn run(_arg: Word) {
        let input1 = Word {
            inner: (felt!(1), felt!(2), felt!(3), felt!(4)),
        };
        let input2 = Word {
            inner: (felt!(5), felt!(6), felt!(7), felt!(8)),
        };
        let input3 = Word {
            inner: (felt!(9), felt!(10), felt!(11), felt!(12)),
        };
        let felt1 = felt!(13);
        let felt2 = felt!(14);
        let felt3 = felt!(15);

        // Returns "hash" of the inputs
        let output = process_word(input1, input2, input3, felt1, felt2, felt3, felt!(7));
        // Expected:
        // input1: 1*1 + 2*2 + 3*4 + 4*8 = 1 + 4 + 12 + 32 = 49
        // input2: 5*16 + 6*32 + 7*64 + 8*128 = 80 + 192 + 448 + 1024 = 1744
        // input3: 9*256 + 10*512 + 11*1024 + 12*2048 = 2304 + 5120 + 11264 + 24576 = 43264
        // felt1: 13*4096 = 53248
        // felt2: 14*8192 = 114688
        // felt3: 15*16384 = 245760
        // Total: 49 + 1744 + 43264 + 53248 + 114688 + 245760 = 458753 + 7 = 458760
        assert_eq(output, felt!(458760));
    }
}
