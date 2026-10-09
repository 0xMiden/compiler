//! The component namespace declared by `[lib].namespace` in `miden-project.toml`.
//!
//! The namespace is the single source of every name the SDK macros generate: the Miden paths of
//! the exported procedures and of the generated FPI (`<namespace>::fpi::<dependency path>::<fn>`)
//! and stored-procedure (`<namespace>::dyncall::<field>`) imports, all carried by WIT
//! `@external-id` attributes; the WIT package and interface ids; the guest trait path of the
//! generated bindings; and the storage slot names.

use miden_assembly_syntax::ast::Path;
use midenc_frontend_wasm_metadata::namespace::{NamespaceError, validate_namespace};
use proc_macro2::{Span, TokenStream};
use quote::quote;

use crate::wit_names::wit_bindgen_rust_ident;

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
    /// The path must be a component namespace by the rule of [`validate_namespace`]: exactly three
    /// unquoted segments (a leading `::` is tolerated), each valid in its position, outside the
    /// reserved library namespaces.
    pub(crate) fn from_path(path: &Path, span: Span) -> syn::Result<Self> {
        // The manifest loader absolutizes the path; show it the way the user wrote it.
        let raw = path.as_str().strip_prefix("::").unwrap_or(path.as_str());
        validate_namespace(raw).map_err(|err| invalid_namespace(raw, span, err))?;
        let mut segments = raw.split("::").map(str::to_owned);
        let (Some(ns), Some(pkg), Some(iface)) =
            (segments.next(), segments.next(), segments.next())
        else {
            unreachable!("a validated namespace has three segments");
        };
        Ok(Self { ns, pkg, iface })
    }

    /// Parses a namespace written as a plain string (`miden::pkg::iface`, optionally with a
    /// leading `::`).
    #[cfg(test)]
    pub(crate) fn parse(value: &str, span: Span) -> syn::Result<Self> {
        Self::from_path(Path::new(value), span)
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
            .map(|segment| wit_bindgen_rust_ident(&kebab(segment), Span::call_site()));
        quote! { self::bindings::exports::#ns::#pkg::#iface::Guest }
    }

    /// The Miden path carried by the stored-procedure dispatch import of the storage field
    /// `field`, e.g. `miden::counter_contract::counter_contract::dyncall::authority`.
    pub(crate) fn dyncall_path(&self, field: &str) -> String {
        format!("{}::dyncall::{field}", self.path())
    }

    /// The storage slot name of the storage field `field`.
    pub(crate) fn storage_slot_name(&self, field: &str) -> String {
        format!("{}::{field}", self.path())
    }
}

/// Converts a namespace segment into its WIT spelling.
fn kebab(segment: &str) -> String {
    segment.replace('_', "-")
}

/// Builds the diagnostic for an invalid `[lib].namespace` value, naming the reason unless the
/// segment count is at fault.
fn invalid_namespace(raw: &str, span: Span, err: NamespaceError) -> syn::Error {
    let reason = match err {
        NamespaceError::SegmentCount => String::new(),
        NamespaceError::Segment { .. } => format!("{err}; "),
        NamespaceError::Reserved { .. } => format!("the namespace {err}; "),
    };
    syn::Error::new(
        span,
        format!(
            "invalid `[lib].namespace` `{raw}` in `miden-project.toml`: {reason}expected a Miden \
             path of exactly three segments, e.g. `{EXAMPLE_NAMESPACE}`. The segments name the \
             exported procedures and become the components of the storage slot names; the first \
             two (`ns::pkg`) form the WIT package id, so the `pkg` segment identifies the crate \
             and must not be shared by two crates a consumer links."
        ),
    )
}

#[cfg(test)]
mod tests {
    use midenc_frontend_wasm_metadata::namespace::NamespaceSegmentError;

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
        let namespace = parse("::acme::wallet2::main").unwrap();
        assert_eq!(namespace.path(), "acme::wallet2::main");
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

    #[test]
    fn rejects_segments_without_a_valid_wit_spelling() {
        let not_snake_case = NamespaceSegmentError::NotSnakeCase;
        for (segment, reason) in [
            ("Wallet2", not_snake_case),
            ("a__b", not_snake_case),
            ("a_", not_snake_case),
            ("_a", not_snake_case),
            ("2a", not_snake_case),
            ("list", NamespaceSegmentError::WitKeyword),
            ("error_context", NamespaceSegmentError::WitKeyword),
            ("match", NamespaceSegmentError::RustKeyword),
        ] {
            let value = format!("miden::{segment}::main");
            let err = parse(&value).expect_err("the segment must be rejected").to_string();
            assert!(
                err.contains(&format!("the segment `{segment}` {reason}; expected")),
                "`{value}`: {err}"
            );
            assert_eq!(err.matches("snake_case").count(), usize::from(reason == not_snake_case));
        }
    }

    #[test]
    fn rejects_rust_keyword_segments() {
        for segment in ["match", "loop", "mod", "self", "super", "crate", "fn", "gen"] {
            let value = format!("miden::{segment}::main");
            let err = parse(&value).expect_err("a Rust keyword must be rejected").to_string();
            assert!(
                err.contains(&format!("the segment `{segment}` is a Rust keyword")),
                "`{value}`: {err}"
            );
        }
    }

    #[test]
    fn rejects_the_sdk_interface_name_as_interface_segment() {
        let err = parse("miden::wallet::core_types").unwrap_err().to_string();
        assert!(err.contains("the segment `core_types` is reserved"), "{err}");
        assert!(parse("miden::core_types::wallet").is_ok());
    }

    #[test]
    fn rejects_library_namespaces() {
        let err = parse("miden::protocol::wallet").unwrap_err().to_string();
        assert!(
            err.contains("the namespace is reserved for the `miden::protocol` library; expected"),
            "{err}"
        );
        assert!(parse("miden::protocol_x::wallet").is_ok());
    }
}
