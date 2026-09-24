//! The component namespace declared by `[lib].namespace` in `miden-project.toml`.
//!
//! The namespace is the single source of every name the SDK macros generate: the Miden paths of
//! the exported procedures (carried by WIT `@external-id` attributes), the WIT package and
//! interface ids, the guest trait path of the generated bindings, and the storage slot names.

use miden_assembly_syntax::ast::{Path, PathComponent};
use proc_macro2::{Span, TokenStream};
use quote::{format_ident, quote};

/// Example namespace shown in diagnostics.
const EXAMPLE_NAMESPACE: &str = "miden::counter_contract::counter_contract";

/// A validated three-segment component namespace, e.g. `miden::counter_contract::counter_contract`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ComponentNamespace {
    /// The first segment (e.g. `miden`).
    ns: String,
    /// The second segment, the package (e.g. `counter_contract`).
    pkg: String,
    /// The third segment, the interface (e.g. `counter_contract`).
    iface: String,
}

impl ComponentNamespace {
    /// Parses a `[lib].namespace` value given as an assembler path.
    ///
    /// The path must consist of exactly three unquoted segments (a leading `::` is tolerated),
    /// each made of ASCII letters, digits and `_` and not starting with `_`.
    pub(crate) fn from_path(path: &Path, span: Span) -> syn::Result<Self> {
        // The manifest loader absolutizes the path; show it the way the user wrote it.
        let raw = path.as_str().strip_prefix("::").unwrap_or(path.as_str());
        let mut segments = Vec::new();
        for component in path.components() {
            match component {
                Ok(PathComponent::Root) => {}
                Ok(PathComponent::Normal(segment)) => segments.push(segment.to_owned()),
                Err(_) => return Err(invalid_namespace(raw, span)),
            }
        }
        Self::from_segments(raw, segments, span)
    }

    /// Parses a namespace written as a plain string (`miden::pkg::iface`, optionally with a
    /// leading `::`).
    #[cfg(test)]
    pub(crate) fn parse(value: &str, span: Span) -> syn::Result<Self> {
        let segments = value
            .strip_prefix("::")
            .unwrap_or(value)
            .split("::")
            .map(str::to_owned)
            .collect();
        Self::from_segments(value, segments, span)
    }

    /// Validates the segments of the namespace `raw` and builds the namespace.
    fn from_segments(raw: &str, segments: Vec<String>, span: Span) -> syn::Result<Self> {
        let [ns, pkg, iface]: [String; 3] =
            segments.try_into().map_err(|_| invalid_namespace(raw, span))?;
        if ![&ns, &pkg, &iface].into_iter().all(|segment| is_valid_segment(segment)) {
            return Err(invalid_namespace(raw, span));
        }
        Ok(Self { ns, pkg, iface })
    }

    /// The Miden path of the namespace, e.g. `miden::counter_contract::counter_contract`.
    pub(crate) fn path(&self) -> String {
        format!("{}::{}::{}", self.ns, self.pkg, self.iface)
    }

    /// The Miden path of the procedure `leaf` exported by this component.
    pub(crate) fn procedure_path(&self, leaf: &str) -> String {
        format!("{}::{leaf}", self.path())
    }

    /// The WIT package name without version, e.g. `miden:counter-contract`.
    pub(crate) fn wit_package(&self) -> String {
        format!("{}:{}", kebab(&self.ns), kebab(&self.pkg))
    }

    /// The WIT interface name, e.g. `counter-contract`.
    pub(crate) fn wit_interface(&self) -> String {
        kebab(&self.iface)
    }

    /// The fully-qualified WIT interface id, e.g. `miden:counter-contract/counter-contract@0.1.0`.
    pub(crate) fn wit_id(&self, version: impl core::fmt::Display) -> String {
        format!("{}/{}@{version}", self.wit_package(), self.wit_interface())
    }

    /// The path of the `Guest` trait `wit-bindgen` generates for the exported interface.
    pub(crate) fn guest_trait_path(&self) -> TokenStream {
        // Mirror wit-bindgen's module naming for each kebab WIT name.
        let [ns, pkg, iface] = [&self.ns, &self.pkg, &self.iface]
            .map(|segment| format_ident!("{}", wit_bindgen_rust::to_rust_ident(&kebab(segment))));
        quote! { self::bindings::exports::#ns::#pkg::#iface::Guest }
    }

    /// The storage slot name of the storage field `field`.
    pub(crate) fn storage_slot_name(&self, field: &str) -> String {
        format!("{}::{field}", self.path())
    }
}

/// Returns true if `segment` is a non-empty `[A-Za-z0-9_]+` identifier not starting with `_`.
fn is_valid_segment(segment: &str) -> bool {
    !segment.is_empty()
        && !segment.starts_with('_')
        && segment.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

/// Converts a namespace segment into its WIT spelling.
fn kebab(segment: &str) -> String {
    segment.replace('_', "-")
}

/// Builds the diagnostic for an invalid `[lib].namespace` value.
fn invalid_namespace(raw: &str, span: Span) -> syn::Error {
    syn::Error::new(
        span,
        format!(
            "invalid `[lib].namespace` `{raw}` in `miden-project.toml`: expected a Miden path of \
             exactly three segments, each made of ASCII letters, digits and `_` and not starting \
             with `_`, e.g. `{EXAMPLE_NAMESPACE}`. The segments name the exported procedures and \
             become the components of the storage slot names."
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(value: &str) -> syn::Result<ComponentNamespace> {
        ComponentNamespace::parse(value, Span::call_site())
    }

    #[test]
    fn accepts_three_segments() {
        let namespace = parse("miden::counter_contract::counter_contract").unwrap();
        assert_eq!(namespace.path(), "miden::counter_contract::counter_contract");
        assert_eq!(namespace.wit_package(), "miden:counter-contract");
        assert_eq!(namespace.wit_interface(), "counter-contract");
        assert_eq!(namespace.wit_id("0.1.0"), "miden:counter-contract/counter-contract@0.1.0");
        assert_eq!(
            namespace.procedure_path("get_count"),
            "miden::counter_contract::counter_contract::get_count"
        );
        assert_eq!(
            namespace.storage_slot_name("count_map"),
            "miden::counter_contract::counter_contract::count_map"
        );
        assert_eq!(
            namespace.guest_trait_path().to_string(),
            "self :: bindings :: exports :: miden :: counter_contract :: counter_contract :: Guest"
        );
    }

    #[test]
    fn accepts_leading_root() {
        let namespace = parse("::acme::Wallet2::main").unwrap();
        assert_eq!(namespace.path(), "acme::Wallet2::main");
    }

    #[test]
    fn accepts_assembler_path() {
        let path = Path::new("::miden::p2id::p2id");
        let namespace = ComponentNamespace::from_path(path, Span::call_site()).unwrap();
        assert_eq!(namespace.path(), "miden::p2id::p2id");
    }

    #[test]
    fn rejects_two_segments() {
        let err = parse("miden::counter_contract").unwrap_err().to_string();
        assert!(err.contains("exactly three segments"), "{err}");
        assert!(err.contains(EXAMPLE_NAMESPACE), "{err}");
        assert!(err.contains("storage slot"), "{err}");
    }

    #[test]
    fn rejects_quoted_segment() {
        let path = Path::new("::\"miden:counter-contract/counter-contract@0.1.0\"");
        let err = ComponentNamespace::from_path(path, Span::call_site()).unwrap_err().to_string();
        assert!(err.contains("miden:counter-contract/counter-contract@0.1.0"), "{err}");
    }

    #[test]
    fn rejects_leading_underscore() {
        assert!(parse("miden::_private::counter").is_err());
    }

    #[test]
    fn rejects_bad_characters() {
        assert!(parse("miden::counter-contract::counter").is_err());
    }
}
