//! WIT names: identifiers, the package id, and parameter names.

use std::collections::BTreeSet;

use heck::ToKebabCase;
use midenc_frontend_wasm_metadata::namespace::{
    NamespaceSegmentError, SegmentPosition, WIT_KEYWORDS, validate_namespace_segment,
};

/// The Rust keywords wit-bindgen's Rust generator turns into identifiers as they are, without the
/// `_` suffix it gives every other keyword: a function, parameter or record field with one of these
/// names would not compile in the generated bindings.
pub const UNESCAPED_RUST_KEYWORDS: &[&str] = &["gen"];

/// Convert a Miden identifier to kebab case: `receive_asset` → `receive-asset`, `NoteType` →
/// `note-type`, `PRIVATE` → `private`, `slot_1` → `slot-1`.
///
/// This is heck's kebab case, the conversion the SDK macros apply to Rust identifiers, so a name
/// given in Rust (`#[account(pkg::Iface)]`) finds the interface, and a procedure gets the same WIT
/// name whichever toolchain built its component.
///
/// The result is not keyword-escaped, and need not be a valid WIT identifier (`_1` → `1`); see
/// [`ident`].
pub fn kebab(name: &str) -> String {
    name.to_kebab_case()
}

/// The WIT spelling of `name`: [`kebab`] case, `%`-escaped when it is a WIT keyword; or, when that
/// is not a valid WIT identifier, the reason `name` has none.
pub fn ident(name: &str) -> Result<String, String> {
    let ident = escape(kebab(name));
    if is_valid(&ident) {
        Ok(ident)
    } else {
        Err(format!("`{name}` has no WIT name (derived `{ident}`)"))
    }
}

/// [`ident`] for a name wit-bindgen also turns into a Rust identifier (a function, a parameter or a
/// record field): additionally an error when that identifier is one of the
/// [`UNESCAPED_RUST_KEYWORDS`].
pub fn rust_ident(name: &str) -> Result<String, String> {
    let ident = ident(name)?;
    let rust = ident.trim_start_matches('%').replace('-', "_");
    if UNESCAPED_RUST_KEYWORDS.contains(&rust.as_str()) {
        return Err(format!(
            "`{name}` would be the Rust keyword `{rust}` in the generated bindings, which \
             wit-bindgen does not escape"
        ));
    }
    Ok(ident)
}

/// The WIT spelling `derived` of `name` as a segment of a package id or as an interface name at
/// `position`; or the reason it cannot be one.
///
/// Unlike [`ident`], the result is never `%`-escaped: the SDK macros write these names back into
/// WIT text and Rust module paths as they are. So a WIT or Rust keyword is an error, like in the
/// namespace of a Rust-built component.
fn segment(name: &str, derived: String, position: SegmentPosition) -> Result<String, String> {
    match validate_namespace_segment(&derived.replace('-', "_"), position) {
        Ok(()) => Ok(derived),
        Err(NamespaceSegmentError::NotSnakeCase) => {
            Err(format!("`{name}` has no WIT name (derived `{derived}`)"))
        }
        Err(err) if derived == name => Err(format!("`{name}` {err}")),
        Err(err) => Err(format!("`{name}` has the WIT name `{derived}`, which {err}")),
    }
}

/// The WIT interface name of the module whose last segment is `leaf`; or the reason it has none.
pub fn interface(leaf: &str) -> Result<String, String> {
    segment(leaf, kebab(leaf), SegmentPosition::Interface)
        .map_err(|err| format!("the module {err}"))
}

/// `%`-escape a kebab-case identifier when it is a WIT keyword.
pub fn escape(kebab: String) -> String {
    // The shared keyword list spells keywords as snake_case segments (`error_context`).
    if WIT_KEYWORDS.contains(&kebab.replace('-', "_").as_str()) {
        format!("%{kebab}")
    } else {
        kebab
    }
}

/// Whether `ident` is a WIT identifier as written in WIT text, in the lower-case form the
/// generator emits: words of ASCII lower-case letters and digits joined by single `-`, the first
/// word starting with a letter (wit-parser's `validate_id`), and `%`-escaped when it is a keyword.
pub fn is_valid(ident: &str) -> bool {
    let bare = match ident.strip_prefix('%') {
        Some(bare) => bare,
        None if escape(ident.to_owned()) != ident => return false,
        None => ident,
    };
    bare.split('-').enumerate().all(|(index, word)| {
        // Only the first word must start with a letter: `slot-1` is valid.
        word.starts_with(|c: char| c.is_ascii_lowercase() || (index > 0 && c.is_ascii_digit()))
            && word.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
    })
}

/// The last `::`-separated segment of a type name: the manifest may qualify it with its module.
pub fn short_name(name: &str) -> &str {
    name.rsplit("::").next().unwrap_or(name)
}

/// The namespace and name of the WIT package id `<namespace>:<name>@<version>` of a package named
/// `package_name` whose Miden namespace starts with `head`; or the reason one of them has no WIT
/// name, a WIT or Rust keyword included (see [`segment`]).
///
/// The name is kebab-normalized and loses a leading `<head>-`, which the namespace part of the id
/// already says: `miden-standards-wallets-basic-wallet` → `miden:standards-wallets-basic-wallet`.
pub fn package_id(head: &str, package_name: &str) -> Result<(String, String), String> {
    let namespace = segment(head, kebab(head), SegmentPosition::Namespace)
        .map_err(|err| format!("the namespace {err}"))?;
    let full = kebab(package_name);
    let name = full
        .strip_prefix(&namespace)
        .and_then(|rest| rest.strip_prefix('-'))
        .filter(|rest| !rest.is_empty())
        .unwrap_or(&full);
    let name = segment(package_name, name.to_owned(), SegmentPosition::Package)
        .map_err(|err| format!("the package name {err}"))?;
    Ok((namespace, name))
}

/// Hands out the parameter names of one function, unique within it.
#[derive(Default)]
pub struct ParamNames {
    taken: BTreeSet<String>,
}

impl ParamNames {
    /// The name of parameter `index`: the kebab name of its named type when that is a valid WIT
    /// name (see [`rust_ident`]), else `arg<index>`; a name already taken gets a counter (`asset`,
    /// `asset2`).
    pub fn next(&mut self, index: usize, type_name: Option<&str>) -> String {
        let base = type_name
            .and_then(|name| rust_ident(short_name(name)).ok())
            .map(|ident| ident.trim_start_matches('%').to_owned())
            .unwrap_or_else(|| format!("arg{index}"));
        let mut name = base.clone();
        let mut counter = 2;
        while self.taken.contains(&name) {
            name = format!("{base}{counter}");
            counter += 1;
        }
        self.taken.insert(name.clone());
        escape(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kebab_splits_snake_and_camel_case() {
        for (from, to) in [
            ("receive_asset", "receive-asset"),
            ("NoteType", "note-type"),
            ("AccountId", "account-id"),
            ("PausableManager", "pausable-manager"),
            ("RoleBasedAccessControl", "role-based-access-control"),
            ("PRIVATE", "private"),
            ("AUTH_CONTROLLED", "auth-controlled"),
            ("HTTPServer", "http-server"),
            ("basic_wallet", "basic-wallet"),
            ("slot_1", "slot-1"),
            ("slot1", "slot1"),
            ("Ownable2Step", "ownable2-step"),
            ("u256", "u256"),
            ("miden-standards-wallets", "miden-standards-wallets"),
            ("_1", "1"),
            ("__", ""),
            ("P2ID", "p2id"),
            ("P2IDNote", "p2id-note"),
            ("A1B", "a1b"),
            ("SLOT_1A", "slot-1a"),
        ] {
            assert_eq!(kebab(from), to, "kebab({from})");
        }
    }

    #[test]
    fn rust_keywords_wit_bindgen_does_not_escape_are_rejected() {
        assert_eq!(
            rust_ident("gen").unwrap_err(),
            "`gen` would be the Rust keyword `gen` in the generated bindings, which wit-bindgen \
             does not escape"
        );
        // wit-bindgen escapes these (`type_`, `move_`).
        assert_eq!(rust_ident("type").unwrap(), "%type");
        assert_eq!(rust_ident("move").unwrap(), "move");
        assert_eq!(rust_ident("generic").unwrap(), "generic");
    }

    #[test]
    fn unescaped_rust_keywords_match_wit_bindgen() {
        let unescaped: Vec<&str> = midenc_frontend_wasm_metadata::namespace::RUST_KEYWORDS
            .iter()
            .copied()
            .filter(|keyword| wit_bindgen_rust::to_rust_ident(keyword) == *keyword)
            .collect();
        assert_eq!(unescaped, UNESCAPED_RUST_KEYWORDS);
    }

    #[test]
    fn keywords_are_escaped() {
        assert_eq!(ident("type").unwrap(), "%type");
        assert_eq!(ident("Record").unwrap(), "%record");
        assert_eq!(ident("asset").unwrap(), "asset");
        assert_eq!(ident("map").unwrap(), "%map");
        assert_eq!(ident("error_context").unwrap(), "%error-context");
    }

    #[test]
    fn identifiers_follow_the_wit_grammar() {
        for valid in ["a", "receive-asset", "slot-1", "u256", "a1-2b", "%type", "%asset"] {
            assert!(is_valid(valid), "`{valid}` is valid");
        }
        for invalid in [
            "", "%", "1", "1-slot", "a--b", "-a", "a-", "Asset", "a_b", "type", "list", "%1", "é",
        ] {
            assert!(!is_valid(invalid), "`{invalid}` is invalid");
        }
        assert_eq!(ident("_1").unwrap_err(), "`_1` has no WIT name (derived `1`)");
        assert_eq!(ident("__").unwrap_err(), "`__` has no WIT name (derived ``)");
    }

    #[test]
    fn package_id_strips_the_namespace_head() {
        let id = |head, name| package_id(head, name).map(|(ns, name)| format!("{ns}:{name}"));
        assert_eq!(
            id("miden", "miden-standards-wallets-basic-wallet").unwrap(),
            "miden:standards-wallets-basic-wallet"
        );
        assert_eq!(id("miden", "miden_foo").unwrap(), "miden:foo");
        assert_eq!(id("miden", "other-foo").unwrap(), "miden:other-foo");
        assert_eq!(id("miden", "midenfoo").unwrap(), "miden:midenfoo");
        assert_eq!(id("miden", "miden").unwrap(), "miden:miden");
    }

    #[test]
    fn package_id_segments_are_checked() {
        assert_eq!(
            package_id("miden", "miden-list").unwrap_err(),
            "the package name `miden-list` has the WIT name `list`, which is a WIT keyword"
        );
        assert_eq!(
            package_id("miden", "miden-match").unwrap_err(),
            "the package name `miden-match` has the WIT name `match`, which is a Rust keyword"
        );
        assert_eq!(
            package_id("use", "use-foo").unwrap_err(),
            "the namespace `use` is a WIT keyword"
        );
        assert_eq!(
            package_id("mod", "mod-foo").unwrap_err(),
            "the namespace `mod` is a Rust keyword"
        );
        assert_eq!(
            package_id("_1", "foo").unwrap_err(),
            "the namespace `_1` has no WIT name (derived `1`)"
        );
        assert_eq!(
            package_id("_1", "foo").unwrap_err(),
            "the namespace `_1` has no WIT name (derived `1`)"
        );
        assert_eq!(
            package_id("miden", "miden-1x").unwrap_err(),
            "the package name `miden-1x` has no WIT name (derived `1x`)"
        );
    }

    #[test]
    fn parameter_names_come_from_types_and_are_unique() {
        let mut names = ParamNames::default();
        assert_eq!(names.next(0, Some("Asset")), "asset");
        assert_eq!(names.next(1, None), "arg1");
        assert_eq!(names.next(2, Some("miden::protocol::types::Asset")), "asset2");
        assert_eq!(names.next(3, Some("NoteType")), "note-type");
        assert_eq!(names.next(4, Some("Asset")), "asset3");
        assert_eq!(names.next(5, Some("_1")), "arg5");
        assert_eq!(names.next(6, Some("Type")), "%type");
        assert_eq!(names.next(7, Some("Gen")), "arg7");
    }

    #[test]
    fn interface_names_are_checked() {
        assert_eq!(interface("basic_wallet").unwrap(), "basic-wallet");
        assert_eq!(interface("record").unwrap_err(), "the module `record` is a WIT keyword");
        assert_eq!(
            interface("Move").unwrap_err(),
            "the module `Move` has the WIT name `move`, which is a Rust keyword"
        );
        assert_eq!(interface("_1").unwrap_err(), "the module `_1` has no WIT name (derived `1`)");
    }
}
