//! WIT names: identifiers, the package id, and parameter names.

use std::collections::BTreeSet;

use midenc_frontend_wasm_metadata::namespace::WIT_KEYWORDS;

/// Convert a Miden identifier to kebab case: `receive_asset` → `receive-asset`, `NoteType` →
/// `note-type`, `PRIVATE` → `private`, `slot_1` → `slot-1`.
///
/// The result is not keyword-escaped, and need not be a valid WIT identifier (`_1` → `1`); see
/// [`ident`].
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
    // A separated digit word stays a word of its own (`slot_1` → `slot-1`), the spelling heck's
    // kebab case gives the Rust identifiers of Rust-built components, so a procedure gets the same
    // WIT name whichever toolchain built its component.
    segments.join("-")
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

/// The `%`-escaped namespace and name of the WIT package id `<namespace>:<name>@<version>` of a
/// package named `package_name` whose Miden namespace starts with `head`; or the reason one of
/// them has no WIT name.
///
/// The name is kebab-normalized and loses a leading `<head>-`, which the namespace part of the id
/// already says: `miden-standards-wallets-basic-wallet` → `miden:standards-wallets-basic-wallet`.
pub fn package_id(head: &str, package_name: &str) -> Result<(String, String), String> {
    let namespace = ident(head).map_err(|err| format!("the namespace {err}"))?;
    let head = kebab(head);
    let full = kebab(package_name);
    let name = full
        .strip_prefix(&head)
        .and_then(|rest| rest.strip_prefix('-'))
        .filter(|rest| !rest.is_empty())
        .unwrap_or(&full);
    let name = escape(name.to_owned());
    if !is_valid(&name) {
        return Err(format!(
            "the package name `{package_name}` has no WIT name (derived `{name}`)"
        ));
    }
    Ok((namespace, name))
}

/// Hands out the parameter names of one function, unique within it.
#[derive(Default)]
pub struct ParamNames {
    taken: BTreeSet<String>,
}

impl ParamNames {
    /// The name of parameter `index`: the kebab name of its named type when that is a valid WIT
    /// name, else `arg<index>`; a name already taken gets a counter (`asset`, `asset2`).
    pub fn next(&mut self, index: usize, type_name: Option<&str>) -> String {
        let base = type_name
            .and_then(|name| ident(short_name(name)).ok())
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
        ] {
            assert_eq!(kebab(from), to, "kebab({from})");
        }
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
    fn package_id_segments_are_escaped_and_checked() {
        assert_eq!(package_id("miden", "miden-list").unwrap(), ("miden".into(), "%list".into()));
        assert_eq!(package_id("use", "use-foo").unwrap(), ("%use".into(), "foo".into()));
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
    }
}
