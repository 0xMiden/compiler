// Enable no_std for the bindings module
#![no_std]
#![deny(warnings)]

pub mod bindings;

#[cfg(feature = "wit")]
pub mod base_sys_wit;
