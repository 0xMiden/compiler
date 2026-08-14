//! Shared definitions for the out-of-band metadata exchanged between the Miden SDK macros and
//! the compiler: Wasm custom-section names and encodings, and the package-section payloads
//! carried through the compiler pipeline into the compiled Miden package (`.masp`).

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

/// Name of the Wasm custom section used to store the serialized AccountComponentMetadata.
pub const WASM_ACCOUNT_COMPONENT_METADATA_CUSTOM_SECTION_NAME: &str = "rodata,miden_account";

/// Name of the Wasm custom section used to store the component's public WIT source.
pub const WASM_COMPONENT_WIT_CUSTOM_SECTION_NAME: &str = "rodata,miden_wit";

/// Name of the Miden package (`.masp`) section that carries the component's public WIT source.
pub const PACKAGE_WIT_SECTION_ID: &str = "wit";

/// Returns the Miden package section id that carries the component's public WIT source.
///
/// One accessor for every producer and consumer, so the planned upstream `SectionId::WIT`
/// changes a single place.
pub fn package_wit_section_id() -> miden_mast_package::SectionId {
    miden_mast_package::SectionId::custom(PACKAGE_WIT_SECTION_ID)
        .expect("the WIT section id must be a valid custom section id")
}

/// The filesystem package-cache exchange contract.
///
/// The compiler publishes compiled dependency packages — and its recorded dependency
/// resolution — into the directory named by [`package_cache::PACKAGE_CACHE_ENV`]; the SDK
/// macros and the contract build script consume them. Every spelling of that contract lives
/// here so the producer and the consumers cannot drift apart. (The contract build script is
/// the one deliberate exception: it is dependency-free by design and spells the same values
/// inline.)
pub mod package_cache {
    use alloc::{format, string::String};

    /// Environment variable naming the package-cache directory a build exchanges packages
    /// through. An empty value counts as unset everywhere.
    pub const PACKAGE_CACHE_ENV: &str = "MIDENC_PACKAGE_CACHE";

    /// Subdirectory of the cache holding the compiler-recorded per-consumer files.
    pub const DEPENDENCY_MANIFEST_DIR: &str = "miden-deps";

    /// Schema version of the dependency artifact map.
    pub const DEPENDENCY_MAP_SCHEMA: i64 = 1;

    /// File name of a consumer's dependency artifact map inside
    /// [`DEPENDENCY_MANIFEST_DIR`].
    pub fn dependency_map_file_name(consumer: &str) -> String {
        format!("{consumer}.deps.toml")
    }

    /// File name of a consumer's watch list inside [`DEPENDENCY_MANIFEST_DIR`].
    pub fn watch_file_name(consumer: &str) -> String {
        format!("{consumer}.watch")
    }

    /// File name of a consumer's recorded cargo dep-info inside
    /// [`DEPENDENCY_MANIFEST_DIR`].
    ///
    /// A compiler-internal record: each consumer's nested cargo build copies the artifact's
    /// consolidated dep-info here, and the root consumer's watch list is assembled from
    /// these records. Neither the SDK macros nor the contract build script read it.
    pub fn dependency_dep_info_file_name(consumer: &str) -> String {
        format!("{consumer}.dep-info")
    }

    /// File name of a published package: the package name with the `.masp` extension
    /// appended.
    ///
    /// Append semantics on purpose: `Path::with_extension` would truncate a package name
    /// that contains a dot, and then the publisher and the artifact map would disagree
    /// about the file's name.
    pub fn package_file_name(package_name: &str) -> String {
        format!("{package_name}.{}", miden_mast_package::Package::EXTENSION)
    }
}

/// Returns the component WIT bytes embedded in a compiled Miden package, when present.
pub fn package_wit(package: &miden_mast_package::Package) -> Option<&[u8]> {
    let wit_section_id = package_wit_section_id();
    package
        .sections
        .iter()
        .find(|section| section.id == wit_section_id)
        .map(|section| section.data.as_ref())
}

/// Counts top-level `package <id>;` declarations in WIT source.
///
/// This is the shared contract between the SDK macros, which embed exactly one declaration per
/// `#[component]` implementation, and the Wasm frontend, which rejects a custom section whose
/// count is not one (two linked implementations concatenate their sections into invalid WIT).
///
/// Comments — including nested `/* */` block comments — are stripped first so commented-out
/// declarations are not counted, and nested package declarations (`package <id> { ... }`, whose
/// `{` precedes any `;`) are excluded. The scan is token-based: any whitespace may follow the
/// keyword, a declaration may span lines, and several declarations may share one line.
pub fn count_top_level_wit_packages(wit: &str) -> usize {
    top_level_wit_packages(wit).len()
}

/// The identifiers of every top-level `package <id>;` declaration in WIT source, in order.
///
/// Same scan as [`count_top_level_wit_packages`]; each identifier is the text between the
/// keyword and the terminating `;`, trimmed, with any `@<version>` suffix retained.
pub fn top_level_wit_packages(wit: &str) -> Vec<String> {
    let stripped = strip_wit_comments(wit);
    let bytes = stripped.as_bytes();
    let mut packages = Vec::new();
    let mut index = 0;
    while let Some(found) = stripped[index..].find("package") {
        let start = index + found;
        let end = start + "package".len();
        index = end;

        // Word boundaries keep identifiers such as `my-package` or `packaged` out.
        let preceded = start == 0
            || matches!(bytes[start - 1], b';' | b'}')
            || bytes[start - 1].is_ascii_whitespace();
        let followed = bytes.get(end).is_some_and(u8::is_ascii_whitespace);
        if !(preceded && followed) {
            continue;
        }

        // A top-level declaration is terminated by `;` before any `{`; the nested notation
        // (`package <id> { ... }`) opens its brace first.
        let rest = &stripped[end..];
        if let Some(semi) = rest.find(';')
            && rest.find('{').is_none_or(|brace| semi < brace)
        {
            packages.push(rest[..semi].trim().into());
            index = end + semi + 1;
        }
    }
    packages
}

/// The name of the first top-level `world <name>` declaration in WIT source, when present.
///
/// Token-based like [`top_level_wit_packages`]: comments are stripped first, and only
/// declarations at brace depth zero count — a world inside the nested package notation
/// (`package <id> { world w { ... } }`) is not a top-level world of the file.
pub fn first_top_level_wit_world(wit: &str) -> Option<String> {
    let stripped = strip_wit_comments(wit);
    let bytes = stripped.as_bytes();
    let mut depth = 0usize;
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'{' => depth += 1,
            b'}' => depth = depth.saturating_sub(1),
            // Anchored on the ASCII `w`, which is always a character boundary, so the slice
            // below cannot split a multi-byte character.
            b'w' if depth == 0 && stripped[index..].starts_with("world") => {
                let end = index + "world".len();
                let preceded = index == 0
                    || matches!(bytes[index - 1], b';' | b'}')
                    || bytes[index - 1].is_ascii_whitespace();
                let followed = bytes.get(end).is_some_and(u8::is_ascii_whitespace);
                if preceded && followed {
                    let name: String = stripped[end..]
                        .trim_start()
                        .chars()
                        .take_while(|ch| ch.is_alphanumeric() || *ch == '-' || *ch == '_')
                        .collect();
                    if !name.is_empty() {
                        return Some(name);
                    }
                }
                index = end;
                continue;
            }
            _ => {}
        }
        index += 1;
    }
    None
}

/// Strips `//` line comments and nested `/* */` block comments, preserving line structure.
///
/// A closed block comment leaves one space behind, so tokens it separated (for example
/// `package/*x*/miden:a;`) do not fuse.
fn strip_wit_comments(wit: &str) -> String {
    let mut stripped = String::with_capacity(wit.len());
    let mut chars = wit.chars().peekable();
    let mut block_depth = 0usize;
    while let Some(ch) = chars.next() {
        match ch {
            '/' if block_depth == 0 && chars.peek() == Some(&'/') => {
                for next in chars.by_ref() {
                    if next == '\n' {
                        stripped.push('\n');
                        break;
                    }
                }
            }
            '/' if chars.peek() == Some(&'*') => {
                chars.next();
                block_depth += 1;
            }
            '*' if block_depth > 0 && chars.peek() == Some(&'/') => {
                chars.next();
                block_depth -= 1;
                if block_depth == 0 {
                    stripped.push(' ');
                }
            }
            '\n' => stripped.push('\n'),
            _ if block_depth == 0 => stripped.push(ch),
            _ => {}
        }
    }
    stripped
}

/// Out-of-band payloads extracted from the input binary and attached to the compiled Miden
/// package as sections.
///
/// Carried through the compiler pipeline as one unit so that adding a payload does not require
/// threading a new field through every stage.
#[derive(Clone, Debug, Default)]
pub struct PackageSections {
    /// The serialized AccountComponentMetadata (name, description, storage layout, etc.).
    pub account_component_metadata: Option<Vec<u8>>,
    /// The component's public WIT source emitted by the `#[component]` macro.
    pub component_wit: Option<Vec<u8>>,
}

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
    /// Metadata for an export marked with `#[account_procedure]`.
    AccountProcedure {
        /// Fully-qualified Rust method path marked with `#[account_procedure]`.
        method_path: String,
        /// Name of the export marked with `#[account_procedure]`.
        export_name: String,
    },
    /// Metadata for the single export marked with `#[note_script]`.
    NoteScript {
        /// Fully-qualified Rust method path marked with `#[note_script]`.
        method_path: String,
        /// Name of the export marked with `#[note_script]`.
        export_name: String,
    },
    /// Metadata for the single entrypoint generated by `#[tx_script]`.
    TxScript {
        /// Fully-qualified Rust function path marked with `#[tx_script]`.
        method_path: String,
        /// Name of the export generated by `#[tx_script]`.
        export_name: String,
    },
}

/// Semantic kind of a protocol export selected by frontend metadata.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProtocolExportKind {
    /// The export is the component's authentication procedure.
    AuthScript,
    /// The export is one of the component's account-interface procedures.
    AccountProcedure,
    /// The export is the component's note-script entrypoint.
    NoteScript,
    /// The export is the transaction-script entrypoint.
    TxScript,
}

impl FrontendMetadata {
    /// Returns the semantic kind of protocol export selected by this metadata entry.
    pub fn protocol_export_kind(&self) -> ProtocolExportKind {
        match self {
            Self::AuthScript { .. } => ProtocolExportKind::AuthScript,
            Self::AccountProcedure { .. } => ProtocolExportKind::AccountProcedure,
            Self::NoteScript { .. } => ProtocolExportKind::NoteScript,
            Self::TxScript { .. } => ProtocolExportKind::TxScript,
        }
    }

    /// Returns the selected protocol-export kind when `export_name` matches the marked export.
    pub fn protocol_export_kind_for(&self, export_name: &str) -> Option<ProtocolExportKind> {
        (self.export_name() == export_name).then(|| self.protocol_export_kind())
    }

    /// Returns the Rust method path marked by this metadata entry.
    pub fn method_path(&self) -> &str {
        match self {
            Self::AuthScript { method_path, .. }
            | Self::AccountProcedure { method_path, .. }
            | Self::NoteScript { method_path, .. }
            | Self::TxScript { method_path, .. } => method_path,
        }
    }

    /// Returns the export name marked by this metadata entry.
    pub fn export_name(&self) -> &str {
        match self {
            Self::AuthScript { export_name, .. }
            | Self::AccountProcedure { export_name, .. }
            | Self::NoteScript { export_name, .. }
            | Self::TxScript { export_name, .. } => export_name,
        }
    }
}

/// Encodes a set of frontend metadata entries into the bytes stored in the frontend metadata custom
/// section. A component may carry several entries (e.g. an optional `#[auth_script]` entry plus one
/// entry per `#[account_procedure]` method), so the section payload is a list.
pub fn encode_section(entries: &[FrontendMetadata]) -> Result<Vec<u8>, FrontendMetadataError> {
    serde_json::to_vec(entries)
}

/// Decodes the frontend metadata entries stored in the frontend metadata custom section.
pub fn decode_section(bytes: &[u8]) -> Result<Vec<FrontendMetadata>, FrontendMetadataError> {
    serde_json::from_slice(bytes)
}

/// Errors that can occur while encoding or decoding frontend metadata bytes.
pub type FrontendMetadataError = serde_json::Error;

#[cfg(test)]
mod tests {
    use alloc::{format, string::ToString};

    use super::*;

    /// Ensures a single embedded component WIT source is recognized as one package.
    #[test]
    fn component_wit_counts_a_single_package_declaration() {
        let wit = r#"// This file is auto-generated by the `#[component]` macro.
package miden:basic-wallet@0.1.0;

use miden:base/core-types@1.0.0;

interface basic-wallet {
    receive-asset: func();
}

world basic-wallet-world {
    export basic-wallet;
}
"#;

        assert_eq!(count_top_level_wit_packages(wit), 1);
    }

    /// Ensures concatenated WIT sections (two linked `#[component]` implementations) are detected.
    #[test]
    fn component_wit_detects_concatenated_package_declarations() {
        let wit = r#"package miden:first@0.1.0;

world first-world {
}
package miden:second@0.1.0;

world second-world {
}
"#;

        assert_eq!(count_top_level_wit_packages(wit), 2);
    }

    /// Ensures nested package declarations and comments are not counted as top-level packages.
    #[test]
    fn component_wit_ignores_nested_packages_and_comments() {
        let wit = r#"// package miden:commented@0.1.0;
package miden:outer@0.1.0;

package miden:nested@0.1.0 {
    interface api {
        get: func() -> u64;
    }
}
"#;

        assert_eq!(count_top_level_wit_packages(wit), 1);
    }

    /// Ensures a one-line package declaration followed by other items on the same line is counted.
    ///
    /// WIT is whitespace-insensitive; the `{` of a following item must not disqualify the
    /// declaration, while a nested `package <id> { ... }` (whose `{` precedes any `;`) still must.
    #[test]
    fn component_wit_counts_one_line_package_declarations() {
        let wit = "package miden:x@1.0.0; interface api { get: func() -> u64; }\n\nworld w { \
                   export api; }\n";
        assert_eq!(count_top_level_wit_packages(wit), 1);

        let nested_one_liner =
            "package miden:nested@0.1.0 { interface api { get: func() -> u64; } }\n";
        assert_eq!(count_top_level_wit_packages(nested_one_liner), 0);

        let concatenated = "package miden:first@0.1.0; world first-world { }\npackage \
                            miden:second@0.1.0; world second-world { }\n";
        assert_eq!(count_top_level_wit_packages(concatenated), 2);
    }

    /// Ensures the scan is whitespace-shape-insensitive: tabs after the keyword, declarations
    /// split across lines, and a block comment separating the keyword from the id all count.
    #[test]
    fn component_wit_counts_whitespace_and_comment_variants() {
        assert_eq!(count_top_level_wit_packages("package\tmiden:tabbed@1.0.0;\n"), 1);
        assert_eq!(count_top_level_wit_packages("package\n    miden:split@1.0.0;\n"), 1);
        assert_eq!(count_top_level_wit_packages("package/*x*/miden:commented@1.0.0;\n"), 1);
        // Identifiers containing the keyword are not declarations.
        assert_eq!(count_top_level_wit_packages("interface my-package { f: func(); }\n"), 0);
    }

    /// Ensures declarations inside (nested) block comments are not counted as top-level packages.
    #[test]
    fn component_wit_ignores_block_commented_packages() {
        let wit = r#"/* legacy:
package miden:old@0.1.0;
/* nested comment with package miden:inner@0.1.0; */
still commented
*/
package miden:new@0.1.0;
"#;

        assert_eq!(count_top_level_wit_packages(wit), 1);
    }

    /// Ensures the collecting scan reports the declared identifiers, version suffix retained.
    #[test]
    fn top_level_wit_packages_reports_identifiers_in_order() {
        let concatenated = "package miden:first@0.1.0; world first-world { }\npackage \
                            miden:second@0.1.0; world second-world { }\n";
        assert_eq!(top_level_wit_packages(concatenated), [
            "miden:first@0.1.0".to_string(),
            "miden:second@0.1.0".to_string()
        ]);
        assert_eq!(top_level_wit_packages("package\tmiden:tabbed@1.0.0;\n"), [
            "miden:tabbed@1.0.0".to_string()
        ]);
    }

    /// Ensures world detection is token-based and depth-aware.
    #[test]
    fn first_top_level_wit_world_is_token_based_and_depth_aware() {
        assert_eq!(
            first_top_level_wit_world("package miden:x@1.0.0;\n\nworld\tmy-world {\n}\n")
                .as_deref(),
            Some("my-world"),
            "tabs after the keyword must not hide the world"
        );
        assert_eq!(
            first_top_level_wit_world("// world commented-out {}\nworld real { }\n").as_deref(),
            Some("real"),
            "commented-out worlds must be ignored"
        );
        assert_eq!(
            first_top_level_wit_world("package miden:nested@0.1.0 { world inner { } }\n"),
            None,
            "a world inside the nested package notation is not a top-level world of the file"
        );
        assert_eq!(
            first_top_level_wit_world("interface world-like { f: func(); }\n"),
            None,
            "identifiers containing the keyword are not declarations"
        );
    }

    /// Ensures byte-wise gluing without a boundary newline still exposes both declarations.
    ///
    /// The producers additionally wrap every embedded payload in boundary newlines
    /// (`normalize_embedded_wit` in the SDK macros), but the token-based scan no longer depends
    /// on it.
    #[test]
    fn component_wit_detects_concatenation_without_boundary_newlines() {
        let first_without_trailing_newline = "package miden:first@0.1.0;\n\nworld first-world {\n}";
        let second = "package miden:second@0.1.0;\n\nworld second-world {\n}\n";

        let glued = format!("{first_without_trailing_newline}{second}");
        assert_eq!(count_top_level_wit_packages(&glued), 2);
    }

    /// Ensures the shared encoder and decoder remain synchronized for the metadata payload format.
    #[test]
    fn frontend_metadata_roundtrips_payload() {
        let entries = [
            FrontendMetadata::AuthScript {
                method_path: "crate::auth::AuthComponent::authenticate".to_string(),
                export_name: "auth".to_string(),
            },
            FrontendMetadata::NoteScript {
                method_path: "crate::notes::PaymentNote::execute".to_string(),
                export_name: "note-script".to_string(),
            },
        ];

        for entry in entries {
            let entry = alloc::vec![entry];
            let bytes = encode_section(&entry).unwrap();

            assert_eq!(decode_section(&bytes).unwrap(), entry);
        }
    }

    /// Ensures a section carrying several `#[account_procedure]` entries round-trips through the
    /// section encoder and decoder.
    #[test]
    fn frontend_metadata_section_roundtrips_multiple_account_procedures() {
        let entries = alloc::vec![
            FrontendMetadata::AccountProcedure {
                method_path: "crate::wallet::BasicWallet::receive_asset".to_string(),
                export_name: "receive-asset".to_string(),
            },
            FrontendMetadata::AccountProcedure {
                method_path: "crate::wallet::BasicWallet::move_asset_to_note".to_string(),
                export_name: "move-asset-to-note".to_string(),
            },
            FrontendMetadata::AccountProcedure {
                method_path: "crate::wallet::BasicWallet::create_note".to_string(),
                export_name: "create-note".to_string(),
            },
        ];

        let bytes = encode_section(&entries).unwrap();

        assert_eq!(decode_section(&bytes).unwrap(), entries);
    }

    /// Ensures protocol-export matching preserves the semantic export kind.
    #[test]
    fn frontend_metadata_matches_protocol_export_kind() {
        let auth_metadata = FrontendMetadata::AuthScript {
            method_path: "crate::auth::AuthComponent::authenticate".to_string(),
            export_name: "auth".to_string(),
        };
        let note_metadata = FrontendMetadata::NoteScript {
            method_path: "crate::notes::PaymentNote::execute".to_string(),
            export_name: "note-script".to_string(),
        };

        assert_eq!(
            auth_metadata.protocol_export_kind_for("auth"),
            Some(ProtocolExportKind::AuthScript)
        );
        assert_eq!(auth_metadata.protocol_export_kind_for("other"), None);
        assert_eq!(
            note_metadata.protocol_export_kind_for("note-script"),
            Some(ProtocolExportKind::NoteScript)
        );
        assert_eq!(note_metadata.protocol_export_kind_for("other"), None);
    }
}
