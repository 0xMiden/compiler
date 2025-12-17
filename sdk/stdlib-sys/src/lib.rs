#![no_std]
#![deny(warnings)]

extern crate alloc;

pub mod intrinsics;
mod stdlib;

pub use intrinsics::{
    Digest, Felt, Word, WordAligned, advice::emit_falcon_sig_to_stack, assert_eq,
};
pub use stdlib::*;
