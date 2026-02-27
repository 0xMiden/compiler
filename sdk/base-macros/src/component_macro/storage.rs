use std::collections::HashMap;

use quote::quote;
use syn::{Field, Type, spanned::Spanned};

use crate::{account_component_metadata::AccountComponentMetadataBuilder, types::StorageFieldType};

/// Normalizes a storage slot name component into a valid identifier-like segment.
///
/// This is a lossy transformation: characters outside `[A-Za-z0-9_]` are replaced with `_`, and
/// empty/leading-underscore components are prefixed to avoid invalid identifiers. Callers should
/// ensure the resulting slot names remain unique.
fn sanitize_slot_name_component(component: &str) -> String {
    let mut out: String = component
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();

    if out.is_empty() {
        out.push('x');
    }
    if out.starts_with('_') {
        out.insert(0, 'x');
    }

    out
}

/// Parsed arguments collected from a `#[storage(...)]` attribute.
struct StorageAttributeArgs {
    description: Option<String>,
    type_attr: Option<String>,
}

/// Attempts to parse a `#[storage(...)]` attribute and returns the extracted arguments.
fn parse_storage_attribute(
    attr: &syn::Attribute,
) -> Result<Option<StorageAttributeArgs>, syn::Error> {
    if !attr.path().is_ident("storage") {
        return Ok(None);
    }

    let mut description_value = None;
    let mut type_value = None;

    let list = match &attr.meta {
        syn::Meta::List(list) => list,
        _ => return Err(syn::Error::new(attr.span(), "Expected #[storage(...)]")),
    };

    let parser = syn::meta::parser(|meta| {
        if meta.path.is_ident("slot") {
            Err(meta.error("`slot(...)` is no longer supported; slots are derived from slot names"))
        } else if meta.path.is_ident("description") {
            let value = meta.value()?;
            let lit: syn::LitStr = value.parse()?;
            description_value = Some(lit.value());
            Ok(())
        } else if meta.path.is_ident("type") {
            let value = meta.value()?;
            let lit: syn::LitStr = value.parse()?;
            type_value = Some(lit.value());
            Ok(())
        } else {
            Err(meta.error("unrecognized storage attribute argument"))
        }
    });

    list.parse_args_with(parser)?;

    Ok(Some(StorageAttributeArgs {
        description: description_value,
        type_attr: type_value,
    }))
}

/// Converts a [`miden_protocol::account::StorageSlotId`] into tokens that reconstruct it as a
/// constant expression.
fn slot_id_tokens(id: miden_protocol::account::StorageSlotId) -> proc_macro2::TokenStream {
    let suffix = id.suffix().as_int();
    let prefix = id.prefix().as_int();
    quote! {
        ::miden::StorageSlotId::new(
            ::miden::Felt::from_u64_unchecked(#suffix),
            ::miden::Felt::from_u64_unchecked(#prefix),
        )
    }
}

/// Processes component struct fields, recording storage metadata and building default
/// initializers.
pub fn process_storage_fields(
    fields: &mut syn::FieldsNamed,
    builder: &mut AccountComponentMetadataBuilder,
    storage_namespace: &str,
) -> Result<Vec<proc_macro2::TokenStream>, syn::Error> {
    let mut field_infos = Vec::new();
    let mut errors = Vec::new();
    let mut slot_names = HashMap::<String, String>::new();
    let mut slot_ids = HashMap::<(u64, u64), String>::new();

    for field in fields.named.iter_mut() {
        if let Err(err) = typecheck_storage_field(field) {
            errors.push(err);
        }
        let field_name = field.ident.as_ref().expect("Named field must have an identifier");
        let field_name_str = field_name.to_string();
        let mut storage_args = None;
        let mut attr_indices_to_remove = Vec::new();

        for (attr_idx, attr) in field.attrs.iter().enumerate() {
            match parse_storage_attribute(attr) {
                Ok(Some(args)) => {
                    if storage_args.is_some() {
                        errors.push(syn::Error::new(attr.span(), "duplicate `storage` attribute"));
                    }
                    storage_args = Some(args);
                    attr_indices_to_remove.push(attr_idx);
                }
                Ok(None) => {}
                Err(e) => errors.push(e),
            }
        }

        for (removed_count, idx_to_remove) in attr_indices_to_remove.into_iter().enumerate() {
            field.attrs.remove(idx_to_remove - removed_count);
        }

        if let Some(args) = storage_args {
            // Slot names are part of the on-chain storage ABI: `StorageSlotId` values are derived
            // from the slot name. Keep this format stable.
            let namespace = sanitize_slot_name_component(storage_namespace);
            let field_component = sanitize_slot_name_component(&field_name_str);
            let slot_name_str = format!("miden::component::{namespace}::{field_component}");
            if let Some(existing_field) = slot_names.get(&slot_name_str) {
                errors.push(syn::Error::new(
                    field.span(),
                    format!(
                        "storage slot name '{slot_name_str}' for field '{field_name_str}' \
                         conflicts with field '{existing_field}'"
                    ),
                ));
                continue;
            }

            let slot_name = miden_protocol::account::StorageSlotName::new(slot_name_str.clone())
                .map_err(|err| {
                    syn::Error::new(
                        field.span(),
                        format!("failed to construct storage slot name: {err}"),
                    )
                })?;
            let slot_id = slot_name.id();
            let slot_id_key = (slot_id.suffix().as_int(), slot_id.prefix().as_int());
            if let Some(existing_field) = slot_ids.get(&slot_id_key) {
                errors.push(syn::Error::new(
                    field.span(),
                    format!(
                        "storage slot id for field '{field_name_str}' conflicts with field \
                         '{existing_field}'"
                    ),
                ));
                continue;
            }
            slot_names.insert(slot_name_str, field_name_str.clone());
            slot_ids.insert(slot_id_key, field_name_str);

            if let Err(err) =
                builder.add_storage_entry(slot_name.clone(), args.description, field, args.type_attr)
            {
                errors.push(err);
            }

            field_infos.push((field_name.clone(), slot_id));
        } else {
            errors
                .push(syn::Error::new(field.span(), "field is missing the `#[storage]` attribute"));
        }
    }

    if let Some(first_error) = errors.into_iter().next() {
        return Err(first_error);
    }

    let mut field_inits = Vec::with_capacity(field_infos.len());
    for (field_name, slot_id) in field_infos.into_iter() {
        let slot = slot_id_tokens(slot_id);
        field_inits.push(quote! {
            #field_name: ::core::convert::From::from(#slot)
        });
    }

    Ok(field_inits)
}

/// Checks that the type of `field` is either `StorageMap` or `Storage` from the `miden` crate.
///
/// # Limitations
///
/// Types are not resolved during macro expansion, so this check just verifies the identifier
/// written in the struct correspond to one of the expected values. Hence the following cannot
/// be detected:
///
/// * A developer defines their own `StorageMap` or `Storage`
/// * A developer uses a valid type from miden but aliases it
pub(crate) fn typecheck_storage_field(field: &Field) -> Result<StorageFieldType, syn::Error> {
    let type_path = match &field.ty {
        Type::Path(type_path) => type_path,
        _ => {
            return Err(syn::Error::new(field.span(), "storage field type must be a path"));
        }
    };

    let segments: Vec<String> = type_path
        .path
        .segments
        .iter()
        .map(|segment| segment.ident.to_string())
        .collect();

    const BASE_CRATE: &str = "miden";
    const TYPENAME_MAP: &str = "StorageMap";
    const TYPENAME_STORAGE: &str = "Storage";

    match segments.as_slice() {
        [a] if a == TYPENAME_MAP => Ok(StorageFieldType::StorageMap),
        [a] if a == TYPENAME_STORAGE => Ok(StorageFieldType::Storage),
        [a, b] if a == BASE_CRATE && b == TYPENAME_MAP => Ok(StorageFieldType::StorageMap),
        [a, b] if a == BASE_CRATE && b == TYPENAME_STORAGE => Ok(StorageFieldType::Storage),
        _ => Err(syn::Error::new(
            field.span(),
            format!(
                "storage field type can only be `{TYPENAME_MAP}` or `{TYPENAME_STORAGE}` from \
                 `{BASE_CRATE}` crate"
            ),
        )),
    }
}
