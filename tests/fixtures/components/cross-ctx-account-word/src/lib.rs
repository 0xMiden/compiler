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

use bindings::exports::miden::cross_ctx_account_word::*;

miden::generate!();
bindings::export!(MyFoo);

use foo::{MixedStruct, NestedStruct, Pair, Triple};
use miden::{Felt, Word, felt};

struct MyFoo;

impl foo::Guest for MyFoo {
    fn process_word(input: Word) -> Word {
        let result = Word::new([
            input.a + felt!(1),
            input.b + felt!(2),
            input.c + felt!(3),
            input.d + felt!(4),
        ]);

        result
    }

    // To test the proper `canon lower` reconstruction on shim + fixup modules bypass
    // The same signature, different name and body
    fn process_another_word(input: Word) -> Word {
        let result = Word::new([
            input.a + felt!(2),
            input.b + felt!(3),
            input.c + felt!(4),
            input.d + felt!(5),
        ]);

        result
    }

    fn process_felt(input: Felt) -> Felt {
        input + felt!(3)
    }

    fn process_pair(input: Pair) -> Pair {
        Pair {
            first: input.first + felt!(4),
            second: input.second + felt!(4),
        }
    }

    fn process_triple(input: Triple) -> Triple {
        Triple {
            x: input.x + felt!(5),
            y: input.y + felt!(5),
            z: input.z + felt!(5),
        }
    }

    fn process_mixed(input: MixedStruct) -> MixedStruct {
        MixedStruct {
            f: input.f + 1000,
            a: input.a + felt!(6),
            b: input.b + 10,
            c: input.c + felt!(7),
            d: input.d + 11,
            e: !input.e,
            g: input.g + 9,
        }
    }

    fn process_nested(input: NestedStruct) -> NestedStruct {
        NestedStruct {
            inner: Pair {
                first: input.inner.first + felt!(8),
                second: input.inner.second + felt!(8),
            },
            value: input.value + felt!(8),
        }
    }
}
