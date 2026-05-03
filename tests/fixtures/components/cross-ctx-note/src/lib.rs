// Do not link against libstd (i.e. anything defined in `std::`)
#![no_std]
#![feature(alloc_error_handler)]

use miden::*;

use crate::bindings::miden::cross_ctx_account::foo::process_felt;

// To test the data segment loading
pub static mut BAR: u32 = 11;

#[note]
struct MyNote;

#[note]
impl MyNote {
    #[note_script]
    pub fn execute(self, _arg: Word) {
        let input = Felt::new(unsafe { BAR } as u64).unwrap();
        assert_eq(input, felt!(11));
        let output = process_felt(input);
        assert_eq(output, felt!(53));
        unsafe { BAR = output.as_canonical_u64() as u32 };
    }
}
