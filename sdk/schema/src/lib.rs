//! Host-side access to the WIT schemas embedded in Miden packages.
//!
//! A schema describes a flat sequence of felts, such as the storage of a note, as a WIT record.
//! This crate holds the part that does not depend on what the felts are: the resolved schema
//! model, value building and decoding, the codec registry, and the loader for author codec
//! components. The kind layers, such as `miden-note-schema`, add the kind-specific types on top.
//!
//! # Profiles
//!
//! A [`SchemaProfile`] names one schema kind: the package section that carries its WIT, the WIT
//! interface and root type alias the schema must define, the [`SchemaLimits`] it is held to, and
//! the name diagnostics use. Every [`Schema`] is resolved with one profile.
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
//!
//! # Features
//!
//! - `protocol` (default): everything that produces or consumes values of protocol types — the
//!   [`ValueBuilder`], decoding to [`DecodedValue`], and the [`CodecRegistry`] with its standard
//!   codecs. It is what makes this crate depend on `miden-protocol`.
//! - `codec-component`: loading author-supplied codec components with Wasmtime. Implies
//!   `protocol`.
//!
//! Without `protocol` the crate is the schema model alone: resolving and validating a schema
//! ([`Schema`]), the structural validation of a codec component ([`validate_codec_component`]),
//! and the limits both are held to. That is all the compiler uses, and it depends on this crate
//! with the feature off so that it does not link the protocol.

#![deny(missing_docs)]

mod artifact;
#[cfg(feature = "protocol")]
mod builder;
#[cfg(feature = "protocol")]
mod codec;
#[cfg(feature = "codec-component")]
mod codec_component;
mod codec_structure;
mod error;
mod schema;
mod section;
mod standard_leaf;
#[cfg(feature = "protocol")]
mod value;

#[cfg(all(test, feature = "protocol"))]
mod tests;

pub use artifact::{PackageArtifact, PackageResolver};
#[cfg(feature = "protocol")]
pub use builder::ValueBuilder;
#[cfg(feature = "protocol")]
pub use codec::{CodecRegistry, ConsumerTypeCodec};
#[cfg(feature = "codec-component")]
pub use codec_component::CodecLimits;
pub use codec_structure::{
    CODEC_GUEST_RUSTFLAGS, CODEC_WASM_FEATURES, MAX_CODEC_COMPONENT_BYTES,
    validate_codec_component, validate_codec_structure,
};
pub use error::{CodecFailure, Error, Result};
pub use miden_field::Felt;
pub use schema::{
    FeltLayout, PrimitiveType, Schema, SchemaCase, SchemaField, SchemaLimits, SchemaProfile,
    SchemaType, SchemaTypeKind,
};
pub use standard_leaf::{ACCOUNT_ID_FQN, ASSET_AMOUNT_FQN, FELT_FQN, StandardLeaf, WORD_FQN};
#[cfg(feature = "protocol")]
pub use value::{DecodedValue, DecodedValueKind};
