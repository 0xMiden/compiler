//! Canonical WIT contract for Miden note codecs.

#![deny(missing_docs)]
#![no_std]

/// The WIT document implemented by note codec components.
pub const NOTE_CODEC_WIT: &str =
    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/wit/note-codec.wit"));
