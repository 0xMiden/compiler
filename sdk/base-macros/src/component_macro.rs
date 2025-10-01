use std::{fs, path::Path, str::FromStr};

use miden_objects::{account::AccountType, utils::Serializable};
use proc_macro::Span;
use proc_macro2::Literal;
use quote::quote;
use semver::Version;
use syn::{parse_macro_input, spanned::Spanned};
use toml::Value;

use crate::account_component_metadata::AccountComponentMetadataBuilder;

/// Cargo metadata relevant for the `#[component]` macro expansion.
struct CargoMetadata {
    name: String,
    version: Version,
    description: String,
    supported_types: Vec<String>,
}

/// Parsed arguments collected from a `#[storage(...)]` attribute.
struct StorageAttributeArgs {
    slot: u8,
    description: Option<String>,
    type_attr: Option<String>,
}

/// Expands the `#[component]` attribute by wiring storage metadata and link section exports.
pub fn component(
    attr: proc_macro::TokenStream,
    item: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    if !attr.is_empty() {
        return syn::Error::new(Span::call_site().into(), "#[component] does not accept arguments")
            .into_compile_error()
            .into();
    }

    let call_site_span = Span::call_site();
    let mut input_struct = parse_macro_input!(item as syn::ItemStruct);
    let struct_name = &input_struct.ident;

    let metadata = match get_package_metadata(call_site_span) {
        Ok(m) => m,
        Err(e) => return e.to_compile_error().into(),
    };

    let mut acc_builder =
        AccountComponentMetadataBuilder::new(metadata.name, metadata.version, metadata.description);

    for st in &metadata.supported_types {
        match AccountType::from_str(st) {
            Ok(at) => acc_builder.add_supported_type(at),
            Err(err) => {
                return syn::Error::new(
                    call_site_span.into(),
                    format!("Invalid account type '{st}' in supported-types: {err}"),
                )
                .to_compile_error()
                .into()
            }
        }
    }

    let default_impl = match &mut input_struct.fields {
        syn::Fields::Named(fields) => {
            let field_inits = match process_storage_fields(fields, &mut acc_builder) {
                Ok(inits) => inits,
                Err(e) => return e.to_compile_error().into(),
            };
            generate_default_impl(struct_name, &field_inits)
        }
        syn::Fields::Unit => quote! {
            impl Default for #struct_name {
                fn default() -> Self {
                    Self
                }
            }
        },
        _ => {
            return syn::Error::new(
                input_struct.fields.span(),
                "The `component` macro only supports unit structs or structs with named fields.",
            )
            .to_compile_error()
            .into();
        }
    };

    let metadata = acc_builder.build();

    let mut metadata_bytes = metadata.to_bytes();
    let padded_len = metadata_bytes.len().div_ceil(16) * 16;
    metadata_bytes.resize(padded_len, 0);

    let link_section = generate_link_section(&metadata_bytes);

    quote! {
        miden::generate!();
        #input_struct
        #default_impl
        #link_section
        bindings::export!(#struct_name);
    }
    .into()
}

/// Reads component metadata (name/description/version/supported types) from the enclosing package
/// manifest.
fn get_package_metadata(call_site_span: Span) -> Result<CargoMetadata, syn::Error> {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".to_string());
    let current_dir = Path::new(&manifest_dir);

    let cargo_toml_path = current_dir.join("Cargo.toml");
    if !cargo_toml_path.is_file() {
        return Ok(CargoMetadata {
            name: String::new(),
            version: Version::new(0, 0, 1),
            description: String::new(),
            supported_types: vec![],
        });
    }

    let cargo_toml_content = fs::read_to_string(&cargo_toml_path).map_err(|e| {
        syn::Error::new(
            call_site_span.into(),
            format!("Failed to read {}: {}", cargo_toml_path.display(), e),
        )
    })?;
    let cargo_toml: Value = cargo_toml_content.parse::<Value>().map_err(|e| {
        syn::Error::new(
            call_site_span.into(),
            format!("Failed to parse {}: {}", cargo_toml_path.display(), e),
        )
    })?;

    let package_table = cargo_toml.get("package").ok_or_else(|| {
        syn::Error::new(
            call_site_span.into(),
            format!(
                "Cargo.toml ({}) does not contain a [package] table",
                cargo_toml_path.display()
            ),
        )
    })?;

    let name = package_table
        .get("name")
        .and_then(|n| n.as_str())
        .map(String::from)
        .ok_or_else(|| {
            syn::Error::new(
                call_site_span.into(),
                format!("Missing 'name' field in [package] table of {}", cargo_toml_path.display()),
            )
        })?;

    let version_str = package_table
        .get("version")
        .and_then(|v| v.as_str())
        .or_else(|| {
            let base = env!("CARGO_MANIFEST_DIR");
            if base.ends_with(cargo_toml_path.parent().unwrap().to_str().unwrap()) {
                Some("0.0.0")
            } else {
                None
            }
        })
        .ok_or_else(|| {
            syn::Error::new(
                call_site_span.into(),
                format!(
                    "Missing 'version' field in [package] table of {} (version.workspace = true \
                     is not yet supported for external crates)",
                    cargo_toml_path.display()
                ),
            )
        })?;

    let version = Version::parse(version_str).map_err(|e| {
        syn::Error::new(
            call_site_span.into(),
            format!(
                "Failed to parse version '{}' from {}: {}",
                version_str,
                cargo_toml_path.display(),
                e
            ),
        )
    })?;

    let description = package_table
        .get("description")
        .and_then(|d| d.as_str())
        .map(String::from)
        .unwrap_or_default();

    let supported_types = cargo_toml
        .get("package")
        .and_then(|pkg| pkg.get("metadata"))
        .and_then(|m| m.get("miden"))
        .and_then(|m| m.get("supported-types"))
        .and_then(|st| st.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect::<Vec<String>>()
        })
        .unwrap_or_default();

    Ok(CargoMetadata {
        name,
        version,
        description,
        supported_types,
    })
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
fn process_storage_fields(
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

/// Synthesizes the `Default` implementation for the component struct using the collected storage
/// initializers.
fn generate_default_impl(
    struct_name: &syn::Ident,
    field_inits: &[proc_macro2::TokenStream],
) -> proc_macro2::TokenStream {
    quote! {
        impl Default for #struct_name {
            fn default() -> Self {
                Self {
                    #(#field_inits),*
                }
            }
        }
    }
}

/// Emits the static metadata blob inside the `rodata,miden_account` link section.
fn generate_link_section(metadata_bytes: &[u8]) -> proc_macro2::TokenStream {
    let link_section_bytes_len = metadata_bytes.len();
    let encoded_bytes_str = Literal::byte_string(metadata_bytes);

    quote! {
        #[unsafe(
            // to test it in the integration(this crate) tests the section name needs to make mach-o section
            // specifier happy and to have "segment and section separated by comma"
            link_section = "rodata,miden_account"
        )]
        #[doc(hidden)]
        #[allow(clippy::octal_escapes)]
        pub static __MIDEN_ACCOUNT_COMPONENT_METADATA_BYTES: [u8; #link_section_bytes_len] = *#encoded_bytes_str;
    }
}
