#![no_std]
#![cfg_attr(all(target_family = "wasm", miden), feature(linkage))]
#![deny(warnings)]

extern crate alloc;

pub mod raw;
mod stdlib;

pub use miden_intrinsics_sys::{
    ElementPtr, Felt, Word, WordAligned, felt,
    intrinsics::{advice::emit_falcon_sig_to_stack, assert, assert_eq, assertz},
};
pub use stdlib::*;

/// The compiler intrinsics of [`miden_intrinsics_sys::intrinsics`], at the path they had when
/// they were defined in this crate.
pub mod intrinsics {
    pub use miden_intrinsics_sys::intrinsics::*;

    pub use crate::Digest;

    pub mod crypto {
        //! [`Digest`] and the Poseidon2 [`merge`], at the path they had before the compiler
        //! intrinsics moved to `miden-intrinsics-sys`. Neither is an intrinsic: [`merge`] calls
        //! the core library's `::miden::core::crypto::hashes::poseidon2::merge`.

        pub use crate::{Digest, poseidon2::merge};
    }
}
