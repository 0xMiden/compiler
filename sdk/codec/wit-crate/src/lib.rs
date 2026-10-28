//! Canonical WIT contract for Miden schema codecs.

#![deny(missing_docs)]
#![no_std]

/// The WIT document implemented by codec components.
pub const CODEC_WIT: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/wit/codec.wit"));
