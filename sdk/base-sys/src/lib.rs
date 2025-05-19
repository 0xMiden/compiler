// Enable no_std for the bindings module
#![no_std]
#![deny(warnings)]

#[cfg(feature = "bindings")]
pub mod bindings;

#[cfg(feature = "masl-lib")]
pub mod masl;

#[cfg(feature = "wit")]
pub mod base_sys_wit;
