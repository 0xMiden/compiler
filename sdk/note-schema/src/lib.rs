//! Host-side access to note storage schemas embedded in Miden packages.
//!
//! # Felt layout
//!
//! The layout is structural over the resolved WIT type tree. The record
//! `miden:base/core-types@1.0.0.felt` is one felt. A `u64` uses two felts in low-then-high
//! `u32` limb order. A `u32`, `u8`, or `bool` uses one range-checked felt. An `option<T>` uses one
//! tag felt followed by its payload when present. A variant uses one declaration-ordinal tag felt
//! followed by the selected case payload. Record fields concatenate in declaration order.
//!
//! Encoding and decoding use [`miden_field_repr::FeltReader`] and
//! [`miden_field_repr::FeltWriter`]. Codecs only parse, display, and validate values. They do not
//! change this layout.

#![deny(missing_docs)]

mod builder;
mod codec;
#[cfg(feature = "codec-component")]
mod codec_component;
mod error;
mod schema;
mod value;

#[cfg(test)]
mod tests;

pub use builder::NoteStorageBuilder;
pub use codec::{
    ACCOUNT_ID_FQN, ASSET_AMOUNT_FQN, CodecRegistry, ConsumerTypeCodec, FELT_FQN, WORD_FQN,
};
pub use error::{Error, Result};
pub use miden_field::Felt;
pub use miden_protocol::note::NoteStorage;
pub use schema::{
    FeltLayout, NoteStorageSchema, PrimitiveType, SchemaCase, SchemaField, SchemaType,
    SchemaTypeKind,
};
pub use value::{DecodedValue, DecodedValueKind};
