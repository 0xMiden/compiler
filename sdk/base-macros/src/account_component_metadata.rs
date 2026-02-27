use std::collections::BTreeSet;

use miden_protocol::account::{
    AccountType, StorageSlotName,
    component::{
        AccountComponentMetadata, MapSlotSchema, StorageSchema, StorageSlotSchema, ValueSlotSchema,
        WordSchema, storage::SchemaTypeId,
    },
};
use proc_macro2::Span;
use semver::Version;
use syn::spanned::Spanned;

use crate::{component_macro::typecheck_storage_field, types::StorageFieldType};

/// Extracts the type arguments for a storage field of the form `Storage<T>` or `StorageMap<K, V>`.
///
/// Proc macros cannot perform type resolution; this helper only inspects the syntactic type path
/// written in the component's struct field.
fn extract_storage_type_args(field: &syn::Field) -> Result<Vec<syn::Type>, syn::Error> {
    let type_path = match &field.ty {
        syn::Type::Path(type_path) => type_path,
        _ => {
            return Err(syn::Error::new(field.span(), "storage field type must be a path"));
        }
    };

    let last_segment = type_path
        .path
        .segments
        .last()
        .ok_or_else(|| syn::Error::new(field.span(), "storage field type must be a path"))?;

    let syn::PathArguments::AngleBracketed(args) = &last_segment.arguments else {
        return Ok(Vec::new());
    };

    let mut out = Vec::new();
    for arg in args.args.iter() {
        if let syn::GenericArgument::Type(ty) = arg {
            out.push(ty.clone());
        }
    }

    Ok(out)
}

/// Derives a [`SchemaTypeId`] from a storage field's type argument.
///
/// Storage items and map keys/values are stored as a single protocol `Word`. The schema type is
/// used for init-time parsing/validation and for downstream introspection. When the type argument
/// corresponds to a known protocol schema type (e.g. `Felt`), we return the matching identifier.
/// Otherwise, we conservatively fall back to `word`.
fn schema_type_id_from_storage_type_arg(ty: &syn::Type) -> SchemaTypeId {
    let syn::Type::Path(type_path) = ty else {
        return SchemaTypeId::native_word();
    };

    let Some(last_segment) = type_path.path.segments.last() else {
        return SchemaTypeId::native_word();
    };

    match last_segment.ident.to_string().as_str() {
        "Word" => SchemaTypeId::native_word(),
        "Felt" => SchemaTypeId::native_felt(),
        "u8" => SchemaTypeId::u8(),
        "u16" => SchemaTypeId::u16(),
        "u32" => SchemaTypeId::u32(),
        _ => SchemaTypeId::native_word(),
    }
}

/// Builds a simple [`WordSchema`] for a storage field's type argument.
fn word_schema_from_storage_type_arg(ty: &syn::Type) -> WordSchema {
    WordSchema::new_simple(schema_type_id_from_storage_type_arg(ty))
}

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

    /// Creates a new [`AccountComponentMetadataBuilder`].
    pub fn new(name: String, version: Version, description: String) -> Self {
        AccountComponentMetadataBuilder {
            name,
            description,
            version,
            supported_types: BTreeSet::new(),
            storage: Vec::new(),
        }
    }

    /// Adds a storage slot schema entry for `field`.
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
