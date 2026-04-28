#![no_std]

//! Opaque stubs for Miden standard library functions.

include!("../../linker_stub.rs");

mod mem;
mod crypto;
mod collections;
