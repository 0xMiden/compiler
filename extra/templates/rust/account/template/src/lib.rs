// Do not link against libstd (i.e. anything defined in `std::`)
#![no_std]
#![feature(alloc_error_handler)]

use miden::{Felt, component, component_storage};

/// Storage layout for the account component (empty).
#[component_storage]
struct {{crate_name | upper_camel_case}}Storage;

/// API of the account component.
#[component]
trait {{crate_name | upper_camel_case}} {
    /// Adds two field elements.
    fn add(&self, a: Felt, b: Felt) -> Felt;
}

#[component]
impl {{crate_name | upper_camel_case}} for {{crate_name | upper_camel_case}}Storage {
    fn add(&self, a: Felt, b: Felt) -> Felt {
        a + b
    }
}
