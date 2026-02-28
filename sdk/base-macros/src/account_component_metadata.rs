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
    ) -> Result<(), syn::Error> {
        match typecheck_storage_field(field)? {
            StorageFieldType::StorageMap => {
                let args = extract_storage_type_args(field)?;
                let key_schema = args
                    .first()
                    .map(word_schema_from_storage_type_arg)
                    .unwrap_or_else(|| WordSchema::new_simple(SchemaTypeId::native_word()));
                let value_schema = args
                    .get(1)
                    .map(word_schema_from_storage_type_arg)
                    .unwrap_or_else(|| WordSchema::new_simple(SchemaTypeId::native_word()));
                let slot_schema = StorageSlotSchema::Map(MapSlotSchema::new(
                    description,
                    None,
                    key_schema,
                    value_schema,
                ));
                self.storage.push((slot_name, slot_schema));
            }
            StorageFieldType::Storage => {
                let r#type = if let Some(field_type) = field_type_attr.as_deref() {
                    SchemaTypeId::new(field_type).map_err(|err| {
                        syn::Error::new(
                            field.span(),
                            format!("invalid storage schema type identifier '{field_type}': {err}"),
                        )
                    })?
                } else {
                    let args = extract_storage_type_args(field)?;
                    args.first()
                        .map(schema_type_id_from_storage_type_arg)
                        .unwrap_or_else(SchemaTypeId::native_word)
                };

                let word_schema = WordSchema::new_simple(r#type);
                let slot_schema =
                    StorageSlotSchema::Value(ValueSlotSchema::new(description, word_schema));
                self.storage.push((slot_name, slot_schema));
            }
        }

        Ok(())
    }

    /// Builds a new [`AccountComponentMetadata`].
    pub fn build(self, span: Span) -> Result<AccountComponentMetadata, syn::Error> {
        let storage_schema = StorageSchema::new(self.storage).map_err(|err| {
            syn::Error::new(span, format!("failed to build component storage schema: {err}"))
        })?;

        Ok(AccountComponentMetadata::new(
            self.name,
            self.description,
            self.version,
            self.supported_types,
            storage_schema,
        ))
    }
}
