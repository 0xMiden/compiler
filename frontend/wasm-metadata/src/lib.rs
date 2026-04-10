//! Shared encoding for frontend-only Wasm metadata emitted by SDK macros.

#![deny(warnings)]
#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]
#![no_std]

extern crate alloc;

use alloc::{string::String, vec::Vec};
use core::fmt;

/// Name of the Wasm custom section used to store frontend metadata bytes.
pub const CUSTOM_SECTION_NAME: &str = "rodata,miden_account_component_frontend";

/// Frontend-only metadata emitted by the SDK macros into a dedicated Wasm custom section.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct FrontendMetadata {
    /// Export name that must be marked with the protocol's `@auth_script` attribute.
    pub auth_export_name: Option<String>,
    /// Export name that must be marked with the protocol's `@note_script` attribute.
    pub note_script_export_name: Option<String>,
}

impl FrontendMetadata {
    /// Returns true if `export_name` is the authentication export selected by frontend metadata.
    pub fn is_auth_export(&self, export_name: &str) -> bool {
        self.auth_export_name.as_deref() == Some(export_name)
    }

    /// Returns true if `export_name` is the note-script export selected by frontend metadata.
    pub fn is_note_script_export(&self, export_name: &str) -> bool {
        self.note_script_export_name.as_deref() == Some(export_name)
    }

    /// Encodes this metadata into the bytes stored in the frontend metadata custom section.
    pub fn to_bytes(&self) -> Result<Vec<u8>, FrontendMetadataError> {
        let mut bytes = Vec::new();
        encode_optional_export_name(&mut bytes, self.auth_export_name.as_deref(), "auth")?;
        encode_optional_export_name(
            &mut bytes,
            self.note_script_export_name.as_deref(),
            "note-script",
        )?;

        Ok(bytes)
    }

    /// Decodes frontend metadata from the bytes stored in the frontend metadata custom section.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, FrontendMetadataError> {
        if bytes.is_empty() {
            return Err(FrontendMetadataError::EmptySection);
        }

        let mut bytes = bytes;
        let auth_export_name = read_optional_export_name(&mut bytes, "auth")?;
        let note_script_export_name = read_optional_export_name(&mut bytes, "note-script")?;
        if !bytes.is_empty() {
            return Err(FrontendMetadataError::TrailingBytes);
        }

        Ok(Self {
            auth_export_name,
            note_script_export_name,
        })
    }
}

/// Errors that can occur while encoding or decoding frontend metadata bytes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FrontendMetadataError {
    /// The metadata section had no bytes.
    EmptySection,
    /// The metadata section ended earlier than required by the payload format.
    Truncated,
    /// The metadata section contained extra bytes after a valid payload.
    TrailingBytes,
    /// An export name in the metadata payload was not valid UTF-8.
    InvalidUtf8,
    /// The payload declared more than one export for a single export kind.
    TooManyExports {
        /// Human-readable export kind used in diagnostics.
        export_kind: &'static str,
    },
    /// An encoded export name exceeded the payload format's fixed length limit.
    ExportNameTooLong {
        /// Human-readable export kind used in diagnostics.
        export_kind: &'static str,
    },
}

impl fmt::Display for FrontendMetadataError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptySection => f.write_str("component frontend metadata section is empty"),
            Self::Truncated => f.write_str("component frontend metadata section is truncated"),
            Self::TrailingBytes => {
                f.write_str("component frontend metadata section has trailing bytes")
            }
            Self::InvalidUtf8 => f.write_str("component frontend metadata must be valid UTF-8"),
            Self::TooManyExports { export_kind } => {
                write!(f, "component frontend metadata supports at most one {export_kind} export")
            }
            Self::ExportNameTooLong { export_kind } => {
                write!(
                    f,
                    "component frontend metadata supports {export_kind} export names up to 255 \
                     bytes"
                )
            }
        }
    }
}

/// Appends an optional export name to the serialized frontend metadata payload.
fn encode_optional_export_name(
    bytes: &mut Vec<u8>,
    export_name: Option<&str>,
    export_kind: &'static str,
) -> Result<(), FrontendMetadataError> {
    bytes.push(u8::from(export_name.is_some()));

    if let Some(export_name) = export_name {
        let name_bytes = export_name.as_bytes();
        let name_len = u8::try_from(name_bytes.len())
            .map_err(|_| FrontendMetadataError::ExportNameTooLong { export_kind })?;
        bytes.push(name_len);
        bytes.extend_from_slice(name_bytes);
    }

    Ok(())
}

/// Reads an optional export name from the serialized frontend metadata payload.
fn read_optional_export_name(
    bytes: &mut &[u8],
    export_kind: &'static str,
) -> Result<Option<String>, FrontendMetadataError> {
    let Some((&export_count, rest)) = bytes.split_first() else {
        return Err(FrontendMetadataError::Truncated);
    };
    *bytes = rest;

    if export_count > 1 {
        return Err(FrontendMetadataError::TooManyExports { export_kind });
    }

    if export_count == 0 {
        return Ok(None);
    }

    let Some((&name_len, rest)) = bytes.split_first() else {
        return Err(FrontendMetadataError::Truncated);
    };
    *bytes = rest;

    let name_len = name_len as usize;
    if bytes.len() < name_len {
        return Err(FrontendMetadataError::Truncated);
    }

    let (name_bytes, rest) = bytes.split_at(name_len);
    let name = core::str::from_utf8(name_bytes).map_err(|_| FrontendMetadataError::InvalidUtf8)?;
    *bytes = rest;

    Ok(Some(String::from(name)))
}

#[cfg(test)]
mod tests {
    use alloc::string::ToString;

    use super::*;

    /// Ensures the shared encoder and decoder remain synchronized for the metadata payload format.
    #[test]
    fn frontend_metadata_roundtrips_payload() {
        let metadata = FrontendMetadata {
            auth_export_name: Some("auth".to_string()),
            note_script_export_name: Some("note-script".to_string()),
        };

        let bytes = metadata.to_bytes().unwrap();

        assert_eq!(FrontendMetadata::from_bytes(&bytes).unwrap(), metadata);
    }
}
