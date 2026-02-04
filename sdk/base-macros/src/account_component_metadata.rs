use std::collections::BTreeSet;

use miden_protocol::account::{
    AccountType, StorageSlotName,
    component::{
        AccountComponentMetadata, MapSlotSchema, StorageSchema, StorageSlotSchema, ValueSlotSchema,
        WordSchema, storage::SchemaTypeId,
    },
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
        field_type: &syn::Type,
        field_type_attr: Option<String>,
    ) {
        let type_path = if let syn::Type::Path(type_path) = field_type {
            type_path
        } else {
            panic!("failed to get type path {field_type:?}")
        };

        if let Some(segment) = type_path.path.segments.last() {
            let type_name = segment.ident.to_string();
            match type_name.as_str() {
                "StorageMap" => {
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
                "Value" => {
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
                _ => panic!("unexpected field type: {type_name}"),
            }
        } else {
            panic!("failed to get last segment of the type path {type_path:?}")
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
