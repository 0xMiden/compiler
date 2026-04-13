//! Shared encoding for frontend-only Wasm metadata emitted by SDK macros.

#![deny(warnings)]
#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]
#![no_std]

extern crate alloc;

use alloc::{string::String, vec::Vec};

use serde::{Deserialize, Serialize};

/// Name of the Wasm custom section used to store frontend metadata bytes.
pub const WASM_FRONTEND_METADATA_CUSTOM_SECTION_NAME: &str =
    "rodata,miden_account_component_frontend";

/// Frontend-only metadata emitted by the SDK macros into a dedicated Wasm custom section.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FrontendMetadata {
    /// Metadata for the single export marked with `#[auth_script]`.
    AuthScript {
        /// Fully-qualified Rust method path marked with `#[auth_script]`.
        method_path: String,
        /// Name of the export marked with `#[auth_script]`.
        export_name: String,
    },
    /// Metadata for the single export marked with `#[note_script]`.
    NoteScript {
        /// Fully-qualified Rust method path marked with `#[note_script]`.
        method_path: String,
        /// Name of the export marked with `#[note_script]`.
        export_name: String,
    },
}

impl FrontendMetadata {
    /// Returns true if `export_name` is the authentication export selected by frontend metadata.
    pub fn is_auth_export(&self, export_name: &str) -> bool {
        matches!(self, Self::AuthScript { export_name: marked_export_name, .. } if marked_export_name == export_name)
    }

    /// Returns true if `export_name` is the note-script export selected by frontend metadata.
    pub fn is_note_script_export(&self, export_name: &str) -> bool {
        matches!(self, Self::NoteScript { export_name: marked_export_name, .. } if marked_export_name == export_name)
    }

    /// Returns the Rust method path marked by this metadata entry.
    pub fn method_path(&self) -> &str {
        match self {
            Self::AuthScript { method_path, .. } | Self::NoteScript { method_path, .. } => {
                method_path
            }
        }
    }

    /// Returns the export name marked by this metadata entry.
    pub fn export_name(&self) -> &str {
        match self {
            Self::AuthScript { export_name, .. } | Self::NoteScript { export_name, .. } => {
                export_name
            }
        }
    }

    /// Encodes this metadata into the bytes stored in the frontend metadata custom section.
    pub fn to_bytes(&self) -> Result<Vec<u8>, FrontendMetadataError> {
        serde_json::to_vec(self)
    }

    /// Decodes frontend metadata from the bytes stored in the frontend metadata custom section.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, FrontendMetadataError> {
        serde_json::from_slice(bytes)
    }
}

/// Errors that can occur while encoding or decoding frontend metadata bytes.
pub type FrontendMetadataError = serde_json::Error;

#[cfg(test)]
mod tests {
    use alloc::string::ToString;

    use super::*;

    /// Ensures the shared encoder and decoder remain synchronized for the metadata payload format.
    #[test]
    fn frontend_metadata_roundtrips_payload() {
        let metadata = [
            FrontendMetadata::AuthScript {
                method_path: "crate::auth::AuthComponent::authenticate".to_string(),
                export_name: "auth".to_string(),
            },
            FrontendMetadata::NoteScript {
                method_path: "crate::notes::PaymentNote::execute".to_string(),
                export_name: "note-script".to_string(),
            },
        ];

        for metadata in metadata {
            let bytes = metadata.to_bytes().unwrap();

            assert_eq!(FrontendMetadata::from_bytes(&bytes).unwrap(), metadata);
        }
    }
}
