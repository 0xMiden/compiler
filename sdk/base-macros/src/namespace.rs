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
    /// each a snake_case identifier that maps to a valid WIT identifier (see
    /// [`is_valid_segment`]).
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

/// The WIT keywords, spelled as snake_case segments; a keyword cannot name a WIT package or
/// interface.
///
/// This mirrors the keywords of the lexer of wit-parser 0.259 (`src/ast/lex.rs`) and must be
/// updated together with that dependency.
const WIT_KEYWORDS: &[&str] = &[
    "as",
    "async",
    "bool",
    "borrow",
    "char",
    "constructor",
    "enum",
    "error_context",
    "export",
    "f32",
    "f64",
    "flags",
    "from",
    "func",
    "future",
    "import",
    "include",
    "interface",
    "list",
    "map",
    "option",
    "own",
    "package",
    "record",
    "resource",
    "result",
    "s16",
    "s32",
    "s64",
    "s8",
    "static",
    "stream",
    "string",
    "tuple",
    "type",
    "u16",
    "u32",
    "u64",
    "u8",
    "use",
    "variant",
    "with",
    "world",
];

/// Returns true if `segment` is a snake_case identifier, `[a-z][a-z0-9]*(_[a-z0-9]+)*`, whose
/// kebab-case form is a valid WIT identifier that is not a WIT keyword, and that is a Rust
/// identifier (not a Rust keyword), as the generated bindings name modules after it.
///
/// `cargo miden new` checks the namespaces it derives against the same rule
/// (`tools/cargo-miden/src/template.rs`), and must follow any change to it.
fn is_valid_segment(segment: &str) -> bool {
    let is_snake_case = segment.starts_with(|ch: char| ch.is_ascii_lowercase())
        && segment.split('_').all(|word| {
            !word.is_empty()
                && word.chars().all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit())
        });
    is_snake_case
        && wit_bindgen_core::wit_parser::validate_id(&kebab(segment)).is_ok()
        && !WIT_KEYWORDS.contains(&segment)
        && syn::parse_str::<syn::Ident>(segment).is_ok()
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
             exactly three segments, each a snake_case identifier (lowercase ASCII letters and \
             digits in words joined by single `_`, starting with a letter, and not a WIT or Rust \
             keyword such as `list` or `match`), e.g. `{EXAMPLE_NAMESPACE}`. The segments name \
             the exported procedures and become the components of the storage slot names; the \
             first two (`ns::pkg`) form the WIT package id, so the `pkg` segment identifies the \
             crate and must not be shared by two crates a consumer links."
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
        for segment in ["Wallet2", "a__b", "a_", "_a", "2a", "list", "error_context", "match"] {
            let value = format!("miden::{segment}::main");
            let err = parse(&value).expect_err("the segment must be rejected").to_string();
            assert!(err.contains("snake_case"), "`{value}`: {err}");
        }
    }

    #[test]
    fn rejects_rust_keyword_segments() {
        for segment in ["match", "loop", "mod", "self", "super", "crate", "fn"] {
            let value = format!("miden::{segment}::main");
            let err = parse(&value).expect_err("a Rust keyword must be rejected").to_string();
            assert!(err.contains("Rust keyword"), "`{value}`: {err}");
        }
    }

    /// `cargo miden new` keeps its own copy of the keyword rule (a proc-macro crate cannot be its
    /// dependency): it must hold exactly the WIT keywords plus Rust keywords.
    #[test]
    fn cargo_miden_namespace_keywords_follow_the_wit_keywords() {
        let source = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tools/cargo-miden/src/template.rs"
        ));
        let start = source
            .find("const NAMESPACE_KEYWORDS: &[&str] = &[")
            .expect("cargo-miden declares `NAMESPACE_KEYWORDS`");
        let list = &source[start..];
        let list = &list[list.find("&[").unwrap() + 2..list.find("];").unwrap()];
        let namespace_keywords: Vec<&str> = list
            .split(',')
            .map(str::trim)
            .filter_map(|entry| entry.strip_prefix('"')?.strip_suffix('"'))
            .collect();
        assert!(!namespace_keywords.is_empty(), "no keywords found in cargo-miden's list");
        // Keywords are snake_case segments (`error_context`); anything else would pass the `syn`
        // check below without being a keyword.
        let malformed: Vec<&str> = namespace_keywords
            .iter()
            .copied()
            .filter(|kw| {
                !kw.starts_with(|c: char| c.is_ascii_lowercase())
                    || !kw.split('_').all(|word| {
                        !word.is_empty()
                            && word.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
                    })
            })
            .collect();
        assert!(
            malformed.is_empty(),
            "cargo-miden's `NAMESPACE_KEYWORDS` has entries that are not \
             `[a-z][a-z0-9]*(_[a-z0-9]+)*`: {malformed:?}"
        );

        let missing: Vec<&str> = WIT_KEYWORDS
            .iter()
            .copied()
            .filter(|kw| !namespace_keywords.contains(kw))
            .collect();
        assert!(missing.is_empty(), "cargo-miden's `NAMESPACE_KEYWORDS` lacks {missing:?}");
        // The rest must be Rust keywords, which the macros reject through `syn`.
        let extra: Vec<&str> = namespace_keywords
            .iter()
            .copied()
            .filter(|kw| !WIT_KEYWORDS.contains(kw) && syn::parse_str::<syn::Ident>(kw).is_ok())
            .collect();
        assert!(
            extra.is_empty(),
            "cargo-miden's `NAMESPACE_KEYWORDS` has entries that are neither WIT nor Rust \
             keywords: {extra:?}"
        );
    }
}
