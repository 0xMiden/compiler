use quote::quote;
use syn::spanned::Spanned;

use crate::account_component_metadata::AccountComponentMetadataBuilder;

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
    slot: u8,
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

    let mut slot_value = None;
    let mut description_value = None;
    let mut type_value = None;

    let list = match &attr.meta {
        syn::Meta::List(list) => list,
        _ => return Err(syn::Error::new(attr.span(), "Expected #[storage(...)]")),
    };

    let parser = syn::meta::parser(|meta| {
        if meta.path.is_ident("slot") {
            let value_stream;
            syn::parenthesized!(value_stream in meta.input);
            let lit: syn::LitInt = value_stream.parse()?;
            slot_value = Some(lit.base10_parse::<u8>()?);
            Ok(())
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

    let slot = slot_value.ok_or_else(|| {
        syn::Error::new(attr.span(), "missing required `slot(N)` argument in `storage` attribute")
    })?;

    Ok(Some(StorageAttributeArgs {
        slot,
        description: description_value,
        type_attr: type_value,
    }))
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

    for field in fields.named.iter_mut() {
        let field_name = field.ident.as_ref().expect("Named field must have an identifier");
        let field_type = &field.ty;
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
            let namespace = sanitize_slot_name_component(storage_namespace);
            let field_component = sanitize_slot_name_component(&field_name.to_string());
            let slot_name_str = format!("miden::component::{namespace}::{field_component}");
            let slot_name =
                miden_protocol::account::StorageSlotName::new(slot_name_str).map_err(|err| {
                    syn::Error::new(
                        field.span(),
                        format!("failed to construct storage slot name: {err}"),
                    )
                })?;

            builder.add_storage_entry(
                slot_name.clone(),
                args.description,
                field_type,
                args.type_attr,
            );

            field_infos.push((field_name.clone(), field_type.clone(), slot_name));
        } else {
            errors
                .push(syn::Error::new(field.span(), "field is missing the `#[storage]` attribute"));
        }
    }

    if let Some(first_error) = errors.into_iter().next() {
        return Err(first_error);
    }

    // Compute slot indices based on the canonical storage slot ordering used by the protocol.
    //
    // Storage slots are ordered by their hashed [`StorageSlotId`]. We assign indices based on
    // that ordering so component code can reference slots deterministically at runtime.
    let mut ordered = field_infos
        .iter()
        .enumerate()
        .map(|(idx, (_, _, slot_name))| (idx, slot_name.clone()))
        .collect::<Vec<_>>();
    ordered.sort_by(|(_, a), (_, b)| a.cmp(b));

    let mut index_by_field = vec![0u8; field_infos.len()];
    for (slot_index, (field_idx, _)) in ordered.into_iter().enumerate() {
        let slot_index = u8::try_from(slot_index).map_err(|_| {
            syn::Error::new(fields.span(), "component storage schema contains more than 255 slots")
        })?;
        index_by_field[field_idx] = slot_index;
    }

    let mut field_inits = Vec::with_capacity(field_infos.len());
    for (idx, (field_name, field_type, _slot_name)) in field_infos.into_iter().enumerate() {
        let slot = index_by_field[idx];
        field_inits.push(quote! {
            #field_name: #field_type { slot: #slot }
        });
    }

    Ok(field_inits)
}
