use std::collections::HashMap;

use quote::quote;
use syn::{Field, Type, ext::IdentExt, spanned::Spanned};

use crate::{
    account_component_metadata::AccountComponentMetadataBuilder, component_macro::stored_procedure,
    namespace::ComponentNamespace, types::StorageFieldType,
};

/// Rust crate the supported storage field types are expected to come from.
const BASE_CRATE: &str = "miden";
/// Rust type name of a storage map slot.
const TYPENAME_MAP: &str = "StorageMap";
/// Rust type name of a storage value slot.
const TYPENAME_VALUE: &str = "StorageValue";

/// Returns the name of the storage field `field` as written, without the `r#` of a raw
/// identifier: the one spelling its storage slot name and, for a `StoredProcedure` slot, the
/// `@external-id` of its dispatch import are built from.
///
/// Returns an error at the field when the name starts with `_`, which `StorageSlotName` rejects
/// in a path segment.
pub(super) fn storage_field_name(field: &syn::Ident) -> syn::Result<String> {
    let name = field.unraw().to_string();
    if name.starts_with('_') {
        return Err(syn::Error::new(
            field.span(),
            format!(
                "storage field `{field}` starts with `_`, but its name becomes a segment of the \
                 storage slot name, and slot name segments cannot start with an underscore; \
                 rename the field"
            ),
        ));
    }
    Ok(name)
}

/// Derives the full storage slot name for a component field.
///
/// Slot names are part of the on-chain storage ABI: they are `<namespace>::<field_name>`, where
/// the namespace is the component's `[lib].namespace`, so private Rust renames of the storage
/// struct cannot change deployed slot names.
fn derive_storage_slot_name(
    namespace: &ComponentNamespace,
    field: &syn::Ident,
) -> syn::Result<String> {
    Ok(namespace.storage_slot_name(&storage_field_name(field)?))
}

/// Parsed arguments collected from a `#[storage(...)]` attribute.
struct StorageAttributeArgs {
    description: Option<String>,
    type_attr: Option<String>,
}

/// Attempts to parse a `#[storage]` / `#[storage(...)]` attribute and returns its arguments.
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
        // Bare `#[storage]` is the shorthand for a slot that carries no optional arguments.
        syn::Meta::Path(_) => {
            return Ok(Some(StorageAttributeArgs {
                description: None,
                type_attr: None,
            }));
        }
        _ => {
            return Err(syn::Error::new(attr.span(), "Expected `#[storage]` or `#[storage(...)]`"));
        }
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
    let suffix = id.suffix().as_canonical_u64();
    let prefix = id.prefix().as_canonical_u64();
    quote! {
        ::miden::StorageSlotId::new(
            ::miden::Felt::new(#suffix).unwrap(),
            ::miden::Felt::new(#prefix).unwrap(),
        )
    }
}

/// Processes component struct fields, recording storage metadata and building default
/// initializers.
pub fn process_storage_fields(
    fields: &mut syn::FieldsNamed,
    builder: &mut AccountComponentMetadataBuilder,
    namespace: Option<&ComponentNamespace>,
) -> Result<Vec<proc_macro2::TokenStream>, syn::Error> {
    let mut field_infos = Vec::new();
    let mut errors = Vec::new();
    let mut slot_names = HashMap::<String, String>::new();
    let mut slot_ids = HashMap::<(u64, u64), String>::new();

    for field in fields.named.iter_mut() {
        let field_type = match typecheck_storage_field(field) {
            Ok(field_type) => field_type,
            Err(err) => {
                errors.push(err);
                continue;
            }
        };
        if let Err(err) = reject_stored_procedure_in_map(field, &field_type) {
            errors.push(err);
            continue;
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
            if let Err(err) =
                reject_stored_procedure_type_override(field, args.type_attr.as_deref())
            {
                errors.push(err);
                continue;
            }
            // Without a project manifest there is no namespace; the caller reports that once
            // field validation is done.
            let Some(namespace) = namespace else {
                continue;
            };
            // `StorageSlotId` values are derived from slot names, so keep this format stable.
            let slot_name_str = match derive_storage_slot_name(namespace, field_name) {
                Ok(slot_name) => slot_name,
                Err(err) => {
                    errors.push(err);
                    continue;
                }
            };
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
            let slot_id_key =
                (slot_id.suffix().as_canonical_u64(), slot_id.prefix().as_canonical_u64());
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

            if let Err(err) = builder.add_storage_entry(
                slot_name.clone(),
                args.description,
                field,
                args.type_attr,
            ) {
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

/// Checks that the type of `field` is either `StorageMap` or `StorageValue` from the `miden`
/// crate.
///
/// # Limitations
///
/// Types are not resolved during macro expansion, so this check just verifies the identifier
/// written in the struct correspond to one of the expected values. Hence the following cannot
/// be detected:
///
/// * A developer defines their own `StorageMap` or `StorageValue`
/// * A developer uses a valid type from miden but aliases it
pub(crate) fn typecheck_storage_field(field: &Field) -> Result<StorageFieldType, syn::Error> {
    if !matches!(&field.ty, Type::Path(_)) {
        return Err(syn::Error::new(field.span(), "storage field type must be a path"));
    }

    storage_field_type(&field.ty).ok_or_else(|| {
        syn::Error::new(
            field.span(),
            format!(
                "storage field type can only be `{TYPENAME_MAP}` or `{TYPENAME_VALUE}` from \
                 `{BASE_CRATE}` crate"
            ),
        )
    })
}

/// Classifies the written spelling of a storage field type, or `None` when it is neither of the
/// supported types.
///
/// This is the single spelling rule for storage slots — `StorageMap`/`StorageValue`, optionally
/// qualified with `miden::` — shared by [`typecheck_storage_field`] and the stored-procedure
/// rewrite, so both agree on which fields are storage slots.
pub(crate) fn storage_field_type(ty: &Type) -> Option<StorageFieldType> {
    let Type::Path(type_path) = ty else {
        return None;
    };

    let segments: Vec<String> = type_path
        .path
        .segments
        .iter()
        .map(|segment| segment.ident.to_string())
        .collect();

    match segments.as_slice() {
        [a] if a == TYPENAME_MAP => Some(StorageFieldType::StorageMap),
        [a] if a == TYPENAME_VALUE => Some(StorageFieldType::StorageValue),
        [a, b] if a == BASE_CRATE && b == TYPENAME_MAP => Some(StorageFieldType::StorageMap),
        [a, b] if a == BASE_CRATE && b == TYPENAME_VALUE => Some(StorageFieldType::StorageValue),
        _ => None,
    }
}

/// Rejects a `StoredProcedure` used as the key or value type of a `StorageMap`.
///
/// A stored procedure root is bound to the one slot whose signature the macro generated, so it
/// cannot be a map entry; without this check the user would face a sealed-trait error pointing at
/// the SDK instead.
fn reject_stored_procedure_in_map(
    field: &Field,
    field_type: &StorageFieldType,
) -> Result<(), syn::Error> {
    if !matches!(field_type, StorageFieldType::StorageMap) {
        return Ok(());
    }
    if !stored_procedure::mentions_stored_procedure(&field.ty) {
        return Ok(());
    }

    Err(syn::Error::new(
        field.ty.span(),
        "`StoredProcedure` is only supported in `StorageValue` slots",
    ))
}

/// Rejects a `#[storage(type = "...")]` override on a stored-procedure slot.
///
/// The generated call always reads a four-felt procedure root out of the slot, so an override
/// would only make the schema the component advertises disagree with what the code does.
fn reject_stored_procedure_type_override(
    field: &Field,
    type_attr: Option<&str>,
) -> Result<(), syn::Error> {
    if type_attr.is_none() || !stored_procedure::mentions_stored_procedure(&field.ty) {
        return Ok(());
    }

    Err(syn::Error::new(
        field.span(),
        "the schema type of a `StoredProcedure` slot is fixed (a word holding the procedure \
         root); remove the `type` argument from its `#[storage(...)]` attribute",
    ))
}

#[cfg(test)]
mod tests {
    use proc_macro2::Span;
    use quote::quote;
    use syn::{parse::Parser, parse_quote};

    use super::{
        StorageFieldType, derive_storage_slot_name, reject_stored_procedure_in_map,
        reject_stored_procedure_type_override, typecheck_storage_field,
    };
    use crate::namespace::ComponentNamespace;

    fn counter_namespace() -> ComponentNamespace {
        ComponentNamespace::parse("miden::counter_contract::counter_contract", Span::call_site())
            .unwrap()
    }

    /// Pins the map-slot diagnostic: a stored root is bound to the one slot whose signature the
    /// macro generated, so it cannot be a map key or value.
    #[test]
    fn rejects_stored_procedures_in_map_slots() {
        let field = syn::Field::parse_named
            .parse2(quote!(hooks: StorageMap<Felt, StoredProcedure<fn()>>))
            .expect("test field must parse");
        let field_type = typecheck_storage_field(&field).expect("test field type must be valid");
        let err = reject_stored_procedure_in_map(&field, &field_type).unwrap_err();
        assert!(err.to_string().contains("only supported in `StorageValue` slots"), "{err}");

        let field = syn::Field::parse_named
            .parse2(quote!(count_map: StorageMap<Word, Felt>))
            .expect("test field must parse");
        reject_stored_procedure_in_map(&field, &StorageFieldType::StorageMap)
            .expect("plain map slots are accepted");
    }

    /// Pins the type-override diagnostic: the metadata would honour the override while the
    /// generated call keeps reading a procedure root, so the advertised schema would lie.
    #[test]
    fn rejects_a_type_override_on_a_stored_procedure_slot() {
        let field = syn::Field::parse_named
            .parse2(quote!(authority: StorageValue<StoredProcedure<fn()>>))
            .expect("test field must parse");
        let err = reject_stored_procedure_type_override(&field, Some("u32")).unwrap_err();
        assert!(
            err.to_string().contains("is fixed (a word holding the procedure root)"),
            "{err}"
        );

        reject_stored_procedure_type_override(&field, None)
            .expect("a stored-procedure slot without an override is accepted");

        let field = syn::Field::parse_named
            .parse2(quote!(count: StorageValue<Felt>))
            .expect("test field must parse");
        reject_stored_procedure_type_override(&field, Some("u32"))
            .expect("ordinary value slots keep their type override");
    }

    #[test]
    fn derives_slot_name_from_namespace_and_field() {
        assert_eq!(
            derive_storage_slot_name(&counter_namespace(), &parse_quote!(count_map)).unwrap(),
            "miden::counter_contract::counter_contract::count_map"
        );
    }

    /// The field name is used as written: a raw identifier loses only its `r#`, and other
    /// spellings are not normalized.
    #[test]
    fn slot_names_use_the_field_name_as_written() {
        assert_eq!(
            derive_storage_slot_name(&counter_namespace(), &parse_quote!(r#type)).unwrap(),
            "miden::counter_contract::counter_contract::type"
        );
        assert_eq!(
            derive_storage_slot_name(&counter_namespace(), &parse_quote!(hookA)).unwrap(),
            "miden::counter_contract::counter_contract::hookA"
        );
    }

    #[test]
    fn rejects_leading_underscore_field_names() {
        let err = derive_storage_slot_name(&counter_namespace(), &parse_quote!(_count_map))
            .unwrap_err()
            .to_string();
        assert!(err.contains("storage field `_count_map` starts with `_`"), "{err}");
        assert!(err.contains("cannot start with an underscore"), "{err}");
    }
}
