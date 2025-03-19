use std::collections::BTreeSet;

use miden_objects::account::{AccountComponentMetadata, AccountType, StorageEntry};
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

    pub fn name(&self) -> String {
        self.name.clone()
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
