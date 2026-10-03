// Enable no_std for the bindings module
#![no_std]
#![cfg_attr(all(target_family = "wasm", miden), feature(linkage))]
#![deny(warnings)]

pub mod bindings;
pub mod raw;
