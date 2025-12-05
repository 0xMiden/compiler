//! Serialization into felt representation for off-chain use.
//!
//! This crate provides serialization of types into their felt memory representation,
//! which can be used for zero-copy deserialization on-chain via `miden-felt-repr-onchain`.

#![no_std]
#![deny(warnings)]

extern crate alloc;

mod account_id;

use alloc::vec::Vec;

pub use account_id::AccountIdFeltRepr;
use miden_core::Felt;

/// Trait for serializing a type into its felt memory representation.
///
/// Implementors must ensure that the serialized `Vec<Felt>` matches the exact memory
/// layout of the type, so it can be used for zero-copy deserialization on-chain.
pub trait ToFeltRepr {
    /// Serializes this value into a vector of `Felt` elements matching the type's memory layout.
    fn to_felt_repr(&self) -> Vec<Felt>;
}
