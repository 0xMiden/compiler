//! WIT names: identifiers, the package id, and parameter names.

use std::collections::BTreeSet;

use heck::{ToKebabCase, ToUpperCamelCase};
use midenc_frontend_wasm_metadata::{
    FPI_ABI_PARAM_NAMES,
    namespace::{NamespaceSegmentError, SegmentPosition, WIT_KEYWORDS, validate_namespace_segment},
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

/// The Rust spelling wit-bindgen gives an enum case whose WIT spelling is `ident` (see
/// [`ident`]): heck's upper camel case.
pub fn upper_camel(ident: &str) -> String {
    ident.trim_start_matches('%').to_upper_camel_case()
}

/// The Rust spelling wit-bindgen gives a type whose WIT spelling is `ident` (see [`ident`]):
/// [`upper_camel`], except `guest`, which becomes `Guest_` because wit-bindgen reserves `Guest`
/// for the traits of exported interfaces.
pub fn rust_type_name(ident: &str) -> String {
    match ident.trim_start_matches('%') {
        "guest" => "Guest_".to_owned(),
        _ => upper_camel(ident),
    }
}

/// The [`rust_type_name`] Rust spelling of the type `name` whose WIT spelling is `ident`; or,
/// when that spelling is the Rust keyword `Self`, the reason `name` has none.
pub fn rust_type_ident(name: &str, ident: &str) -> Result<String, String> {
    not_self(name, rust_type_name(ident))
}

/// The [`upper_camel`] Rust spelling of the enum case `name` whose WIT spelling is `ident`; or,
/// when that spelling is the Rust keyword `Self`, the reason `name` has none.
pub fn rust_case_ident(name: &str, ident: &str) -> Result<String, String> {
    not_self(name, upper_camel(ident))
}

/// `rust`, the Rust spelling of `name`; or, when it is the Rust keyword `Self`, the reason `name`
/// has none.
fn not_self(name: &str, rust: String) -> Result<String, String> {
    if rust == "Self" {
        return Err(format!(
            "`{name}` would be the Rust keyword `Self` in the generated bindings, which \
             wit-bindgen does not escape"
        ));
    }
    Ok(rust)
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
/// already says: `miden-standards-wallets-basic-wallet` → `miden:standards-wallets-basic-wallet`;
/// it keeps the head when the rest alone would be a keyword or no WIT name: `miden-list` →
/// `miden:miden-list`.
pub fn package_id(head: &str, package_name: &str) -> Result<(String, String), String> {
    let namespace = segment(head, kebab(head), SegmentPosition::Namespace)
        .map_err(|err| format!("the namespace {err}"))?;
    let full = kebab(package_name);
    let name = full
        .strip_prefix(&namespace)
        .and_then(|rest| rest.strip_prefix('-'))
        .filter(|rest| !rest.is_empty())
        .unwrap_or(&full);
    // A stripped name that is a keyword or no WIT name (`miden-list` → `list`, `miden-1x` → `1x`)
    // falls back to the full name.
    let name = segment(package_name, name.to_owned(), SegmentPosition::Package)
        .or_else(|err| {
            if name == full {
                return Err(err);
            }
            segment(package_name, full.clone(), SegmentPosition::Package).map_err(|_| err)
        })
        .map_err(|err| format!("the package name {err}"))?;
    Ok((namespace, name))
}

/// Hands out the parameter names of one function, unique within it and distinct from the
/// [`FPI_ABI_PARAM_NAMES`] the SDK's FPI imports prepend to them.
pub struct ParamNames {
    /// The unescaped names handed out so far, and the FPI parameter names.
    taken: BTreeSet<String>,
}

impl Default for ParamNames {
    fn default() -> Self {
        Self {
            taken: FPI_ABI_PARAM_NAMES.iter().map(|name| (*name).to_owned()).collect(),
        }
    }
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

    /// Snake, camel and screaming-case names convert to kebab case at word and acronym breaks.
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

    /// Only Rust keywords that wit-bindgen leaves unescaped are rejected as identifiers.
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

    /// The list of unescaped Rust keywords matches what wit-bindgen actually leaves unescaped.
    #[test]
    fn unescaped_rust_keywords_match_wit_bindgen() {
        let unescaped: Vec<&str> = midenc_frontend_wasm_metadata::namespace::RUST_KEYWORDS
            .iter()
            .copied()
            .filter(|keyword| wit_bindgen_rust::to_rust_ident(keyword) == *keyword)
            .collect();
        assert_eq!(unescaped, UNESCAPED_RUST_KEYWORDS);
    }

    /// Rust type and case names match wit-bindgen's, and unescaped `Self` is rejected.
    #[test]
    fn rust_type_names_follow_wit_bindgen() {
        let rust = |name: &str| rust_type_ident(name, &ident(name).unwrap());
        assert_eq!(rust("SLOT1").unwrap(), "Slot1");
        assert_eq!(rust("SLOT_1").unwrap(), "Slot1");
        assert_eq!(rust("AUTH_CONTROLLED").unwrap(), "AuthControlled");
        assert_eq!(rust("Record").unwrap(), "Record");
        assert_eq!(rust("Guest").unwrap(), "Guest_");
        // wit-bindgen remaps `guest` for type names only.
        assert_eq!(rust_case_ident("GUEST", &ident("GUEST").unwrap()).unwrap(), "Guest");
        assert_eq!(
            rust_case_ident("SELF", &ident("SELF").unwrap()).unwrap_err(),
            "`SELF` would be the Rust keyword `Self` in the generated bindings, which wit-bindgen \
             does not escape"
        );
        assert_eq!(
            rust("SELF").unwrap_err(),
            "`SELF` would be the Rust keyword `Self` in the generated bindings, which wit-bindgen \
             does not escape"
        );
    }

    /// WIT keywords get a `%` prefix, other names stay bare.
    #[test]
    fn keywords_are_escaped() {
        assert_eq!(ident("type").unwrap(), "%type");
        assert_eq!(ident("Record").unwrap(), "%record");
        assert_eq!(ident("asset").unwrap(), "asset");
        assert_eq!(ident("map").unwrap(), "%map");
        assert_eq!(ident("error_context").unwrap(), "%error-context");
    }

    /// Identifiers are valid only per the WIT grammar, and names deriving none are rejected.
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

    /// The package id drops a leading namespace segment from the package name.
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

    /// Package id segments that are keywords or have no WIT name fall back or are rejected.
    #[test]
    fn package_id_segments_are_checked() {
        // A keyword or invalid name left by stripping the head falls back to the full name...
        let id = |head, name| package_id(head, name).map(|(ns, name)| format!("{ns}:{name}"));
        assert_eq!(id("miden", "miden-list").unwrap(), "miden:miden-list");
        assert_eq!(id("miden", "miden-match").unwrap(), "miden:miden-match");
        // ...and a package name that is a keyword in full is rejected.
        assert_eq!(
            package_id("miden", "list").unwrap_err(),
            "the package name `list` is a WIT keyword"
        );
        assert_eq!(
            package_id("miden", "Match").unwrap_err(),
            "the package name `Match` has the WIT name `match`, which is a Rust keyword"
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
        assert_eq!(id("miden", "miden-1x").unwrap(), "miden:miden-1x");
        assert_eq!(
            package_id("miden", "_1x").unwrap_err(),
            "the package name `_1x` has no WIT name (derived `1x`)"
        );
    }

    /// Parameter names derive from their type names, get numbered on reuse, else `arg<N>`.
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

    /// A type-derived parameter name never takes a name the FPI imports prepend.
    #[test]
    fn parameter_names_avoid_the_fpi_parameters() {
        let mut names = ParamNames::default();
        assert_eq!(names.next(0, Some("AccountIdPrefix")), "account-id-prefix2");
        assert_eq!(names.next(1, Some("ForeignProcRoot")), "foreign-proc-root2");
    }

    /// Module names that are keywords or have no WIT name are rejected as interface names.
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
