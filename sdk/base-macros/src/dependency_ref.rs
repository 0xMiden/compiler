//! Shared parsing and selection of `package::Interface` dependency references.
//!
//! Both `#[account(...)]` and `#[component(...)]` name their Miden package dependencies as
//! two-segment Rust paths: the first segment is the manifest dependency (snake_case form of the
//! kebab-case package name), the second names the dependency's exported WIT interface in
//! UpperCamelCase (kebab-cased for the WIT lookup).

use std::collections::HashSet;

use heck::{ToKebabCase, ToUpperCamelCase};
use proc_macro2::Span;
use syn::{
    Error, Token,
    parse::{Parse, ParseStream},
    punctuated::Punctuated,
    spanned::Spanned,
};

use crate::wit_world::{ManifestPackage, SelectedDependency};

/// One parsed `package::Interface` (optionally `… as Alias`) dependency reference.
#[derive(Debug)]
pub(crate) struct DependencyRef {
    /// Kebab-case Miden package dependency name used for the manifest lookup.
    pub(crate) package: String,
    /// Package path segment as written, echoed verbatim in diagnostics so the suggested fix
    /// matches what the user typed (e.g. `counter_contract`, not the kebab-cased lookup key).
    pub(crate) package_ident: syn::Ident,
    /// Kebab-case WIT interface name used for the dependency lookup.
    pub(crate) interface: String,
    /// Interface path segment as written; the generated trait name unless `alias` overrides it.
    pub(crate) interface_ident: syn::Ident,
    /// Optional `as Alias` override for the generated trait name. Lets a reference select an
    /// interface while naming the generated trait differently — e.g. to avoid a clash when one
    /// crate uses the same interface as both a sibling component and an `#[account]` FPI wrapper,
    /// or when two packages export the same interface name.
    pub(crate) alias: Option<syn::Ident>,
    /// Span of the whole reference, for diagnostics.
    pub(crate) span: Span,
}

impl DependencyRef {
    /// The name of the Rust trait generated for this component: the `as Alias` override when
    /// present, otherwise the interface segment as written.
    pub(crate) fn trait_ident(&self) -> &syn::Ident {
        self.alias.as_ref().unwrap_or(&self.interface_ident)
    }
}

/// One `package::Interface` or `package::Interface as Alias` item, before validation.
struct RawDependencyRef {
    /// The `package::Interface` path selecting the dependency and its exported WIT interface.
    path: syn::Path,
    /// Optional `as Alias` override for the generated trait name.
    alias: Option<syn::Ident>,
}

impl Parse for RawDependencyRef {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let path = input.parse::<syn::Path>()?;
        let alias = if input.peek(Token![as]) {
            input.parse::<Token![as]>()?;
            Some(input.parse::<syn::Ident>()?)
        } else {
            None
        };
        Ok(Self { path, alias })
    }
}

/// Parsed `#[account(...)]` / `#[component(...)]` dependency reference list.
#[derive(Debug)]
pub(crate) struct DependencyRefArgs {
    pub(crate) refs: Vec<DependencyRef>,
}

impl Parse for DependencyRefArgs {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let raw = Punctuated::<RawDependencyRef, Token![,]>::parse_terminated(input)?;
        let mut refs = Vec::with_capacity(raw.len());
        let mut seen = HashSet::new();
        for raw_ref in &raw {
            let mut dependency_ref = parse_dependency_ref(&raw_ref.path)?;
            if let Some(alias) = &raw_ref.alias {
                validate_alias(alias)?;
            }
            dependency_ref.alias = raw_ref.alias.clone();
            if !seen.insert((dependency_ref.package.clone(), dependency_ref.interface.clone())) {
                return Err(Error::new(
                    dependency_ref.span,
                    format!(
                        "duplicate dependency reference `{}::{}`",
                        dependency_ref.package_ident, dependency_ref.interface_ident
                    ),
                ));
            }
            refs.push(dependency_ref);
        }
        Ok(Self { refs })
    }
}

/// Rejects an `as Alias` that is not UpperCamelCase.
///
/// The alias becomes the generated trait name, so a snake_case or lowercase alias would fire
/// `non_camel_case_types` on macro-generated code the user cannot easily silence. Requiring an
/// UpperCamelCase alias keeps the generated trait name idiomatic.
fn validate_alias(alias: &syn::Ident) -> syn::Result<()> {
    let name = alias.to_string();
    let is_upper_camel =
        name.chars().next().is_some_and(|c| c.is_ascii_uppercase()) && !name.contains('_');
    if !is_upper_camel {
        return Err(Error::new(
            alias.span(),
            format!(
                "account trait alias `{name}` must be written in UpperCamelCase (e.g. \
                 `RemoteCounter`) so the generated trait name is idiomatic"
            ),
        ));
    }
    Ok(())
}

/// Validates one attribute path and splits it into package and interface names.
fn parse_dependency_ref(path: &syn::Path) -> syn::Result<DependencyRef> {
    if path.leading_colon.is_some() {
        return Err(Error::new(
            path.span(),
            "dependency references cannot start with `::`; write `package::Interface`",
        ));
    }
    if let Some(segment) = path.segments.iter().find(|segment| !segment.arguments.is_none()) {
        return Err(Error::new(
            segment.arguments.span(),
            "dependency references cannot use generic arguments; write `package::Interface`",
        ));
    }

    match path.segments.len() {
        2 => {
            let package_ident = path.segments[0].ident.clone();
            let interface_ident = path.segments[1].ident.clone();
            Ok(DependencyRef {
                package: package_ident.to_string().to_kebab_case(),
                package_ident,
                interface: interface_ident.to_string().to_kebab_case(),
                interface_ident,
                alias: None,
                span: path.span(),
            })
        }
        1 => {
            let package_ident = &path.segments[0].ident;
            let package = package_ident.to_string().to_kebab_case();
            let suggested_interface = package_ident.to_string().to_upper_camel_case();
            Err(Error::new(
                path.span(),
                format!(
                    "dependency reference `{package_ident}` is missing the WIT interface name; \
                     write `{package_ident}::{suggested_interface}` to select the `{}` interface \
                     exported by package `{package}`",
                    suggested_interface.to_kebab_case(),
                ),
            ))
        }
        _ => Err(Error::new(
            path.span(),
            "dependency references must have exactly two segments; write `package::Interface`",
        )),
    }
}

/// Loads the project manifest dependencies and narrows each reference to its interface.
pub(crate) fn select_dependencies(
    manifest: &ManifestPackage,
    requested: &[DependencyRef],
    error_span: Span,
) -> syn::Result<Vec<SelectedDependency>> {
    let dependencies = manifest.collect_miden_dependencies(error_span)?;
    let available = dependencies
        .iter()
        .map(|dependency| dependency.name.clone())
        .collect::<Vec<_>>();

    requested
        .iter()
        .map(|reference| {
            let dependency = dependencies
                .iter()
                .find(|dependency| dependency.name == reference.package)
                .ok_or_else(|| {
                    Error::new(
                        reference.span,
                        format!(
                            "dependency `{}` is not declared in miden-project.toml; available \
                             dependencies: {}",
                            reference.package_ident,
                            format_available_dependencies(&available)
                        ),
                    )
                })?;
            dependency.select(&reference.interface).ok_or_else(|| {
                Error::new(
                    reference.span,
                    format!(
                        "dependency `{}` does not export a WIT interface named `{}` (from `{}`); \
                         exported interfaces: {}",
                        reference.package_ident,
                        reference.interface,
                        reference.interface_ident,
                        format_available_dependencies(
                            &dependency
                                .interface_names()
                                .iter()
                                .map(|name| name.to_string())
                                .collect::<Vec<_>>()
                        )
                    ),
                )
            })
        })
        .collect()
}

/// Formats a name list for diagnostics.
pub(crate) fn format_available_dependencies(available: &[String]) -> String {
    if available.is_empty() {
        "none".to_string()
    } else {
        available.join(", ")
    }
}

#[cfg(test)]
mod tests {
    use quote::quote;

    use super::*;

    #[test]
    fn parses_two_segment_references() {
        let args = syn::parse2::<DependencyRefArgs>(quote! {
            counter_contract::CounterContract, pausable::Pausable
        })
        .unwrap();

        assert_eq!(args.refs.len(), 2);
        assert_eq!(args.refs[0].package, "counter-contract");
        assert_eq!(args.refs[0].interface, "counter-contract");
        assert_eq!(args.refs[0].interface_ident, "CounterContract");
        assert_eq!(args.refs[1].package, "pausable");
        assert_eq!(args.refs[1].interface, "pausable");
    }

    #[test]
    fn rejects_bare_package_reference_with_migration_message() {
        let err = syn::parse2::<DependencyRefArgs>(quote!(counter_contract)).unwrap_err();
        let message = err.to_string();

        assert!(message.contains("missing the WIT interface name"));
        assert!(message.contains("counter_contract::CounterContract"));
        assert!(message.contains("package `counter-contract`"));
    }

    #[test]
    fn rejects_overlong_references() {
        let err = syn::parse2::<DependencyRefArgs>(quote!(a::b::C)).unwrap_err();
        assert!(err.to_string().contains("exactly two segments"));
    }

    #[test]
    fn rejects_leading_colons_and_generics() {
        let err = syn::parse2::<DependencyRefArgs>(quote!(::a::B)).unwrap_err();
        assert!(err.to_string().contains("cannot start with `::`"));

        let err = syn::parse2::<DependencyRefArgs>(quote!(a::B<u32>)).unwrap_err();
        assert!(err.to_string().contains("cannot use generic arguments"));
    }

    #[test]
    fn rejects_duplicate_references() {
        let err = syn::parse2::<DependencyRefArgs>(quote! {
            counter_contract::CounterContract, counter_contract::CounterContract
        })
        .unwrap_err();
        assert!(err.to_string().contains("duplicate dependency reference"));
    }

    #[test]
    fn allows_distinct_interfaces_of_one_package() {
        let args = syn::parse2::<DependencyRefArgs>(quote! {
            counter_contract::CounterContract, counter_contract::Pausable
        })
        .unwrap();
        assert_eq!(args.refs.len(), 2);
    }

    #[test]
    fn parses_as_alias_overriding_the_trait_name() {
        let args = syn::parse2::<DependencyRefArgs>(quote! {
            counter_contract::CounterContract as RemoteCounter
        })
        .unwrap();

        assert_eq!(args.refs.len(), 1);
        // The interface lookup still uses the path segment, not the alias.
        assert_eq!(args.refs[0].interface, "counter-contract");
        assert_eq!(args.refs[0].interface_ident, "CounterContract");
        // The generated trait name is the alias.
        assert_eq!(args.refs[0].alias.as_ref().unwrap(), "RemoteCounter");
        assert_eq!(args.refs[0].trait_ident().to_string(), "RemoteCounter");
    }

    #[test]
    fn trait_ident_defaults_to_the_interface_without_an_alias() {
        let args =
            syn::parse2::<DependencyRefArgs>(quote!(counter_contract::CounterContract)).unwrap();
        assert!(args.refs[0].alias.is_none());
        assert_eq!(args.refs[0].trait_ident().to_string(), "CounterContract");
    }

    #[test]
    fn rejects_non_upper_camel_case_alias() {
        let err = syn::parse2::<DependencyRefArgs>(quote! {
            counter_contract::CounterContract as remote_counter
        })
        .unwrap_err();
        assert!(err.to_string().contains("must be written in UpperCamelCase"), "{err}");
    }
}
