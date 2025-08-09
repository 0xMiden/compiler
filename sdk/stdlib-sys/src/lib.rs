#![no_std]
#![deny(warnings)]

extern crate alloc;

pub mod intrinsics;
mod stdlib;
#[cfg(feature = "wit")]
pub mod stdlib_wit;

pub use intrinsics::{assert_eq, Digest, Felt, Word, WordAligned};
pub use stdlib::*;
