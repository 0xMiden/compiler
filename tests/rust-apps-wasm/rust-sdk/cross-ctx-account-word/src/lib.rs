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

miden::miden_generate!();
bindings::export!(MyFoo);

use foo::{MixedStruct, NestedStruct, Pair, Triple};
use miden::{felt, Felt, Word};

struct MyFoo;

impl foo::Guest for MyFoo {
    fn process_word(input: Word) -> Word {
        let result = Word::new([
            input.inner.0 + felt!(1),
            input.inner.1 + felt!(2),
            input.inner.2 + felt!(3),
            input.inner.3 + felt!(4),
        ]);

        result
    }

    // To test the proper `canon lower` reconstruction on shim + fixup modules bypass
    // The same signature, different name and body
    fn process_another_word(input: Word) -> Word {
        let result = Word::new([
            input.inner.0 + felt!(2),
            input.inner.1 + felt!(3),
            input.inner.2 + felt!(4),
            input.inner.3 + felt!(5),
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
