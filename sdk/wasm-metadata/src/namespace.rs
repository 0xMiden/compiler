//! The rule for a component namespace (`[lib].namespace`, e.g.
//! `miden::counter_contract::counter_contract`) and its segments.
//!
//! The SDK macros enforce it on the declared namespace and `cargo miden new` on the namespace it
//! derives from a project name, so both accept exactly the same namespaces.

use alloc::string::{String, ToString};
use core::fmt;

/// The WIT keywords, spelled as snake_case segments; a keyword cannot name a WIT package,
/// interface or type.
///
/// This mirrors the keywords of the lexer of wit-parser 0.259 (`src/ast/lex.rs`) and must be
/// updated together with that dependency.
pub const WIT_KEYWORDS: &[&str] = &[
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

/// The lowercase strict and reserved keywords of Rust 2024, which cannot name the modules the
/// generated bindings derive from namespace segments.
pub const RUST_KEYWORDS: &[&str] = &[
    "abstract", "as", "async", "await", "become", "box", "break", "const", "continue", "crate",
    "do", "dyn", "else", "enum", "extern", "false", "final", "fn", "for", "gen", "if", "impl",
    "in", "let", "loop", "macro", "match", "mod", "move", "mut", "override", "priv", "pub", "ref",
    "return", "self", "static", "struct", "super", "trait", "true", "try", "type", "typeof",
    "unsafe", "unsized", "use", "virtual", "where", "while", "yield",
];

/// Segments that cannot name the interface (third segment) of a namespace.
///
/// Every generated WIT package `use`s the SDK's `core-types` interface at package level, so an
/// interface of the same name collides with it.
pub const RESERVED_INTERFACE_SEGMENTS: &[&str] = &["core_types"];

/// The position of a segment in a three-segment namespace `ns::pkg::iface`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SegmentPosition {
    /// The first segment (`ns`), the WIT package namespace.
    Namespace,
    /// The second segment (`pkg`), the WIT package name.
    Package,
    /// The third segment (`iface`), the WIT interface name.
    Interface,
}

impl SegmentPosition {
    /// The positions of the three segments, in order.
    pub const ALL: [Self; 3] = [Self::Namespace, Self::Package, Self::Interface];
}

/// Why a namespace segment is invalid.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NamespaceSegmentError {
    /// The segment is not a snake_case identifier `[a-z][a-z0-9]*(_[a-z0-9]+)*`.
    NotSnakeCase,
    /// The segment is a WIT keyword.
    WitKeyword,
    /// The segment is a Rust keyword.
    RustKeyword,
    /// The segment is reserved in the interface position.
    ReservedInterface,
}

impl fmt::Display for NamespaceSegmentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::NotSnakeCase => {
                "is not a snake_case identifier (lowercase ASCII letters and digits in words \
                 joined by single `_`, starting with a letter)"
            }
            Self::WitKeyword => "is a WIT keyword",
            Self::RustKeyword => "is a Rust keyword",
            Self::ReservedInterface => {
                "is reserved as an interface name: it collides with the SDK's `core-types` WIT \
                 interface"
            }
        })
    }
}

/// Checks that `segment` may appear at `position` in a component namespace.
///
/// A segment must be a snake_case identifier, `[a-z][a-z0-9]*(_[a-z0-9]+)*` (so its kebab-case
/// form is a valid WIT identifier), that is neither a WIT keyword nor a Rust keyword, and, in the
/// interface position, not one of [`RESERVED_INTERFACE_SEGMENTS`].
pub fn validate_namespace_segment(
    segment: &str,
    position: SegmentPosition,
) -> Result<(), NamespaceSegmentError> {
    let is_snake_case = segment.starts_with(|ch: char| ch.is_ascii_lowercase())
        && segment.split('_').all(|word| {
            !word.is_empty()
                && word.chars().all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit())
        });
    if !is_snake_case {
        return Err(NamespaceSegmentError::NotSnakeCase);
    }
    if WIT_KEYWORDS.contains(&segment) {
        return Err(NamespaceSegmentError::WitKeyword);
    }
    if RUST_KEYWORDS.contains(&segment) {
        return Err(NamespaceSegmentError::RustKeyword);
    }
    if position == SegmentPosition::Interface && RESERVED_INTERFACE_SEGMENTS.contains(&segment) {
        return Err(NamespaceSegmentError::ReservedInterface);
    }
    Ok(())
}

/// Why a namespace is invalid.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NamespaceError {
    /// The namespace does not have exactly three `::`-separated segments.
    SegmentCount,
    /// A segment is invalid at its position.
    Segment {
        /// The offending segment.
        segment: String,
        /// Why the segment is invalid.
        reason: NamespaceSegmentError,
    },
}

impl fmt::Display for NamespaceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SegmentCount => f.write_str("does not have exactly three `::`-separated segments"),
            Self::Segment { segment, reason } => write!(f, "the segment `{segment}` {reason}"),
        }
    }
}

/// Checks that `namespace` is a component namespace: an optional leading `::` followed by exactly
/// three `::`-separated segments, each valid at its position (see [`validate_namespace_segment`]).
pub fn validate_namespace(namespace: &str) -> Result<(), NamespaceError> {
    let namespace = namespace.strip_prefix("::").unwrap_or(namespace);
    let mut segments = namespace.split("::");
    let (Some(ns), Some(pkg), Some(iface), None) =
        (segments.next(), segments.next(), segments.next(), segments.next())
    else {
        return Err(NamespaceError::SegmentCount);
    };
    for (segment, position) in [ns, pkg, iface].into_iter().zip(SegmentPosition::ALL) {
        validate_namespace_segment(segment, position).map_err(|reason| {
            NamespaceError::Segment {
                segment: segment.to_string(),
                reason,
            }
        })?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_whole_namespaces() {
        assert_eq!(validate_namespace("miden::counter_contract::counter_contract"), Ok(()));
        assert_eq!(validate_namespace("::acme::wallet2::main"), Ok(()));
        for namespace in ["", "miden::counter", "miden::a::b::c"] {
            assert_eq!(
                validate_namespace(namespace),
                Err(NamespaceError::SegmentCount),
                "`{namespace}`"
            );
        }
        assert_eq!(
            validate_namespace("miden::wallet::core_types"),
            Err(NamespaceError::Segment {
                segment: "core_types".to_string(),
                reason: NamespaceSegmentError::ReservedInterface,
            })
        );
        assert_eq!(
            validate_namespace("miden::match::main").unwrap_err().to_string(),
            "the segment `match` is a Rust keyword"
        );
    }

    fn validate(segment: &str) -> Result<(), NamespaceSegmentError> {
        validate_namespace_segment(segment, SegmentPosition::Package)
    }

    #[test]
    fn accepts_snake_case_segments() {
        for segment in ["counter_contract", "miden", "wallet2", "a1_2b"] {
            assert_eq!(validate(segment), Ok(()), "`{segment}`");
        }
    }

    #[test]
    fn rejects_segments_that_are_not_snake_case() {
        for segment in ["", "Wallet2", "a__b", "a_", "_a", "2a", "counter-contract", "naïve"] {
            assert_eq!(validate(segment), Err(NamespaceSegmentError::NotSnakeCase), "`{segment}`");
        }
    }

    #[test]
    fn rejects_wit_keywords() {
        for segment in ["type", "list", "error_context"] {
            assert_eq!(validate(segment), Err(NamespaceSegmentError::WitKeyword), "`{segment}`");
        }
    }

    #[test]
    fn rejects_rust_keywords() {
        for segment in ["gen", "match", "self", "crate", "try", "yield"] {
            assert_eq!(validate(segment), Err(NamespaceSegmentError::RustKeyword), "`{segment}`");
        }
    }

    #[test]
    fn reserves_core_types_in_the_interface_position_only() {
        assert_eq!(
            validate_namespace_segment("core_types", SegmentPosition::Interface),
            Err(NamespaceSegmentError::ReservedInterface)
        );
        for position in [SegmentPosition::Namespace, SegmentPosition::Package] {
            assert_eq!(validate_namespace_segment("core_types", position), Ok(()));
        }
    }
}
