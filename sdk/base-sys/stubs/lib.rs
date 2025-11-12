#![no_std]

//! Unreachable stubs for Miden base SDK functions.
//!
//! These stubs are compiled by build.rs into a separate rlib and
//! linked to `miden-base-sys` so that the Wasm translator can lower
//! the calls appropriately. They are not part of the crate sources.

mod active_account;
mod asset;
mod active_note;
mod faucet;
mod input_note;
mod output_note;
mod native_account;
mod tx;

// No panic handler here; the stubs are packaged as a single object into a
// static archive by build.rs to avoid introducing panic symbols.
