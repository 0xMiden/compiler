use super::*;

pub mod account;
pub mod active_note;
pub mod asset;
pub mod faucet;
pub mod input_note;
pub mod note;
pub mod output_note;
pub mod tx;

/// Builds the `[lib].namespace` for an account-component binding test.
///
/// The interface segment must match the component trait name (kebab-case), since the project
/// assembler ties the component's library identity to this namespace.
pub(crate) fn account_component_namespace(name: &str, interface: &str) -> String {
    let package = name.replace('_', "-");
    format!("miden:{package}/{interface}@0.0.1")
}

/// Splits a method definition into its trait signature declaration and its (non-`pub`) impl
/// method.
///
/// The snippet is parsed structurally with `syn`, so attributes, visibility, and braces inside
/// the signature or body are handled like Rust rather than by string surgery. Attributes (doc
/// comments, `#[auth_script]`, ...) move to the trait declaration, where the component macros
/// expect them.
pub(crate) fn split_method(method: &str) -> (String, String) {
    use quote::{ToTokens, quote};

    let mut method: syn::ImplItemFn = syn::parse_str(method)
        .expect("binding test fixture must be a valid Rust method definition");
    method.vis = syn::Visibility::Inherited;

    let attrs = core::mem::take(&mut method.attrs);
    let sig = &method.sig;
    let trait_signature = quote!(#(#attrs)* #sig;).to_string();
    let impl_method = method.to_token_stream().to_string();
    (trait_signature, impl_method)
}

/// Renders the three-part account-component source (`#[component_storage]` struct, `#[component]`
/// trait, and `#[component]` trait impl) for a binding test, deriving a unit storage struct named
/// `{trait_name}Storage` from the trait name.
pub(crate) fn account_component_source(trait_name: &str, method: &str) -> String {
    let storage_struct = format!("struct {trait_name}Storage;");
    account_component_source_with_storage(&storage_struct, trait_name, method)
}

/// Like [`account_component_source`], for tests that need `#[storage(...)]` fields.
///
/// `storage_struct` must declare a struct named `{trait_name}Storage`, which the generated impl
/// block targets.
pub(crate) fn account_component_source_with_storage(
    storage_struct: &str,
    trait_name: &str,
    method: &str,
) -> String {
    let storage_name = format!("{trait_name}Storage");
    let (trait_signature, impl_method) = split_method(method);
    format!(
        "#[component_storage]
{storage_struct}

#[component]
trait {trait_name} {{
    {trait_signature}
}}

#[component]
impl {trait_name} for {storage_name} {{
    {impl_method}
}}
"
    )
}
