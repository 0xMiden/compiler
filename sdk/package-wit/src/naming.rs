//! WIT names: identifiers, the package id, and parameter names.

use std::{collections::BTreeSet, fmt::Display};

use midenc_frontend_wasm_metadata::namespace::WIT_KEYWORDS;

/// Convert a Miden identifier to kebab case: `receive_asset` → `receive-asset`, `NoteType` →
/// `note-type`, `PRIVATE` → `private`.
///
/// The result is a valid WIT identifier for any ASCII identifier that starts with a letter, but is
/// not keyword-escaped; see [`ident`].
pub fn kebab(name: &str) -> String {
    let chars: Vec<char> = name.chars().collect();
    let mut segments: Vec<String> = Vec::new();
    let mut current = String::new();
    for (i, &c) in chars.iter().enumerate() {
        if !c.is_ascii_alphanumeric() {
            if !current.is_empty() {
                segments.push(core::mem::take(&mut current));
            }
            continue;
        }
        // A word starts at an upper-case letter that follows a lower-case letter or a digit
        // (`noteType`), or that ends a run of capitals and starts a capitalized word (`HTTPServer`
        // → `http-server`).
        let prev = i.checked_sub(1).map(|i| chars[i]);
        let next = chars.get(i + 1);
        let boundary = c.is_ascii_uppercase()
            && match prev {
                Some(p) if p.is_ascii_lowercase() || p.is_ascii_digit() => true,
                Some(p) if p.is_ascii_uppercase() => next.is_some_and(char::is_ascii_lowercase),
                _ => false,
            };
        if boundary && !current.is_empty() {
            segments.push(core::mem::take(&mut current));
        }
        current.push(c.to_ascii_lowercase());
    }
    if !current.is_empty() {
        segments.push(current);
    }

    // WIT accepts a digit-leading word after the first one (`slot-1`). The digits are still
    // joined to the previous word (`slot_1` → `slot1`) because that spelling round-trips through
    // the heck case conversions of the Rust bindings.
    let mut out = String::new();
    for segment in segments {
        if !out.is_empty() && !segment.starts_with(|c: char| c.is_ascii_digit()) {
            out.push('-');
        }
        out.push_str(&segment);
    }
    out
}

/// The WIT spelling of `name`: [`kebab`] case, `%`-escaped when it is a WIT keyword.
pub fn ident(name: &str) -> String {
    escape(kebab(name))
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

/// The last `::`-separated segment of a type name: the manifest may qualify it with its module.
pub fn short_name(name: &str) -> &str {
    name.rsplit("::").next().unwrap_or(name)
}

/// The WIT package id `<head>:<name>@<version>` of a package named `package_name` whose Miden
/// namespace starts with `head`.
///
/// The name is kebab-normalized and loses a leading `<head>-`, which the namespace part of the id
/// already says: `miden-standards-wallets-basic-wallet` → `miden:standards-wallets-basic-wallet`.
pub fn package_id(head: &str, package_name: &str, version: impl Display) -> String {
    let head = kebab(head);
    let name = kebab(package_name);
    let name = name
        .strip_prefix(&head)
        .and_then(|rest| rest.strip_prefix('-'))
        .filter(|rest| !rest.is_empty())
        .unwrap_or(&name);
    format!("{head}:{name}@{version}")
}

/// Hands out the parameter names of one function, unique within it.
#[derive(Default)]
pub struct ParamNames {
    taken: BTreeSet<String>,
}

impl ParamNames {
    /// The name of parameter `index`: the kebab name of its named type when it has one, else
    /// `arg<index>`; a name already taken gets a counter (`asset`, `asset2`).
    pub fn next(&mut self, index: usize, type_name: Option<&str>) -> String {
        let base = match type_name {
            Some(name) => kebab(short_name(name)),
            None => format!("arg{index}"),
        };
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
            ("slot_1", "slot1"),
            ("u256", "u256"),
            ("miden-standards-wallets", "miden-standards-wallets"),
        ] {
            assert_eq!(kebab(from), to, "kebab({from})");
        }
    }

    #[test]
    fn keywords_are_escaped() {
        assert_eq!(ident("type"), "%type");
        assert_eq!(ident("Record"), "%record");
        assert_eq!(ident("asset"), "asset");
        assert_eq!(ident("map"), "%map");
        assert_eq!(ident("error_context"), "%error-context");
    }

    #[test]
    fn package_id_strips_the_namespace_head() {
        assert_eq!(
            package_id("miden", "miden-standards-wallets-basic-wallet", "0.17.0"),
            "miden:standards-wallets-basic-wallet@0.17.0"
        );
        assert_eq!(package_id("miden", "miden_foo", "1.0.0"), "miden:foo@1.0.0");
        assert_eq!(package_id("miden", "other-foo", "1.0.0"), "miden:other-foo@1.0.0");
        assert_eq!(package_id("miden", "midenfoo", "1.0.0"), "miden:midenfoo@1.0.0");
        assert_eq!(package_id("miden", "miden", "1.0.0"), "miden:miden@1.0.0");
    }

    #[test]
    fn parameter_names_come_from_types_and_are_unique() {
        let mut names = ParamNames::default();
        assert_eq!(names.next(0, Some("Asset")), "asset");
        assert_eq!(names.next(1, None), "arg1");
        assert_eq!(names.next(2, Some("miden::protocol::types::Asset")), "asset2");
        assert_eq!(names.next(3, Some("NoteType")), "note-type");
        assert_eq!(names.next(4, Some("Asset")), "asset3");
    }
}
