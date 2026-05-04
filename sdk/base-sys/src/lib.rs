// Enable no_std for the bindings module
#![no_std]
#![cfg_attr(target_family = "wasm", feature(linkage))]
#![deny(warnings)]

pub mod bindings;
