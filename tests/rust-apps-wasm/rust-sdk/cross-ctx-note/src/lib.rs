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

use miden::*;

miden::generate!();
bindings::export!(MyNote);

use bindings::{
    exports::miden::base::note_script::Guest, miden::cross_ctx_account::foo::process_felt,
};

// To test the data segment loading
pub static mut BAR: u32 = 11;

struct MyNote;

impl Guest for MyNote {
    fn run(_arg: Word) {
        let input = Felt::from_u32(unsafe { BAR });
        assert_eq(input, felt!(11));
        let output = process_felt(input);
        assert_eq(output, felt!(53));
        unsafe { BAR = output.as_u64() as u32 };
    }
}
