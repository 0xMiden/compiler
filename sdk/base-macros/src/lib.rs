extern crate proc_macro;

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

                        // Parse token stream to find slot(N)
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

                        // If we found a slot value, create the field initialization
                        if let Some(slot) = slot_value {
                            field_inits.push(quote! {
                                #field_name: #field_type { slot: #slot }
                            });
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

    // Combine the original struct with the generated instance
    let output = quote! {
        #input

        #instance
    };

    proc_macro::TokenStream::from(output)
}
