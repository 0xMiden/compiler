//! Serialization/deserialization for felt representation in on-chain execution.

#![no_std]
#![deny(warnings)]

pub use miden_felt_repr::{Felt, FeltReader, FeltWriter, FromFeltRepr, ToFeltRepr};
/// Re-export the derive macros with the same name as the traits.
pub use miden_felt_repr_derive::DeriveFromFeltReprOnchain as FromFeltRepr;
pub use miden_felt_repr_derive::DeriveToFeltReprOnchain as ToFeltRepr;
