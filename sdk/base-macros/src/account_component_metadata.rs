use std::sync::Arc;

use miden_protocol::account::{
    StorageSlotName,
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
        // TODO(i1352 phase 2): flip to miden::protocol::stored_procedure once the protocol
        // registers it.
        "ProcedureRoot" => SchemaType::native_word(),
        // TODO(i1352 phase 2): flip to miden::protocol::stored_procedure once the protocol
        // registers it.
        "StoredProcedure" => SchemaType::native_word(),
        "Felt" | "AssetAmount" => SchemaType::native_felt(),
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

/// Rejects storage type arguments that a storage map cannot hold.
fn reject_unsupported_map_type_arg(ty: &syn::Type) -> Result<(), syn::Error> {
    let syn::Type::Path(type_path) = ty else {
        return Ok(());
    };

    let Some(last_segment) = type_path.path.segments.last() else {
        return Ok(());
    };

    if last_segment.ident == "ProcedureRoot" {
        return Err(syn::Error::new(
            ty.span(),
            "`ProcedureRoot` is not supported in storage maps yet; use a `StorageValue` slot",
        ));
    }

    if last_segment.ident == "StoredProcedure" {
        return Err(syn::Error::new(
            ty.span(),
            "`StoredProcedure` is not supported in storage maps yet; use a `StorageValue` slot",
        ));
    }

    Ok(())
}

/// Builds protocol metadata for an account component during macro expansion.
pub struct AccountComponentMetadataBuilder {
    /// The human-readable name of the component.
    name: String,

    /// A brief description of what this component is and how it works.
    description: Arc<str>,

    /// The version of the component using semantic versioning.
    version: Version,

    /// Storage schema entries defining the component's storage layout.
    storage: Vec<(StorageSlotName, StorageSlotSchema)>,
}

impl AccountComponentMetadataBuilder {
    /// Creates a new metadata builder.
    pub fn new(name: String, version: Version, description: impl Into<Arc<str>>) -> Self {
        Self {
            name,
            description: description.into(),
            version,
            storage: Vec::new(),
        }
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
                for arg in &args {
                    reject_unsupported_map_type_arg(arg)?;
                }
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
            StorageFieldType::StorageValue => {
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

        Ok(AccountComponentMetadata::new(self.name)
            .with_description(self.description.as_ref())
            .with_version(self.version)
            .with_storage_schema(storage_schema))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Returns the first field of a struct written as `name: type`.
    fn field_from_declaration(declaration: proc_macro2::TokenStream) -> syn::Field {
        let item: syn::ItemStruct = syn::parse_quote! {
            struct Component { #declaration }
        };
        item.fields.into_iter().next().expect("struct must have one field")
    }

    fn builder() -> AccountComponentMetadataBuilder {
        AccountComponentMetadataBuilder::new(
            "component".to_string(),
            Version::new(1, 0, 0),
            "description",
        )
    }

    fn slot_name() -> StorageSlotName {
        StorageSlotName::new("component::component::slot".to_string()).expect("valid slot name")
    }

    #[test]
    fn procedure_root_maps_to_the_word_schema_type() {
        let ty: syn::Type = syn::parse_quote!(ProcedureRoot);
        assert_eq!(schema_type_from_storage_type_arg(&ty), SchemaType::native_word());

        let ty: syn::Type = syn::parse_quote!(miden::ProcedureRoot);
        assert_eq!(schema_type_from_storage_type_arg(&ty), SchemaType::native_word());
    }

    #[test]
    fn stored_procedure_maps_to_the_word_schema_type() {
        let ty: syn::Type = syn::parse_quote!(StoredProcedure<fn(Word, Felt) -> Felt>);
        assert_eq!(schema_type_from_storage_type_arg(&ty), SchemaType::native_word());

        let ty: syn::Type = syn::parse_quote!(miden::StoredProcedure<fn(Word)>);
        assert_eq!(schema_type_from_storage_type_arg(&ty), SchemaType::native_word());
    }

    #[test]
    fn stored_procedure_is_rejected_in_storage_maps() {
        let field = field_from_declaration(
            quote::quote!(slot: StorageMap<Word, StoredProcedure<fn(Word)>>),
        );
        let error = builder()
            .add_storage_entry(slot_name(), None, &field, None)
            .expect_err("`StoredProcedure` map values must be rejected");
        assert_eq!(
            error.to_string(),
            "`StoredProcedure` is not supported in storage maps yet; use a `StorageValue` slot"
        );

        let field = field_from_declaration(
            quote::quote!(slot: StorageMap<StoredProcedure<fn(Word)>, Word>),
        );
        let error = builder()
            .add_storage_entry(slot_name(), None, &field, None)
            .expect_err("`StoredProcedure` map keys must be rejected");
        assert_eq!(
            error.to_string(),
            "`StoredProcedure` is not supported in storage maps yet; use a `StorageValue` slot"
        );
    }

    #[test]
    fn procedure_root_is_rejected_in_storage_maps() {
        let field = field_from_declaration(quote::quote!(slot: StorageMap<Word, ProcedureRoot>));
        let error = builder()
            .add_storage_entry(slot_name(), None, &field, None)
            .expect_err("`ProcedureRoot` map values must be rejected");
        assert_eq!(
            error.to_string(),
            "`ProcedureRoot` is not supported in storage maps yet; use a `StorageValue` slot"
        );

        let field = field_from_declaration(quote::quote!(slot: StorageMap<ProcedureRoot, Word>));
        let error = builder()
            .add_storage_entry(slot_name(), None, &field, None)
            .expect_err("`ProcedureRoot` map keys must be rejected");
        assert_eq!(
            error.to_string(),
            "`ProcedureRoot` is not supported in storage maps yet; use a `StorageValue` slot"
        );
    }
}
