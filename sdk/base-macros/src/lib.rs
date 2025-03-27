use account_component_metadata::AccountComponentMetadataBuilder;
use miden_objects::utils::Serializable;
use proc_macro2::Literal;

extern crate proc_macro;

mod account_component_metadata;

/// Account component
#[proc_macro_attribute]
pub fn component(
    _attr: proc_macro::TokenStream,
    item: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    use quote::quote;
    use syn::parse_macro_input;

    // Parse the input as an item struct
    let mut input = parse_macro_input!(item as syn::ItemStruct);
    let struct_name = &input.ident;

    // Create a vector to hold field initializations
    let mut field_inits = Vec::new();

    let mut acc_builder = AccountComponentMetadataBuilder::new(struct_name.to_string());

    // Process each field in the struct to extract storage slot info
    if let syn::Fields::Named(ref mut named_fields) = input.fields {
        for field in &mut named_fields.named {
            let field_name = &field.ident;
            let field_type = &field.ty;

            let mut attr_indices_to_remove = Vec::new();

            // Look for the storage attribute
            for (attr_idx, attr) in field.attrs.iter().enumerate() {
                if attr.path().is_ident("storage") {
                    if let syn::Meta::List(meta_list) = &attr.meta {
                        let mut slot_value = None;
                        let mut description = None;
                        let mut type_value = None;

                        // Parse token stream to find slot(N) and description = "..."
                        let tokens = meta_list.tokens.clone();
                        let tokens_str = tokens.to_string();

                        // Look for slot(N) pattern
                        if let Some(slot_idx) = tokens_str.find("slot") {
                            let after_slot = &tokens_str[slot_idx..];
                            if let Some(open_paren) = after_slot.find('(') {
                                if let Some(close_paren) = after_slot[open_paren..].find(')') {
                                    let slot_str =
                                        &after_slot[open_paren + 1..open_paren + close_paren];
                                    if let Ok(slot) = slot_str.trim().parse::<u8>() {
                                        slot_value = Some(slot);
                                    }
                                }
                            }
                        }

                        // Look for description = "..." pattern
                        if let Some(desc_idx) = tokens_str.find("description") {
                            let after_desc = &tokens_str[desc_idx..];
                            // Find the equals sign after "description"
                            if let Some(equals_idx) = after_desc.find('=') {
                                let after_equals = &after_desc[equals_idx + 1..];
                                let trimmed = after_equals.trim();
                                // Look for opening quote
                                if trimmed.starts_with('"') {
                                    if let Some(closing_quote_idx) = trimmed[1..].find('"') {
                                        let desc_value = &trimmed[1..closing_quote_idx + 1];
                                        description = Some(desc_value.to_string());
                                    }
                                }
                            }
                        }

                        // Look for type = "..." pattern
                        if let Some(type_idx) = tokens_str.find("type") {
                            let after_type = &tokens_str[type_idx..];
                            // Find equals sign
                            if let Some(equals_idx) = after_type.find('=') {
                                let after_equals = &after_type[equals_idx + 1..];
                                let trimmed = after_equals.trim();
                                // Look for opening quote
                                if trimmed.starts_with('"') {
                                    if let Some(closing_quote_idx) = trimmed[1..].find('"') {
                                        let type_val = &trimmed[1..closing_quote_idx + 1];
                                        type_value = Some(type_val.to_string());
                                    }
                                }
                            }
                        }

                        // If we found a slot value, create the field initialization
                        if let Some(slot) = slot_value {
                            field_inits.push(quote! {
                                #field_name: #field_type { slot: #slot }
                            });

                            // Extract the field name as a string
                            let field_name_str = field_name.clone().unwrap().to_string();

                            // Add a storage entry to the component metadata
                            acc_builder.add_storage_entry(
                                &field_name_str,
                                description,
                                slot,
                                field_type,
                                type_value,
                            );
                        }
                    }
                    attr_indices_to_remove.push(attr_idx);
                }
            }
            for idx_to_remove in attr_indices_to_remove {
                field.attrs.remove(idx_to_remove);
            }
        }
    }

    // Generate the constant instance declaration
    let instance = quote! {
        #[allow(non_upper_case_globals)]
        const #struct_name: #struct_name = #struct_name {
            #(#field_inits),*
        };
    };

    let acc_component_metadata_bytes = acc_builder.build().to_bytes();
    let link_section_bytes_len = acc_component_metadata_bytes.len();
    let encoded_bytes_str = Literal::byte_string(&acc_component_metadata_bytes);

    let acc_component_metadata_link_section = quote! {
        #[unsafe(
            // to test it in the integration tests the section name needs to make mach-o section
            // specifier happy and to have "segment and section separated by comma"
            link_section = "rodata,miden_account"
        )]
        #[doc(hidden)]
        #[allow(clippy::octal_escapes)]
        pub static __MIDEN_ACCOUNT_COMPONENT_METADATA_BYTES: [u8; #link_section_bytes_len] = *#encoded_bytes_str;
    };

    // Combine the original struct with the generated instance and serialized account component metadata
    let output = quote! {
        #input

        #instance

        #acc_component_metadata_link_section
    };

    proc_macro::TokenStream::from(output)
}
