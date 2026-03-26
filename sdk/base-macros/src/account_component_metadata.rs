use std::collections::BTreeSet;

use miden_protocol::account::{
    AccountType, StorageSlotName,
    component::{
        AccountComponentMetadata, MapSlotSchema, StorageSchema, StorageSlotSchema, ValueSlotSchema,
        WordSchema, storage::SchemaType,
    },
};
use proc_macro2::Span;
use semver::Version;
use syn::spanned::Spanned;

use crate::{component_macro::typecheck_storage_field, types::StorageFieldType};

/// Extracts the generic type arguments from a storage field declaration.
fn extract_storage_type_args(field: &syn::Field) -> Result<Vec<syn::Type>, syn::Error> {
    let type_path = match &field.ty {
        syn::Type::Path(type_path) => type_path,
        _ => return Err(syn::Error::new(field.span(), "storage field type must be a path")),
    };

    let last_segment = type_path
        .path
        .segments
        .last()
        .ok_or_else(|| syn::Error::new(field.span(), "storage field type must be a path"))?;

    let syn::PathArguments::AngleBracketed(args) = &last_segment.arguments else {
        return Ok(Vec::new());
    };

    Ok(args
        .args
        .iter()
        .filter_map(|arg| match arg {
            syn::GenericArgument::Type(ty) => Some(ty.clone()),
            _ => None,
        })
        .collect())
}

/// Derives the protocol storage schema type from a storage type argument.
fn schema_type_from_storage_type_arg(ty: &syn::Type) -> SchemaType {
    let syn::Type::Path(type_path) = ty else {
        return SchemaType::native_word();
    };

    let Some(last_segment) = type_path.path.segments.last() else {
        return SchemaType::native_word();
    };

    match last_segment.ident.to_string().as_str() {
        "Word" => SchemaType::native_word(),
        "Felt" => SchemaType::native_felt(),
        "u8" => SchemaType::u8(),
        "u16" => SchemaType::u16(),
        "u32" => SchemaType::u32(),
        _ => SchemaType::native_word(),
    }
}

/// Builds a simple word schema from a storage type argument.
fn word_schema_from_storage_type_arg(ty: &syn::Type) -> WordSchema {
    WordSchema::new_simple(schema_type_from_storage_type_arg(ty))
}

/// Builds protocol metadata for an account component during macro expansion.
pub struct AccountComponentMetadataBuilder {
    /// The human-readable name of the component.
    name: String,

    /// A brief description of what this component is and how it works.
    description: String,

    /// The version of the component using semantic versioning.
    version: Version,

    /// A set of supported target account types for this component.
    supported_types: BTreeSet<AccountType>,

    /// Storage schema entries defining the component's storage layout.
    storage: Vec<(StorageSlotName, StorageSlotSchema)>,
}

impl AccountComponentMetadataBuilder {
    /// Creates a new metadata builder.
    pub fn new(name: String, version: Version, description: String) -> Self {
        Self {
            name,
            description,
            version,
            supported_types: BTreeSet::new(),
            storage: Vec::new(),
        }
    }

    /// Adds a supported account type to this component metadata.
    pub fn add_supported_type(&mut self, account_type: AccountType) {
        self.supported_types.insert(account_type);
    }

    /// Adds a storage-schema entry derived from a component field.
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
                    .unwrap_or_else(|| WordSchema::new_simple(SchemaType::native_word()));
                let value_schema = args
                    .get(1)
                    .map(word_schema_from_storage_type_arg)
                    .unwrap_or_else(|| WordSchema::new_simple(SchemaType::native_word()));
                let slot_schema = StorageSlotSchema::Map(MapSlotSchema::new(
                    description,
                    None,
                    key_schema,
                    value_schema,
                ));
                self.storage.push((slot_name, slot_schema));
            }
            StorageFieldType::Storage => {
                let schema_type = if let Some(field_type) = field_type_attr.as_deref() {
                    SchemaType::new(field_type).map_err(|err| {
                        syn::Error::new(
                            field.span(),
                            format!("invalid storage field type attribute `{field_type}`: {err}"),
                        )
                    })?
                } else {
                    let args = extract_storage_type_args(field)?;
                    args.first()
                        .map(schema_type_from_storage_type_arg)
                        .unwrap_or_else(SchemaType::native_word)
                };

                let word_schema = WordSchema::new_simple(schema_type);
                let slot_schema =
                    StorageSlotSchema::Value(ValueSlotSchema::new(description, word_schema));
                self.storage.push((slot_name, slot_schema));
            }
        }

        Ok(())
    }

    /// Builds the final [`AccountComponentMetadata`].
    pub fn build(self, span: Span) -> Result<AccountComponentMetadata, syn::Error> {
        let storage_schema = StorageSchema::new(self.storage).map_err(|err| {
            syn::Error::new(span, format!("failed to build component storage schema: {err}"))
        })?;

        Ok(AccountComponentMetadata::new(self.name, self.supported_types)
            .with_description(self.description)
            .with_version(self.version)
            .with_storage_schema(storage_schema))
    }
}
