#![no_std]

//! Unreachable stubs for Miden base SDK functions.
//!
//! These stubs are compiled by build.rs into a separate rlib and
//! linked to `miden-base-sys` so that the Wasm translator can lower
//! the calls appropriately. They are not part of the crate sources.

mod account;
mod note;
mod tx;

// Minimal panic handler for `#![no_std]` staticlib builds on wasm.
// We never intend to panic, but the symbol must exist.
#[cfg(stub_staticlib)]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}
