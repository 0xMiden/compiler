//! Serialization/deserialization for felt representation in off-chain use.
//!
//! This crate provides traits and utilities for converting Rust types to and from
//! a sequence of `Felt` elements, suitable for preparing data to send to on-chain code.

#![no_std]
#![deny(warnings)]

mod account_id;

pub use account_id::AccountIdFeltRepr;
pub use miden_felt_repr::{Felt, FeltReader, FeltWriter, FromFeltRepr, ToFeltRepr};
/// Re-export the derive macros with the same name as the traits.
pub use miden_felt_repr_derive::DeriveFromFeltReprOffchain as FromFeltRepr;
pub use miden_felt_repr_derive::DeriveToFeltReprOffchain as ToFeltRepr;
