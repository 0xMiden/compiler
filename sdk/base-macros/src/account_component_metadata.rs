use std::collections::BTreeSet;

use miden_protocol::account::{
    AccountType, StorageSlotName,
    component::{
        AccountComponentMetadata, MapSlotSchema, StorageSchema, StorageSlotSchema, ValueSlotSchema,
        WordSchema, storage::SchemaTypeId,
    },
};
use semver::Version;

use crate::{component_macro::typecheck_storage_field, types::StorageFieldType};

pub struct AccountComponentMetadataBuilder {
    /// The human-readable name of the component.
    name: String,

    /// A brief description of what this component is and how it works.
    description: String,

    /// The version of the component using semantic versioning.
    /// This can be used to track and manage component upgrades.
    version: Version,

    /// A set of supported target account types for this component.
    supported_types: BTreeSet<AccountType>,

    /// Storage schema entries defining the component's storage layout.
    storage: Vec<(StorageSlotName, StorageSlotSchema)>,
}

impl AccountComponentMetadataBuilder {
    /// Adds a supported account type to this component metadata.
    pub fn add_supported_type(&mut self, account_type: AccountType) {
        self.supported_types.insert(account_type);
    }

    pub fn new(name: String, version: Version, description: String) -> Self {
        AccountComponentMetadataBuilder {
            name,
            description,
            version,
            supported_types: BTreeSet::new(),
            storage: Vec::new(),
        }
    }

    pub fn add_storage_entry(
        &mut self,
        slot_name: StorageSlotName,
        description: Option<String>,
        field: &syn::Field,
        field_type_attr: Option<String>,
    ) {
        match typecheck_storage_field(field) {
            Ok(StorageFieldType::StorageMap) => {
                if let Some(description) = description {
                    let key_schema = WordSchema::new_simple(SchemaTypeId::native_word());
                    let value_schema = WordSchema::new_simple(SchemaTypeId::native_word());
                    let slot_schema = StorageSlotSchema::Map(MapSlotSchema::new(
                        Some(description),
                        None,
                        key_schema,
                        value_schema,
                    ));
                    self.storage.push((slot_name, slot_schema));
                } else {
                    let key_schema = WordSchema::new_simple(SchemaTypeId::native_word());
                    let value_schema = WordSchema::new_simple(SchemaTypeId::native_word());
                    let slot_schema = StorageSlotSchema::Map(MapSlotSchema::new(
                        None,
                        None,
                        key_schema,
                        value_schema,
                    ));
                    self.storage.push((slot_name, slot_schema));
                }
            }
            Ok(StorageFieldType::Storage) => {
                let r#type = if let Some(field_type) = field_type_attr.as_deref() {
                    SchemaTypeId::new(field_type)
                        .unwrap_or_else(|_| panic!("well formed attribute type {field_type}"))
                } else {
                    SchemaTypeId::native_word()
                };

                let word_schema = WordSchema::new_simple(r#type);
                let slot_schema =
                    StorageSlotSchema::Value(ValueSlotSchema::new(description, word_schema));
                self.storage.push((slot_name, slot_schema));
            }
            Err(err) => panic!("invalid field type for storage: {err}"),
        }
    }

    pub fn build(self) -> AccountComponentMetadata {
        let storage_schema =
            StorageSchema::new(self.storage).expect("failed to build component storage schema");
        AccountComponentMetadata::new(
            self.name,
            self.description,
            self.version,
            self.supported_types,
            storage_schema,
        )
    }
}
