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

use bindings::{exports::miden::base::script::Guest, miden::cross_ctx_account_word::foo::*};
use miden::*;

struct MyNote;

impl Guest for MyNote {
    fn script(_arg: Word) {
        let input = Word {
            inner: (felt!(2), felt!(3), felt!(4), felt!(5)),
        };

        let output = process_word(input.clone());

        assert_eq(output.inner.0, felt!(3));
        assert_eq(output.inner.1, felt!(5));
        assert_eq(output.inner.2, felt!(7));
        assert_eq(output.inner.3, felt!(9));

        let output = process_another_word(input);

        assert_eq(output.inner.0, felt!(4));
        assert_eq(output.inner.1, felt!(6));
        assert_eq(output.inner.2, felt!(8));
        assert_eq(output.inner.3, felt!(10));

        let felt_input = felt!(9);
        let felt_output = process_felt(felt_input);
        assert_eq(felt_output, felt!(12));

        let pair_input = Pair {
            first: felt!(10),
            second: felt!(20),
        };
        let pair_output = process_pair(pair_input);
        assert_eq(pair_output.first, felt!(14)); // 10 + 4
        assert_eq(pair_output.second, felt!(24)); // 20 + 4

        let triple_input = Triple {
            x: felt!(100),
            y: felt!(200),
            z: felt!(300),
        };
        let triple_output = process_triple(triple_input);
        assert_eq(triple_output.x, felt!(105)); // 100 + 5
        assert_eq(triple_output.y, felt!(205)); // 200 + 5
        assert_eq(triple_output.z, felt!(305)); // 300 + 5

        let mixed_input = MixedStruct {
            f: u64::MAX - 1000,
            a: Felt::new(Felt::M - 1 - 6).unwrap(),
            b: u32::MAX - 10,
            c: felt!(50),
            d: 111,
            e: false,
            g: 3,
        };
        let mixed_output = process_mixed(mixed_input);
        if mixed_output.f != u64::MAX {
            // fail
            assert_eq!(0, 1);
        }
        assert_eq(mixed_output.a, Felt::new(Felt::M - 1).unwrap()); // M - 1 - 6 + 6
        assert_eq(mixed_output.b.into(), Felt::from_u32(u32::MAX)); // u32::MAX - 10 + 10
        assert_eq(mixed_output.c, felt!(57)); // 50 + 7
        assert_eq(mixed_output.d.into(), Felt::from_u32(122));
        assert_eq(Felt::from_u32(mixed_output.e as u32), felt!(1));
        assert_eq(mixed_output.g.into(), Felt::from_u32(12));

        let nested_input = NestedStruct {
            inner: Pair {
                first: felt!(30),
                second: felt!(40),
            },
            value: felt!(50),
        };
        let nested_output = process_nested(nested_input);
        assert_eq(nested_output.inner.first, felt!(38)); // 30 + 8
        assert_eq(nested_output.inner.second, felt!(48)); // 40 + 8
        assert_eq(nested_output.value, felt!(58)); // 50 + 8
    }
}
