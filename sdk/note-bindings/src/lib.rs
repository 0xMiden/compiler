//! Typed host bindings for Miden note storage schemas.
//!
//! Generate bindings from a built package, and then use the generated storage type.
//!
//! ```ignore
//! miden_note_bindings::from_package!("../p2id-note/target/miden/release/p2id.masp");
//!
//! fn decode(storage: &miden_note_bindings::NoteStorage) -> P2idNote {
//!     P2idNote::from_note_storage(storage).unwrap()
//! }
//! ```

#![deny(missing_docs)]

pub use miden_field_repr::*;
#[doc(hidden)]
pub use miden_note_bindings_macros::from_wit_text;
pub use miden_note_bindings_macros::{from_package, from_project};
pub use miden_note_schema::{CodecRegistry, Error, NoteStorage, Result};
pub use miden_protocol::{account, address, asset};

/// Support used by generated typed bindings.
#[doc(hidden)]
pub mod __private {
    pub use miden_field;
    pub use miden_field_repr;
    pub use miden_note_schema;
    pub use miden_protocol;
}
