#![no_std]
#![deny(warnings)]

extern crate alloc;

pub mod intrinsics;
mod stdlib;

pub use intrinsics::{
    advice::emit_falcon_sig_to_stack, assert_eq, Digest, Felt, Word, WordAligned,
};
pub use stdlib::*;
