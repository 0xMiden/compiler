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

miden::miden_generate!();
bindings::export!(MyFoo);

use miden::Felt;

// To test the data segment loading
pub static mut FOO: u32 = 42;

struct MyFoo;

impl foo::Guest for MyFoo {
    fn process_felt(input: Felt) -> Felt {
        let res = input + Felt::from_u32(unsafe { FOO });
        unsafe { FOO = res.as_u64() as u32 };
        res
    }
}
