//! Attribute macro for explicit active and foreign account bindings.

use heck::ToSnakeCase;
use proc_macro2::{Ident, Span};
use quote::format_ident;
use syn::{Error, Fields, ItemStruct, spanned::Spanned};

use crate::{
    dependency_ref::{DependencyRef, DependencyRefArgs, select_dependencies},
    fpi, generate, manifest_paths,
    wit_world::{ManifestPackage, import_world_wit},
};

const FOREIGN_ACCOUNT_WORLD: &str = "foreign-account-bindings";

/// Expands `#[account(...)]` into a typed account API wrapper.
pub(crate) fn expand(
    attr: proc_macro::TokenStream,
    item: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    match expand_inner(attr.into(), item.into()) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

/// Performs fallible expansion for `#[account(...)]`.
fn expand_inner(
    attr: proc_macro2::TokenStream,
    item: proc_macro2::TokenStream,
) -> syn::Result<proc_macro2::TokenStream> {
    let args = syn::parse2::<DependencyRefArgs>(attr)?;
    if args.refs.is_empty() {
        return Err(Error::new(
            Span::call_site(),
            "account requires at least one dependency reference written as a Rust-style Miden \
             package name and its WIT interface, for example \
             #[account(counter_contract::CounterContract)] for package `counter-contract`",
        ));
    }
    let account_struct = syn::parse2::<ItemStruct>(item)?;
    validate_empty_struct(&account_struct)?;
    reject_duplicate_trait_names(&args.refs)?;
    reject_struct_trait_name_collision(&account_struct, &args.refs)?;

    let manifest = ManifestPackage::load(Span::call_site())?;
    let dependencies = select_dependencies(&manifest, &args.refs, Span::call_site())?;
    let imports = dependencies
        .iter()
        .map(|dependency| dependency.import().to_owned())
        .collect::<Vec<_>>();
    // The trait name generated for each component is the interface segment as written, parallel to
    // `dependencies` (both follow the order of `args.refs`).
    let trait_idents = args
        .refs
        .iter()
        .map(|reference| reference.interface_ident.clone())
        .collect::<Vec<_>>();
    let with_entries = fpi::dependency_type_with_entries(&dependencies);
    let inline_wit = import_world_wit(FOREIGN_ACCOUNT_WORLD, &imports);
    let wit_config = manifest_paths::resolve_wit_paths(manifest_paths::ResolveOptions {
        allow_missing_local_wit: true,
    })?;
    let bindings = generate::generate_inline_fpi_bindings(
        &wit_config,
        &inline_wit,
        FOREIGN_ACCOUNT_WORLD,
        &imports,
        &with_entries,
    )?;
    let binding_module_ident = binding_module_ident(&account_struct.ident);

    fpi::augment_foreign_account_bindings(
        bindings,
        account_struct,
        dependencies,
        trait_idents,
        binding_module_ident,
    )
}

/// Rejects references whose generated component traits would share a name with the wrapper struct.
///
/// Each `package::Interface` reference generates a `pub trait <Interface>` next to the `#[account]`
/// struct, so a reference whose interface segment equals the struct name would put a struct and a
/// trait with the same name in one module — a raw `E0428` far from its cause. Catch it here.
fn reject_struct_trait_name_collision(
    account_struct: &ItemStruct,
    refs: &[DependencyRef],
) -> syn::Result<()> {
    if let Some(reference) =
        refs.iter().find(|reference| reference.interface_ident == account_struct.ident)
    {
        return Err(Error::new(
            reference.span,
            format!(
                "account reference `{}::{}` generates a trait named `{}`, which collides with the \
                 `#[account]` struct `{}`; rename the struct so it differs from its component \
                 interface names",
                reference.package_ident,
                reference.interface_ident,
                reference.interface_ident,
                account_struct.ident,
            ),
        ));
    }
    Ok(())
}

/// Rejects references whose interface segments would generate identically named component traits.
fn reject_duplicate_trait_names(refs: &[DependencyRef]) -> syn::Result<()> {
    for (index, reference) in refs.iter().enumerate() {
        if let Some(previous) = refs[..index]
            .iter()
            .find(|previous| previous.interface_ident == reference.interface_ident)
        {
            return Err(Error::new(
                reference.span,
                format!(
                    "account references `{}::{}` and `{}::{}` would both generate a trait named \
                     `{}`; component interface names must be unique within one `#[account]`",
                    previous.package_ident,
                    previous.interface_ident,
                    reference.package_ident,
                    reference.interface_ident,
                    reference.interface_ident,
                ),
            ));
        }
    }
    Ok(())
}

/// Verifies that the attribute is applied to a non-generic empty struct.
fn validate_empty_struct(account_struct: &ItemStruct) -> syn::Result<()> {
    if !account_struct.generics.params.is_empty() {
        return Err(Error::new(
            account_struct.generics.span(),
            "account supports only non-generic structs",
        ));
    }

    match &account_struct.fields {
        Fields::Unit => Ok(()),
        Fields::Named(fields) if fields.named.is_empty() => Ok(()),
        Fields::Unnamed(fields) if fields.unnamed.is_empty() => Ok(()),
        _ => Err(Error::new(
            account_struct.fields.span(),
            "account must be applied to an empty struct; remove all fields because the macro \
             generates account wrapper methods on that type",
        )),
    }
}

/// Builds a stable hidden module name for the generated FPI WIT bindings.
fn binding_module_ident(account_ident: &Ident) -> Ident {
    format_ident!("__miden_foreign_account_{}", account_ident.to_string().to_snake_case())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_missing_dependency_names_with_actionable_message() {
        let err = expand_inner(
            quote::quote!(),
            quote::quote! {
                struct CounterAccount;
            },
        )
        .unwrap_err();
        let message = err.to_string();

        assert!(message.contains("account requires at least one dependency reference"));
        assert!(message.contains("Rust-style Miden package"));
        assert!(message.contains("#[account(counter_contract::CounterContract)]"));
    }

    #[test]
    fn validates_dependency_names_before_struct_shape() {
        let err = expand_inner(
            quote::quote!(),
            quote::quote! {
                struct CounterAccount {
                    account_id: miden::AccountId,
                }
            },
        )
        .unwrap_err();
        let message = err.to_string();

        assert!(message.contains("account requires at least one dependency reference"));
    }

    #[test]
    fn rejects_bare_package_reference_with_migration_message() {
        let err = expand_inner(
            quote::quote!(counter_contract),
            quote::quote! {
                struct CounterAccount;
            },
        )
        .unwrap_err();
        let message = err.to_string();

        assert!(message.contains("missing the WIT interface name"));
        assert!(message.contains("counter_contract::CounterContract"));
    }

    #[test]
    fn rejects_non_empty_struct_with_actionable_message() {
        let account_struct = syn::parse_quote! {
            struct CounterAccount {
                account_id: miden::AccountId,
            }
        };
        let err = validate_empty_struct(&account_struct).unwrap_err();
        let message = err.to_string();

        assert!(message.contains("account must be applied to an empty struct"));
        assert!(message.contains("remove all fields"));
    }

    #[test]
    fn rejects_struct_named_like_a_generated_trait() {
        let err = expand_inner(
            quote::quote!(counter_contract::CounterContract),
            quote::quote! {
                struct CounterContract;
            },
        )
        .unwrap_err();
        let message = err.to_string();

        assert!(message.contains("generates a trait named `CounterContract`"), "{message}");
        assert!(message.contains("collides with the `#[account]` struct `CounterContract`"));
        assert!(message.contains("rename the struct"));
    }

    #[test]
    fn rejects_duplicate_generated_trait_names_across_packages() {
        let err = expand_inner(
            quote::quote!(first_counter::Counter, second_counter::Counter),
            quote::quote! {
                struct Wallet;
            },
        )
        .unwrap_err();
        let message = err.to_string();

        assert!(message.contains("would both generate a trait named `Counter`"), "{message}");
        assert!(message.contains("first_counter::Counter"));
        assert!(message.contains("second_counter::Counter"));
    }
}
