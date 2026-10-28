//! Typed host bindings for Miden package schemas.
//!
//! Generate bindings from a built package, and then use the generated types. The schema section
//! the package carries selects what is generated:
//!
//! - a note package: the note's storage root type, which converts to and from `NoteStorage`;
//! - an account component package: a `Storage` root with one field per storage slot, which
//!   builds the component's [`InitStorageData`] and decodes slot words.
//!
//! ```ignore
//! miden_bindings::from_package!("../p2id-note/target/miden/release/p2id.masp");
//!
//! fn decode(storage: &miden_bindings::NoteStorage) -> P2idNote {
//!     P2idNote::from_note_storage(storage).unwrap()
//! }
//! ```
//!
//! ```ignore
//! miden_bindings::from_project!("../storage-example");
//!
//! fn init_data(owner: miden_bindings::Word) -> miden_bindings::InitStorageData {
//!     Storage { owner_public_key: owner, asset_qty_map: vec![] }
//!         .to_init_storage_data()
//!         .unwrap()
//! }
//! ```

#![deny(missing_docs)]

pub use miden_account_storage_schema::{AccountStorageSchema, InitStorageData};
#[doc(hidden)]
pub use miden_bindings_macros::from_wit_text;
pub use miden_bindings_macros::{from_package, from_project};
pub use miden_field_repr::*;
pub use miden_note_schema::{CodecRegistry, Error, NoteStorage, Result};
pub use miden_protocol::{account, address, asset};

/// Support used by generated typed bindings.
#[doc(hidden)]
pub mod __private {
    pub use miden_account_storage_schema;
    pub use miden_field;
    pub use miden_field_repr;
    pub use miden_note_schema;
    pub use miden_protocol;
    pub use miden_schema;
}
