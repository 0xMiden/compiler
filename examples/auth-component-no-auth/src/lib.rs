// Do not link against libstd (i.e. anything defined in `std::`)
#![no_std]
#![feature(alloc_error_handler)]

// However, we could still use some standard library types while
// remaining no-std compatible, if we uncommented the following lines:
//
//
// extern crate alloc;
// use alloc::vec::Vec;

use miden::{Word, component, component_storage};

#[component_storage]
struct AuthComponentStorage;

/// API of the no-auth authentication component.
#[component]
trait AuthComponent {
    #[auth_script]
    fn auth_procedure(&mut self, _arg: Word);
}

#[component]
impl AuthComponent for AuthComponentStorage {
    fn auth_procedure(&mut self, _arg: Word) {
        // translated from MASM at
        // https://github.com/0xMiden/miden-base/blob/e4912663276ab8eebb24b84d318417cb4ea0bba3/crates/miden-lib/asm/account_components/no_auth.masm?plain=1
        let init_comm = miden::native_account::get_initial_commitment();
        let curr_comm = self.compute_commitment();
        // check if the account state has changed by comparing initial and final commitments
        if curr_comm != init_comm {
            // if the account has been updated, increment the nonce
            self.incr_nonce();
        }
    }
}
