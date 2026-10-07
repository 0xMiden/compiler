//! Rust names for MASM paths and identifiers.
//!
//! A MASM name is kept as it is wherever Rust allows, so a reader can find a binding by the name
//! the package uses. Three things change it: case follows Rust's conventions for modules, types,
//! enum variants and constants; a Rust keyword becomes a raw identifier (or, where Rust has no raw
//! form, gains a trailing `_`); and a character no Rust identifier may contain (a quoted MASM
//! name may contain any) becomes `_`.

use std::collections::{BTreeMap, btree_map::Entry};

use miden_assembly_syntax::ast::{Path, PathComponent};

use crate::Error;

/// Rust's strict and reserved keywords, in every edition the generated text may be compiled in.
const KEYWORDS: &[&str] = &[
    "as", "break", "const", "continue", "crate", "else", "enum", "extern", "false", "fn", "for",
    "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub", "ref", "return",
    "self", "Self", "static", "struct", "super", "trait", "true", "type", "unsafe", "use", "where",
    "while", "async", "await", "dyn", "abstract", "become", "box", "do", "final", "macro",
    "override", "priv", "typeof", "unsized", "virtual", "yield", "try", "gen",
];

/// The keywords that have no raw form: `r#self`, `r#Self`, `r#super` and `r#crate` are errors.
const NOT_RAW: &[&str] = &["self", "Self", "super", "crate"];

/// A module name: `snake_case`.
pub(crate) fn module_ident(segment: &str) -> String {
    keyword_safe(start_with_a_letter(snake_case(segment)))
}

/// A procedure, field or parameter name: the MASM name, made a valid identifier.
pub(crate) fn item_ident(name: &str) -> String {
    keyword_safe(sanitize(name))
}

/// A type name: `UpperCamelCase`.
///
/// The name is split on `_` and every piece is capitalized; a piece written entirely in capitals
/// is lowercased after its first letter (`FUNGIBLE` → `Fungible`), any other piece keeps the case
/// of its remaining letters, so a name that is already camel case stays as it is.
pub(crate) fn type_ident(name: &str) -> String {
    let sanitized = sanitize(name);
    let camel: String = sanitized.split('_').map(capitalize).collect();
    // A name made only of underscores has no pieces left; it is still a name.
    keyword_safe(if camel.is_empty() {
        sanitized
    } else {
        start_with_a_letter(camel)
    })
}

/// A constant name: `SCREAMING_SNAKE_CASE`.
pub(crate) fn const_ident(name: &str) -> String {
    keyword_safe(start_with_a_letter(words(&sanitize(name)).join("_").to_uppercase()))
}

/// An enum variant name: `UpperCamelCase`, as [`type_ident`] (`PRIVATE` → `Private`).
pub(crate) fn variant_ident(name: &str) -> String {
    type_ident(name)
}

/// A name in `snake_case` (`NoteType` → `note_type`), before [`item_ident`] makes it a valid
/// identifier: the base other names are built from.
pub(crate) fn snake_case(name: &str) -> String {
    words(&sanitize(name)).join("_").to_lowercase()
}

/// The name a wrapper's `extern "C"` declaration gives the procedure `name`: `__` and the name,
/// which no keyword is.
pub(crate) fn extern_ident(name: &str) -> String {
    format!("__{}", sanitize(name))
}

/// The symbol of the procedure at `path`, which its binding declares and its stub defines: the
/// path without its leading `::`, as the Wasm frontend looks it up.
pub(crate) fn link_name(path: &Path) -> String {
    path.to_relative().to_string()
}

/// The name of the stub that defines the procedure at `path`: its components joined by `__`.
pub(crate) fn stub_ident(path: &Path) -> Result<String, Error> {
    let segments: Vec<String> = segments(path)?.into_iter().map(sanitize).collect();
    Ok(segments.join("__"))
}

/// The first two of `items`, each paired with the Rust name it maps to, that map to the same
/// name, and that name; `None` if every name is different.
///
/// Rust names are not one-to-one with MASM names (`fooBar` and `foo_bar` are both the module
/// `foo_bar`), so every namespace the generated text fills is checked with this.
pub(crate) fn duplicate<T: Clone>(
    items: impl IntoIterator<Item = (T, String)>,
) -> Option<(T, T, String)> {
    let mut seen: BTreeMap<String, T> = BTreeMap::new();
    for (item, name) in items {
        match seen.entry(name) {
            Entry::Occupied(first) => {
                return Some((first.get().clone(), item, first.key().clone()));
            }
            Entry::Vacant(slot) => {
                slot.insert(item);
            }
        }
    }
    None
}

/// The absolute Rust path of the item at MASM `path`, when the bindings generated from `root` are
/// mounted at the Rust path `base`.
///
/// `root` is stripped from `path`; the remaining modules are mapped with [`module_ident`] and the
/// item's own name with [`item_ident`]. A `path` outside `root` is [`Error::OutsideRoot`].
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "no backend refers to an item by its absolute path yet"
    )
)]
pub(crate) fn rust_path(root: &str, path: &Path, base: &str) -> Result<String, Error> {
    absolute(root, path, base, item_ident)
}

/// As [`rust_path`], for a type: its own name is mapped with [`type_ident`].
pub(crate) fn type_path(root: &str, path: &Path, base: &str) -> Result<String, Error> {
    absolute(root, path, base, type_ident)
}

/// The Rust path of the type at MASM `path` as written inside the module `from`, both in the
/// bindings generated from `root`.
///
/// The path is relative — `super::` up to the nearest common module, then down — so the generated
/// text refers to its own types correctly wherever it is mounted. A type in `from` itself is its
/// bare name. A path down from `from` starts with `self::`, so a module that shares its name with
/// a crate (`core`, `alloc`) is never ambiguous.
pub(crate) fn relative_type_path(root: &str, from: &Path, path: &Path) -> Result<String, Error> {
    let from = under_root(root, from)?;
    let to = under_root(root, path)?;
    let Some((name, to)) = to.split_last() else {
        return Err(Error::Message(format!("`{path}` is the root `{root}`, not a type under it")));
    };
    Ok(relative(&from, to, &type_ident(name)))
}

/// As [`relative_type_path`], for the item whose Rust name is `ident` in the MASM module
/// `module`: a type the bindings declare themselves, which is named already.
pub(crate) fn relative_item_path(
    root: &str,
    from: &Path,
    module: &Path,
    ident: &str,
) -> Result<String, Error> {
    Ok(relative(&under_root(root, from)?, &under_root(root, module)?, ident))
}

/// The path of `ident` in the module whose segments below the root are `to`, written inside the
/// module whose segments are `from`.
fn relative(from: &[&str], to: &[&str], ident: &str) -> String {
    let common = from.iter().zip(to).take_while(|(a, b)| a == b).count();
    let mut parts: Vec<String> = vec!["super".to_string(); from.len() - common];
    if parts.is_empty() && to.len() > common {
        parts.push("self".to_string());
    }
    parts.extend(to[common..].iter().map(|segment| module_ident(segment)));
    parts.push(ident.to_string());
    parts.join("::")
}

/// `base` followed by the mapped components of `path` below `root`.
fn absolute(
    root: &str,
    path: &Path,
    base: &str,
    last: fn(&str) -> String,
) -> Result<String, Error> {
    let segments = under_root(root, path)?;
    let mut parts: Vec<String> = Vec::with_capacity(segments.len() + 1);
    if !base.is_empty() {
        parts.push(base.to_string());
    }
    if let Some((name, modules)) = segments.split_last() {
        parts.extend(modules.iter().map(|segment| module_ident(segment)));
        parts.push(last(name));
    }
    Ok(parts.join("::"))
}

/// Whether `path` is a module above `root`, one of its strict ancestors: `::miden` above
/// `::miden::core`. Such a module is only the way down to the root and has no place in the
/// generated module tree. A leading `::` on either is ignored.
pub(crate) fn is_above_root(root: &str, path: &Path) -> Result<bool, Error> {
    let root = segments(Path::new(root))?;
    let path = segments(path)?;
    Ok(path.len() < root.len() && path.iter().zip(&root).all(|(p, r)| p == r))
}

/// The components of `path` below `root`, or [`Error::OutsideRoot`].
///
/// A leading `::` on either is ignored: manifest paths are absolute, and a root is accepted with
/// or without it.
pub(crate) fn under_root<'p>(root: &str, path: &'p Path) -> Result<Vec<&'p str>, Error> {
    let root_segments = segments(Path::new(root))?;
    let mut path_segments = segments(path)?;
    let under = path_segments.len() >= root_segments.len()
        && path_segments.iter().zip(&root_segments).all(|(p, r)| p == r);
    if !under {
        return Err(Error::OutsideRoot {
            path: path.to_string(),
            root: root.to_string(),
        });
    }
    path_segments.drain(..root_segments.len());
    Ok(path_segments)
}

/// The named components of `path`, unquoted, without the root anchor.
fn segments(path: &Path) -> Result<Vec<&str>, Error> {
    if path.is_empty() {
        return Ok(Vec::new());
    }
    path.components()
        .filter_map(|component| match component {
            Ok(PathComponent::Root) => None,
            Ok(component) => Some(Ok(component.as_str())),
            Err(err) => Some(Err(Error::Message(format!("`{path}` is not a valid path: {err}")))),
        })
        .collect()
}

/// The words of a name: its `_`-separated pieces, each split again where its case changes from
/// lower to upper (`maxAmount` → `max`, `Amount`) or where an acronym ends (`HTTPServer` →
/// `HTTP`, `Server`).
fn words(name: &str) -> Vec<String> {
    let mut words = Vec::new();
    for piece in name.split('_').filter(|piece| !piece.is_empty()) {
        let chars: Vec<char> = piece.chars().collect();
        let mut word = String::new();
        for (i, &c) in chars.iter().enumerate() {
            let boundary = i > 0 && c.is_uppercase() && {
                let previous = chars[i - 1];
                let next_is_lower = chars.get(i + 1).is_some_and(|next| next.is_lowercase());
                previous.is_lowercase()
                    || previous.is_ascii_digit()
                    || (previous.is_uppercase() && next_is_lower)
            };
            if boundary {
                words.push(core::mem::take(&mut word));
            }
            word.push(c);
        }
        words.push(word);
    }
    if words.is_empty() {
        // A name made only of underscores: keep it rather than produce an empty identifier.
        words.push(name.to_string());
    }
    words
}

/// `piece` with its first letter in upper case; the rest lowercased if `piece` has no lowercase
/// letter (`PRIVATE` → `Private`), kept otherwise (`StorageSlotId` stays).
fn capitalize(piece: &str) -> String {
    let mut chars = piece.chars();
    let Some(first) = chars.next() else {
        return String::new();
    };
    let rest = chars.as_str();
    let rest = if rest.chars().any(char::is_lowercase) {
        rest.to_string()
    } else {
        rest.to_lowercase()
    };
    first.to_uppercase().chain(rest.chars()).collect()
}

/// `name` with every character no Rust identifier may contain replaced by `_`, and a leading `_`
/// added when it would otherwise start with a digit or be empty.
fn sanitize(name: &str) -> String {
    let name: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    start_with_a_letter(name)
}

/// `name`, prefixed with `_` if it is empty or starts with a digit.
fn start_with_a_letter(name: String) -> String {
    if name.chars().next().is_none_or(|c| c.is_ascii_digit()) {
        format!("_{name}")
    } else {
        name
    }
}

/// `ident` as a raw identifier if it is a keyword, with a trailing `_` if it is a keyword that has
/// no raw form, or as it is.
///
/// `_` alone is a pattern, not an identifier, so it becomes `__`.
fn keyword_safe(ident: String) -> String {
    if NOT_RAW.contains(&ident.as_str()) || ident == "_" {
        format!("{ident}_")
    } else if KEYWORDS.contains(&ident.as_str()) {
        format!("r#{ident}")
    } else {
        ident
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idents_follow_rust_conventions() {
        assert_eq!(module_ident("active_account"), "active_account");
        assert_eq!(module_ident("type"), "r#type");
        assert_eq!(item_ident("match"), "r#match");
        assert_eq!(item_ident("get_id"), "get_id");
        assert_eq!(type_ident("StorageSlotId"), "StorageSlotId");
        assert_eq!(type_ident("note_type"), "NoteType");
        assert_eq!(const_ident("ASSET_SIZE"), "ASSET_SIZE");
        assert_eq!(const_ident("max_amount"), "MAX_AMOUNT");
        assert_eq!(variant_ident("PRIVATE"), "Private");
        assert_eq!(variant_ident("FUNGIBLE_ASSET"), "FungibleAsset");
    }

    #[test]
    fn case_conversion_keeps_digits_and_finds_camel_case_words() {
        assert_eq!(module_ident("falcon512_poseidon2"), "falcon512_poseidon2");
        assert_eq!(module_ident("SortedArray"), "sorted_array");
        assert_eq!(const_ident("maxAmount"), "MAX_AMOUNT");
        assert_eq!(const_ident("HTTPServer"), "HTTP_SERVER");
        assert_eq!(const_ident("U256_MAX"), "U256_MAX");
        assert_eq!(type_ident("BigEndianUint256"), "BigEndianUint256");
        assert_eq!(type_ident("u256"), "U256");
        assert_eq!(type_ident("NONE"), "None");
        assert_eq!(item_ident("diff_mod_M"), "diff_mod_M", "an item keeps its case");
    }

    #[test]
    fn keywords_without_a_raw_form_gain_a_trailing_underscore() {
        assert_eq!(item_ident("self"), "self_");
        assert_eq!(module_ident("super"), "super_");
        assert_eq!(module_ident("crate"), "crate_");
        assert_eq!(type_ident("self"), "Self_", "`self` camel-cased is the keyword `Self`");
        assert_eq!(item_ident("gen"), "r#gen");
        assert_eq!(item_ident("_"), "__");
    }

    #[test]
    fn characters_rust_does_not_allow_become_underscores() {
        // A quoted MASM name may contain any character.
        assert_eq!(item_ident("hello-world"), "hello_world");
        assert_eq!(item_ident("$kernel"), "_kernel");
        assert_eq!(item_ident("1st"), "_1st");
        assert_eq!(type_ident("note-type"), "NoteType");
        assert_eq!(const_ident("a.b"), "A_B");
    }

    /// Splitting a name into words drops the `_` a leading digit needs; every kind of name keeps
    /// it.
    #[test]
    fn a_leading_digit_keeps_its_underscore_in_every_case() {
        assert_eq!(module_ident("1st"), "_1st");
        assert_eq!(module_ident("2nd_Pass"), "_2nd_pass");
        assert_eq!(const_ident("1st"), "_1ST");
        assert_eq!(type_ident("1st"), "_1st");
        assert_eq!(item_ident("1st"), "_1st");
        assert_eq!(
            relative_type_path("::p", Path::new("::p"), Path::new("::p::1a::T")).unwrap(),
            "self::_1a::T"
        );
    }

    /// Two MASM names of one Rust name are found, whatever lies between them.
    #[test]
    fn duplicates_are_the_first_two_items_of_one_rust_name() {
        let modules = ["fooBar", "other", "foo_bar", "FooBar"];
        let mapped = modules.map(|name| (name, module_ident(name)));
        assert_eq!(duplicate(mapped), Some(("fooBar", "foo_bar", "foo_bar".to_string())));
        let constants = ["MAX", "max_amount", "min"].map(|name| (name, const_ident(name)));
        assert_eq!(duplicate(constants), None);
        // Stub names join the path with `__`, which a component may contain too.
        let stubs = ["::p::a::b__c", "::p::a__b::c"]
            .map(|path| (path, stub_ident(Path::new(path)).unwrap()));
        assert_eq!(
            duplicate(stubs),
            Some(("::p::a::b__c", "::p::a__b::c", "p__a__b__c".to_string()))
        );
    }

    #[test]
    fn rust_paths_strip_the_root_and_map_every_segment() {
        let path = Path::new("::miden::protocol::active_account::get_id");
        assert_eq!(
            rust_path("::miden::protocol", path, "crate::raw::protocol").unwrap(),
            "crate::raw::protocol::active_account::get_id"
        );
        let outside = Path::new("::miden::core::mem::pipe_words_to_memory");
        assert!(matches!(
            rust_path("::miden::protocol", outside, "crate::raw::protocol"),
            Err(Error::OutsideRoot { .. })
        ));
    }

    #[test]
    fn root_matching_is_by_whole_segment_and_ignores_the_leading_separator() {
        let path = Path::new("::miden::protocolx::f");
        assert!(matches!(
            rust_path("::miden::protocol", path, "crate"),
            Err(Error::OutsideRoot { .. })
        ));
        let path = Path::new("::miden::protocol::type::f");
        assert_eq!(rust_path("miden::protocol", path, "crate").unwrap(), "crate::r#type::f");
        assert_eq!(
            type_path("::miden::protocol", Path::new("::miden::protocol::types::note_type"), "p")
                .unwrap(),
            "p::types::NoteType"
        );
        assert_eq!(rust_path("", Path::new("::a::b"), "").unwrap(), "a::b", "an empty root");
    }

    #[test]
    fn procedures_have_an_extern_name_a_symbol_and_a_stub_name() {
        assert_eq!(snake_case("NoteType"), "note_type");
        assert_eq!(snake_case("StorageSlotId"), "storage_slot_id");
        assert_eq!(snake_case("Type"), "type", "the base of `r#type`, which `item_ident` makes");
        assert_eq!(extern_ident("match"), "__match", "no keyword starts with `__`");
        let path = Path::new("::miden::core::mem::pipe_words_to_memory");
        assert_eq!(link_name(path), "miden::core::mem::pipe_words_to_memory");
        assert_eq!(stub_ident(path).unwrap(), "miden__core__mem__pipe_words_to_memory");
    }

    #[test]
    fn own_types_are_referred_to_relative_to_the_referring_module() {
        let root = "::fixture";
        let pair = Path::new("::fixture::Pair");
        assert_eq!(relative_type_path(root, Path::new("::fixture"), pair).unwrap(), "Pair");
        assert_eq!(
            relative_type_path(root, Path::new("::fixture::nested"), pair).unwrap(),
            "super::Pair"
        );
        let deep = Path::new("::fixture::types::core::Id");
        assert_eq!(
            relative_type_path(root, Path::new("::fixture"), deep).unwrap(),
            "self::types::core::Id",
            "a path down starts with `self::`, so a module named `core` is not the crate"
        );
        assert_eq!(
            relative_type_path(root, Path::new("::fixture::a::b"), deep).unwrap(),
            "super::super::types::core::Id"
        );
        assert_eq!(
            relative_type_path(root, Path::new("::fixture::types::core"), deep).unwrap(),
            "Id"
        );
        assert!(matches!(
            relative_type_path(root, Path::new("::other"), pair),
            Err(Error::OutsideRoot { .. })
        ));
    }
}
