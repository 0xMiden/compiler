//! Low-level Rust bindings for the Miden compiler's intrinsics, and the support types generated
//! bindings use.
//!
//! The intrinsics ([`intrinsics`]) are part of the compiler, not of a Miden package, so their
//! bindings are written by hand here rather than generated from a package manifest. The crate's
//! build script compiles the matching linker stubs (`stubs/`) into an archive that every
//! dependent links.
#![no_std]
#![cfg_attr(all(target_family = "wasm", miden), feature(linkage))]
#![deny(warnings)]

mod constants;
mod element_ptr;
pub mod intrinsics;
mod word_aligned;

pub use miden_field::{Felt, Word};

pub use self::{
    constants::{FeltConstant, WordConstant},
    element_ptr::ElementPtr,
    word_aligned::WordAligned,
};

/// The items generated bindings refer to, under one path: the SDK's `Felt` and `Word`, the
/// word-aligned return-area wrapper, the typed element address, and the constant carriers.
pub mod support {
    pub use crate::{ElementPtr, Felt, FeltConstant, Word, WordAligned, WordConstant};
}
