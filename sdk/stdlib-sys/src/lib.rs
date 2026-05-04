#![no_std]
#![cfg_attr(all(target_family = "wasm", miden), feature(linkage))]
#![deny(warnings)]

extern crate alloc;

pub mod intrinsics;
mod stdlib;

pub use intrinsics::{
    Digest, Felt, Word, WordAligned, advice::emit_falcon_sig_to_stack, assert, assert_eq, assertz,
};
pub use stdlib::*;
