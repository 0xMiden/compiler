#![no_std]
#![deny(warnings)]

extern crate alloc;

mod intrinsics;
mod stdlib;
#[cfg(feature = "wit")]
pub mod stdlib_wit;

pub use intrinsics::*;
pub use stdlib::*;
