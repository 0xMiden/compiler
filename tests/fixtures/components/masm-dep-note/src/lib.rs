//! A note whose only dependency is `../masm-dep-account`, a Rust account component that binds a
//! MASM package of its own. Building this note builds the account through the compiler's
//! dependency route; `execute` reaches the account's bound procedures through it.

#![no_std]
#![feature(alloc_error_handler)]

use miden::*;

use crate::bindings::miden::masm_dep_account::foo::process_felt;

#[note]
struct MyNote;

#[note]
impl MyNote {
    #[note_script]
    pub fn execute(self, _arg: Word) {
        let output = process_felt(felt!(11));
        assert_eq(output, felt!(53));
    }
}
