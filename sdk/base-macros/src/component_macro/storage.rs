use quote::quote;
use syn::spanned::Spanned;

use crate::account_component_metadata::AccountComponentMetadataBuilder;

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
) -> Result<Vec<proc_macro2::TokenStream>, syn::Error> {
    let mut field_inits = Vec::new();
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
            let slot = args.slot;
            field_inits.push(quote! {
                #field_name: #field_type { slot: #slot }
            });

            builder.add_storage_entry(
                &field_name.to_string(),
                args.description,
                args.slot,
                field_type,
                args.type_attr,
            );
        } else {
            errors
                .push(syn::Error::new(field.span(), "field is missing the `#[storage]` attribute"));
        }
    }

    if let Some(first_error) = errors.into_iter().next() {
        Err(first_error)
    } else {
        Ok(field_inits)
    }
}
