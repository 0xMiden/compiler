// Do not link against libstd (i.e. anything defined in `std::`)
#![no_std]
#![feature(alloc_error_handler)]

use miden::{Word, component, component_storage};

#[component_storage]
struct AuthComponentStorage;

/// API of the authentication component.
#[component]
trait AuthComponent {
    #[auth_script]
    fn verify(&self, _arg: Word);
}

#[component]
impl AuthComponent for AuthComponentStorage {
    fn verify(&self, _arg: Word) {
        // If this procedure returns control the transcation authentication is considered succesfull.
        // In case of panic the transaction authentication considered as failed by the transaction kernel.
        todo!()
    }
}
