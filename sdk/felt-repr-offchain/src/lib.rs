//! Serialization into felt representation for off-chain use.
//!
//! This crate provides serialization of types into their felt memory representation,
//! which can be used for deserialization on-chain via `miden-felt-repr-onchain`.

#![no_std]
#![deny(warnings)]

extern crate alloc;

mod account_id;

use alloc::{vec, vec::Vec};

pub use account_id::AccountIdFeltRepr;
use miden_core::Felt;
/// Re-export the derive macro with the same name as the trait.
pub use miden_felt_repr_derive::DeriveToFeltRepr as ToFeltRepr;

/// Trait for serializing a type into its felt memory representation.
pub trait ToFeltRepr {
    /// Serializes this value into a vector of `Felt` elements.
    fn to_felt_repr(&self) -> Vec<Felt>;
}

/// Base implementation for `Felt` itself.
impl ToFeltRepr for Felt {
    fn to_felt_repr(&self) -> Vec<Felt> {
        vec![*self]
    }
}
