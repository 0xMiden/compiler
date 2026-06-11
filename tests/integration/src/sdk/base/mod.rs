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

/// Splits a `pub fn ...` method definition into its trait signature and its (non-`pub`) impl method.
///
/// The method's opening brace is always the first `{`, so the signature is everything before it.
pub(crate) fn split_method(method: &str) -> (String, String) {
    let signature = method.split('{').next().unwrap_or_default().trim();
    let signature = signature.strip_prefix("pub ").unwrap_or(signature);
    let trait_signature = format!("{signature};");
    let impl_method = method.trim().strip_prefix("pub ").unwrap_or(method.trim()).to_string();
    (trait_signature, impl_method)
}

/// Renders the three-part account-component source (`#[component_storage]` struct, `#[component]`
/// trait, and `#[component]` trait impl) for a binding test.
///
/// `storage_struct` is the storage struct declaration (e.g. `struct TestAssetStorage;` or with
/// `#[storage(...)]` fields). `trait_name` is the component trait; the impl targets `storage_name`.
pub(crate) fn account_component_source(
    storage_struct: &str,
    storage_name: &str,
    trait_name: &str,
    method: &str,
) -> String {
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
