use std::collections::BTreeSet;

use miden_objects::account::{
    AccountComponentMetadata, AccountType, MapRepresentation, StorageEntry, StoragePlaceholder,
    WordRepresentation,
};
use semver::Version;

pub struct AccountComponentMetadataBuilder {
    /// The human-readable name of the component.
    name: String,

    /// A brief description of what this component is and how it works.
    description: String,

    /// The version of the component using semantic versioning.
    /// This can be used to track and manage component upgrades.
    version: Version,

    /// A set of supported target account types for this component.
    targets: BTreeSet<AccountType>,

    /// A list of storage entries defining the component's storage layout and initialization
    /// values.
    storage: Vec<StorageEntry>,
}

// TODO: parse `description`, `version` and `targets` from Cargo.toml

impl AccountComponentMetadataBuilder {
    pub fn new(name: String) -> Self {
        AccountComponentMetadataBuilder {
            name,
            description: String::new(),
            version: Version::parse("0.0.1").unwrap(),
            targets: BTreeSet::new(),
            storage: Vec::new(),
        }
    }

    pub fn add_storage_entry(
        &mut self,
        name: &str,
        description: Option<String>,
        slot: u8,
        field_type: &syn::Type,
        field_type_str: Option<String>,
    ) {
        // TODO: store field_type_str
        let type_path = if let syn::Type::Path(type_path) = field_type {
            Some(type_path)
        } else {
            None
        };

        if let Some(type_path) = type_path {
            if let Some(segment) = type_path.path.segments.last() {
                let type_name = segment.ident.to_string();

                match type_name.as_str() {
                    "StorageMap" => {
                        if let Ok(entry) = StorageEntry::new_map(
                            name.to_string(),
                            description,
                            slot,
                            MapRepresentation::Template(StoragePlaceholder::new("key").unwrap()),
                        ) {
                            self.storage.push(entry);
                        }
                    }
                    "Value" => {
                        self.storage.push(StorageEntry::new_value(
                            name.to_string(),
                            description,
                            slot,
                            WordRepresentation::Template(
                                StoragePlaceholder::new("map_key").unwrap(),
                            ),
                        ));
                    }
                    _ => panic!("unexpected field type: {}", type_name),
                }
            }
        }
    }

    pub fn build(self) -> AccountComponentMetadata {
        AccountComponentMetadata::new(
            self.name,
            self.description,
            self.version,
            self.targets,
            self.storage,
        )
        .expect("failed to build AccountComponentMetadata")
    }
}
