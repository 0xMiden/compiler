use alloc::{borrow::Cow, string::String};
use core::fmt;

use super::quoting::{push_quoted, unescape_quoted};
use crate::{
    FunctionIdent, SmallVec, SymbolName,
    diagnostics::{Diagnostic, miette},
    interner, smallvec,
};

#[derive(Debug, thiserror::Error, Diagnostic)]
pub enum InvalidSymbolPathError {
    #[error("invalid symbol path: cannot be empty")]
    Empty,
    #[error("invalid symbol path: unexpected components found after leaf")]
    UnexpectedTrailingComponents,
    #[error("invalid symbol path: only one root component is allowed, and it must come first")]
    UnexpectedRootPlacement,
}

/// This type is a custom [crate::Attribute] for [super::Symbol] references.
///
/// A [SymbolPath] is represented much like a filesystem path, i.e. as a vector of components.
/// Each component refers to a distinct `Symbol` that must be resolvable, the details of which
/// depends on what style of path is used.
///
/// Similar to filesystem paths, there are two types of paths supported:
///
/// * Unrooted (i.e. relative) paths. These are resolved from the nearest parent `SymbolTable`,
///   and must terminate with `SymbolNameComponent::Leaf`.
/// * Absolute paths. The resolution rules for these depends on what the top-level operation is
///   as reachable from the containing operation, described in more detail below. These paths
///   must begin with `SymbolNameComponent::Root`.
///
/// NOTE: There is no equivalent of the `.` or `..` nodes in a filesystem path in symbol paths,
/// at least at the moment. Thus there is no way to refer to symbols some arbitrary number of
/// parents above the current `SymbolTable`, they must be resolved to absolute paths by the
/// frontend for now.
///
/// # Symbol Resolution
///
/// Relative paths, as mentioned above, are resolved from the nearest parent `SymbolTable`; if
/// no `SymbolTable` is present, an error will be raised.
///
/// Absolute paths are relatively simple, but supports two use cases, based on the _top-level_
/// operation reachable from the current operation, i.e. the operation at the top of the
/// ancestor tree which has no parent:
///
/// 1. If the top-level operation is an anonymous `SymbolTable` (i.e. it is not also a `Symbol`),
///    then that `SymbolTable` corresponds to the global (root) namespace, and symbols are
///    resolved recursively from there.
/// 2. If the top-level operation is a named `SymbolTable` (i.e. it is also a `Symbol`), then it
///    is presumed that the top-level operation is defined in the global (root) namespace, even
///    though we are unable to reach the global namespace directly. Thus, the symbol we're
///    trying to resolve _must_ be a descendant of the top-level operation. This implies that
///    the symbol path of the top-level operation must be a prefix of `path`.
///
/// We support the second style to allow for working with more localized chunks of IR, when no
/// symbol references escape the top-level `SymbolTable`. This is mostly useful in testing
/// scenarios.
///
/// Symbol resolution of absolute paths will fail if:
///
/// * The top-level operation is not a `SymbolTable`
/// * The top-level operation is a `Symbol` whose path is not a prefix of `path`
/// * We are unable to resolve any component of the path, starting from the top-level
/// * Any intermediate symbol in the path refers to a `Symbol` which is not also a `SymbolTable`
#[derive(Clone)]
pub struct SymbolPath {
    /// The underlying components of the symbol name (alternatively called the symbol path).
    pub path: SmallVec<[SymbolNameComponent; 3]>,
}

impl FromIterator<SymbolNameComponent> for SymbolPath {
    fn from_iter<I>(iter: I) -> Self
    where
        I: IntoIterator<Item = SymbolNameComponent>,
    {
        Self {
            path: SmallVec::from_iter(iter),
        }
    }
}

impl SymbolPath {
    pub fn new<I>(components: I) -> Result<Self, InvalidSymbolPathError>
    where
        I: IntoIterator<Item = SymbolNameComponent>,
    {
        let mut path = SmallVec::default();

        let mut components = components.into_iter();

        match components.next() {
            None => return Err(InvalidSymbolPathError::Empty),
            Some(component @ (SymbolNameComponent::Root | SymbolNameComponent::Component(_))) => {
                path.push(component);
            }
            Some(component @ SymbolNameComponent::Leaf(_)) => {
                if components.next().is_some() {
                    return Err(InvalidSymbolPathError::UnexpectedTrailingComponents);
                }
                path.push(component);
                return Ok(Self { path });
            }
        };

        while let Some(component) = components.next() {
            match component {
                SymbolNameComponent::Root => {
                    return Err(InvalidSymbolPathError::UnexpectedRootPlacement);
                }
                component @ SymbolNameComponent::Component(_) => {
                    path.push(component);
                }
                component @ SymbolNameComponent::Leaf(_) => {
                    path.push(component);
                    if components.next().is_some() {
                        return Err(InvalidSymbolPathError::UnexpectedTrailingComponents);
                    }
                }
            }
        }

        Ok(Self { path })
    }

    /// Converts a [FunctionIdent] representing a fully-qualified Miden Assembly procedure path,
    /// to it's equivalent [SymbolPath] representation.
    ///
    /// # Example
    ///
    /// ```rust
    /// use midenc_hir::{SymbolPath, SymbolNameComponent, FunctionIdent};
    ///
    /// let id = FunctionIdent {
    ///     module: "intrinsics::mem".into(),
    ///     function: "load_felt_unchecked".into(),
    /// };
    /// assert_eq!(
    ///     SymbolPath::from_masm_function_id(id),
    ///     SymbolPath::from_iter([
    ///         SymbolNameComponent::Root,
    ///         SymbolNameComponent::Component("intrinsics".into()),
    ///         SymbolNameComponent::Component("mem".into()),
    ///         SymbolNameComponent::Leaf("load_felt_unchecked".into()),
    ///     ])
    /// );
    /// ```
    pub fn from_masm_function_id(id: FunctionIdent) -> Self {
        let mut path = Self::from_masm_module_id(id.module.as_str());
        path.path.push(SymbolNameComponent::Leaf(id.function.as_symbol()));
        path
    }

    /// Converts a [str] representing a fully-qualified Miden Assembly module path, to it's
    /// equivalent [SymbolPath] representation.
    ///
    /// The id is split on every `::`, so this is meant for ids whose segments contain none; a
    /// segment is kept as written, quotes included. A path with a quoted segment containing `::`
    /// (`::"acme::pkg"::app`) must go through [SymbolPath::from_library_path] instead.
    ///
    /// # Example
    ///
    /// ```rust
    /// use midenc_hir::{SymbolPath, SymbolNameComponent};
    ///
    /// assert_eq!(
    ///     SymbolPath::from_masm_module_id("intrinsics::mem"),
    ///     SymbolPath::from_iter([
    ///         SymbolNameComponent::Root,
    ///         SymbolNameComponent::Component("intrinsics".into()),
    ///         SymbolNameComponent::Component("mem".into()),
    ///     ])
    /// );
    /// ```
    pub fn from_masm_module_id(id: &str) -> Self {
        let parts = id.split("::");
        Self::from_iter(
            core::iter::once(SymbolNameComponent::Root)
                .chain(parts.map(SymbolName::intern).map(SymbolNameComponent::Component)),
        )
    }

    /// Returns the segments of the name of a symbol-table op, the inverse of
    /// [SymbolPath::to_symbol_name].
    ///
    /// A component may be *named* by a multi-segment path; such a name occupies one entry in its
    /// parent's symbol table, but contributes one [SymbolNameComponent::Component] per segment to
    /// every [SymbolPath] that passes through it. The name is the canonical key of the path:
    ///
    /// * the segments are joined with `::`;
    /// * in a multi-segment key, a segment containing `:` or starting with `"` is written quoted,
    ///   `"..."`, with `"` and `\` escaped by a `\`; any other segment is written raw;
    /// * a single-segment key is written quoted only when it contains `::`.
    ///
    /// So `["acme::pkg", "app"]` is keyed `"acme::pkg"::app`, and a name without `::` is a single
    /// segment, as written. Module names cannot contain `::`, so they are always one segment. Leaf
    /// symbol names (functions, globals) are opaque and must never be split with this function.
    pub fn segments_of(name: SymbolName) -> SmallVec<[SymbolName; 3]> {
        let s = name.as_str();
        if !s.contains("::") {
            return smallvec![name];
        }
        split_key(s)
    }

    /// Returns true when the name `inner` nests in the name `outer`, i.e. when `outer` is a strict
    /// segment-prefix of `inner`: `a::bc` nests in `a` but not in `a::b`, and no name nests in
    /// itself. Both are canonical keys (see [SymbolPath::segments_of]) and are compared segment by
    /// segment: `"acme::pkg"::app` nests in `"acme::pkg"`, not in `acme`.
    pub fn nests_in(inner: SymbolName, outer: SymbolName) -> bool {
        let (inner, outer) = (Self::segments_of(inner), Self::segments_of(outer));
        inner.len() > outer.len() && inner.starts_with(&outer)
    }

    /// Returns the canonical key of all non-root components of this path (leaf included).
    ///
    /// This is the symbol-table key of a component named by this path, and the inverse of
    /// [SymbolPath::segments_of], which describes the spelling.
    pub fn to_symbol_name(&self) -> SymbolName {
        let components = self
            .path
            .iter()
            .filter(|c| !c.is_root())
            .copied()
            .collect::<SmallVec<[SymbolNameComponent; 4]>>();
        Self::join_components(&components)
    }

    /// Returns the canonical key of the names of `components` (see [SymbolPath::segments_of]).
    pub(crate) fn join_components(components: &[SymbolNameComponent]) -> SymbolName {
        match components {
            // `segments_of` reads a key without `::` as one segment, as written, so a lone segment
            // is quoted only when it contains `::`, even when it starts with `"`. A raw lone key
            // never contains `::` and a multi-segment key always does, so the two cannot collide.
            [single] => {
                let name = single.as_symbol_name();
                if !name.as_str().contains("::") {
                    return name;
                }
                let mut quoted = String::new();
                push_quoted(&mut quoted, name.as_str());
                SymbolName::intern(quoted)
            }
            _ => {
                let mut joined = String::new();
                for (i, component) in components.iter().enumerate() {
                    if i > 0 {
                        joined.push_str("::");
                    }
                    let segment = component.as_symbol_name();
                    let segment = segment.as_str();
                    // A raw segment never contains `:`, so every `::` outside quotes is a
                    // boundary and distinct segment lists give distinct keys (`a:` + `b` would
                    // otherwise read back as `a` + `:b`). A raw segment starting with `"` would
                    // be read back as a quoted one.
                    if segment.contains(':') || segment.starts_with('"') {
                        push_quoted(&mut joined, segment);
                    } else {
                        joined.push_str(segment);
                    }
                }
                SymbolName::intern(joined)
            }
        }
    }

    /// Returns the leaf component of the symbol path
    pub fn name(&self) -> SymbolName {
        match self.path.last().expect("expected non-empty symbol path") {
            SymbolNameComponent::Leaf(name) => *name,
            component => panic!("invalid symbol path: expected leaf node, got: {component:?}"),
        }
    }

    /// Set the value of the leaf component of the path, or append it if not yet present
    pub fn set_name(&mut self, name: SymbolName) {
        match self.path.last_mut() {
            Some(SymbolNameComponent::Leaf(prev_name)) => {
                *prev_name = name;
            }
            _ => {
                self.path.push(SymbolNameComponent::Leaf(name));
            }
        }
    }

    /// Returns the first non-root component of the symbol path, if the path is absolute
    pub fn namespace(&self) -> Option<SymbolName> {
        if self.is_absolute() {
            match self.path[1] {
                SymbolNameComponent::Component(ns) => Some(ns),
                SymbolNameComponent::Leaf(_) => None,
                SymbolNameComponent::Root => unreachable!(
                    "malformed symbol path: root components may only occur at the start of a path"
                ),
            }
        } else {
            None
        }
    }

    /// Derive a Miden Assembly `LibraryPath` from this symbol path
    pub fn to_library_path(&self) -> midenc_session::LibraryPath {
        use midenc_session::LibraryPath;

        let components = self.path.iter();
        let mut path = LibraryPath::default();
        for component in components {
            if component.is_root() {
                path.push_component("::");
                continue;
            } else {
                path.push_component(component.as_symbol_name().as_str());
            }
        }

        path
    }

    /// Derive a symbol path from the Miden Assembly path `path`, the inverse of
    /// [SymbolPath::to_library_path]: one [SymbolNameComponent::Component] per segment of the
    /// path, rooted when the path is absolute.
    ///
    /// A quoted segment stays one component. Segments that fail to parse are skipped; a valid
    /// path has none.
    pub fn from_library_path(path: &midenc_session::miden_assembly_syntax::Path) -> SymbolPath {
        use midenc_session::miden_assembly_syntax::PathComponent;

        path.components()
            .filter_map(|component| component.ok())
            .map(|component| match component {
                PathComponent::Root => SymbolNameComponent::Root,
                component => SymbolNameComponent::Component(SymbolName::intern(component.as_str())),
            })
            .collect()
    }

    /// Returns true if this symbol name is fully-qualified
    pub fn is_absolute(&self) -> bool {
        matches!(&self.path[0], SymbolNameComponent::Root)
    }

    /// Returns true if this symbol name is nested
    pub fn has_parent(&self) -> bool {
        if self.is_absolute() {
            self.path.len() > 2
        } else {
            self.path.len() > 1
        }
    }

    /// Returns true if `self` is a prefix of `other`, i.e. `other` is a further qualified symbol
    /// reference.
    ///
    /// NOTE: If `self` and `other` are equal, `self` is considered a prefix. The caller should
    /// check if the two references are identical if they wish to distinguish the two cases.
    pub fn is_prefix_of(&self, other: &Self) -> bool {
        other.is_prefixed_by(&self.path)
    }

    /// Returns true if `prefix` is a prefix of `self`, i.e. `self` is a further qualified symbol
    /// reference.
    ///
    /// NOTE: If `self` and `prefix` are equal, `prefix` is considered a valid prefix. The caller
    /// should check if the two references are identical if they wish to distinguish the two cases.
    pub fn is_prefixed_by(&self, prefix: &[SymbolNameComponent]) -> bool {
        let mut a = prefix.iter();
        let mut b = self.path.iter();

        let mut index = 0;
        loop {
            match (a.next(), b.next()) {
                (Some(part_a), Some(part_b)) if part_a == part_b => {
                    index += 1;
                }
                (None, Some(_)) => break index > 0,
                _ => break false,
            }
        }
    }

    /// Returns an iterator over the path components of this symbol name
    pub fn components(&self) -> impl ExactSizeIterator<Item = SymbolNameComponent> + '_ {
        self.path.iter().copied()
    }

    /// Get the parent of this path, i.e. all but the last component
    pub fn parent(&self) -> Option<SymbolPath> {
        match self.path.split_last()? {
            (SymbolNameComponent::Root, []) => None,
            (_, rest) => Some(SymbolPath {
                path: SmallVec::from_slice(rest),
            }),
        }
    }

    /// Get the portion of this path without the `Leaf` component, if present.
    pub fn without_leaf(&self) -> Cow<'_, SymbolPath> {
        match self.path.split_last() {
            Some((SymbolNameComponent::Leaf(_), rest)) => Cow::Owned(SymbolPath {
                path: SmallVec::from_slice(rest),
            }),
            _ => Cow::Borrowed(self),
        }
    }
}

/// Prints the path as its segments joined with `::`, with a leading `::` when the path is
/// absolute. A component segment that contains `::` is written in the quoted form of a canonical
/// key (see [SymbolPath::segments_of]), e.g. `::"acme::pkg"::app`; a leaf is written as is. For
/// the Miden Assembly spelling use [SymbolPath::to_library_path].
impl fmt::Display for SymbolPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut components = self.path.iter();

        if self.is_absolute() {
            let _ = components.next();
            f.write_str("::")?;
        }

        match components.next() {
            Some(component) => write_segment(f, component)?,
            None => return Ok(()),
        }
        for component in components {
            f.write_str("::")?;
            write_segment(f, component)?;
        }
        Ok(())
    }
}

/// Writes `component` for [SymbolPath]'s `Display`: a component segment containing `::` quoted,
/// anything else (a leaf in particular, which is opaque and never split) as is.
fn write_segment(f: &mut fmt::Formatter<'_>, component: &SymbolNameComponent) -> fmt::Result {
    match component {
        SymbolNameComponent::Component(segment) if segment.as_str().contains("::") => {
            let mut quoted = String::new();
            push_quoted(&mut quoted, segment.as_str());
            f.write_str(&quoted)
        }
        component => f.write_str(component.as_symbol_name().as_str()),
    }
}

/// Splits the canonical key `key` into its segments (see [SymbolPath::segments_of]).
///
/// A segment starting with `"` runs to the next unescaped `"`, which must be followed by `::` or
/// the end of the key; any other segment, or a quoted one that does not close that way, runs to
/// the next `::` or the end of the key.
fn split_key(key: &str) -> SmallVec<[SymbolName; 3]> {
    let mut segments = SmallVec::new();
    let mut rest = key;
    loop {
        let (segment, tail) = match rest.strip_prefix('"').and_then(unquote_segment) {
            Some((segment, tail)) => (SymbolName::intern(segment), tail),
            None => match rest.split_once("::") {
                Some((segment, tail)) => (SymbolName::intern(segment), Some(tail)),
                None => (SymbolName::intern(rest), None),
            },
        };
        segments.push(segment);
        match tail {
            Some(tail) => rest = tail,
            None => return segments,
        }
    }
}

/// Reads the quoted segment whose opening `"` precedes `quoted` (see [unescape_quoted]).
///
/// Returns the segment and the text after the `::` that follows its closing `"`, or `None` for
/// that text when the key ends there; `None` when the segment does not close before `::` or the
/// end of the key.
fn unquote_segment(quoted: &str) -> Option<(String, Option<&str>)> {
    let (segment, after) = unescape_quoted(quoted)?;
    if after.is_empty() {
        Some((segment, None))
    } else {
        Some((segment, Some(after.strip_prefix("::")?)))
    }
}

impl fmt::Debug for SymbolPath {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("SymbolPath")
            .field_with("path", |f| f.debug_list().entries(self.path.iter()).finish())
            .finish()
    }
}
impl crate::formatter::PrettyPrint for SymbolPath {
    fn render(&self) -> crate::formatter::Document {
        use crate::formatter::*;
        display(self)
    }
}
impl Eq for SymbolPath {}
impl PartialEq for SymbolPath {
    fn eq(&self, other: &Self) -> bool {
        self.path == other.path
    }
}
impl PartialOrd for SymbolPath {
    #[inline]
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for SymbolPath {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        self.path.cmp(&other.path)
    }
}
impl core::hash::Hash for SymbolPath {
    fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
        self.path.hash(state);
    }
}

/// A component of a namespaced [SymbolName].
///
/// A component refers to one of the following:
///
/// * The root/global namespace anchor, i.e. indicates that other components are to be resolved
///   relative to the root (possibly anonymous) symbol table.
/// * The name of a symbol table nested within another symbol table or root namespace
/// * The name of a symbol (which must always be the leaf component of a path)
#[derive(Copy, Clone, PartialEq, Eq, Hash)]
pub enum SymbolNameComponent {
    /// A component that signals the path is relative to the root symbol table
    Root,
    /// A component of the symbol name path
    Component(SymbolName),
    /// The name of the symbol in its local symbol table
    Leaf(SymbolName),
}

impl SymbolNameComponent {
    pub fn as_symbol_name(&self) -> SymbolName {
        match self {
            Self::Root => interner::symbols::Empty,
            Self::Component(name) | Self::Leaf(name) => *name,
        }
    }

    #[inline]
    pub fn is_root(&self) -> bool {
        matches!(self, Self::Root)
    }

    #[inline]
    pub fn is_leaf(&self) -> bool {
        matches!(self, Self::Leaf(_))
    }
}

impl fmt::Debug for SymbolNameComponent {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Root => f.write_str("Root"),
            Self::Component(name) => {
                f.debug_tuple("Component").field_with(|f| f.write_str(name.as_str())).finish()
            }
            Self::Leaf(name) => {
                f.debug_tuple("Leaf").field_with(|f| f.write_str(name.as_str())).finish()
            }
        }
    }
}

impl Ord for SymbolNameComponent {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        use core::cmp::Ordering;

        if self == other {
            return Ordering::Equal;
        }

        match (self, other) {
            (Self::Root, _) => Ordering::Less,
            (_, Self::Root) => Ordering::Greater,
            (Self::Component(x), Self::Component(y)) => x.cmp(y),
            (Self::Component(_), _) => Ordering::Less,
            (_, Self::Component(_)) => Ordering::Greater,
            (Self::Leaf(x), Self::Leaf(y)) => x.cmp(y),
        }
    }
}

impl PartialOrd for SymbolNameComponent {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

#[cfg(test)]
mod tests {
    use alloc::string::ToString;

    use super::*;

    #[test]
    fn from_library_path_inverts_to_library_path() {
        use midenc_session::miden_assembly_syntax::Path;

        let path = SymbolPath::from_library_path(Path::new("::miden::a::b"));
        assert_eq!(path, SymbolPath::from_masm_module_id("miden::a::b"));
        assert_eq!(path.to_library_path().to_string(), "::miden::a::b");

        let relative = SymbolPath::from_library_path(Path::new("miden::a"));
        assert!(!relative.is_absolute());
        assert_eq!(relative.to_string(), "miden::a");
    }

    #[test]
    fn nests_in_requires_a_strict_segment_prefix() {
        let nests = |inner: &str, outer: &str| {
            SymbolPath::nests_in(SymbolName::intern(inner), SymbolName::intern(outer))
        };
        assert!(nests("a::b", "a"));
        assert!(nests("a::b::c", "a::b"));
        assert!(!nests("a::b", "a::b"));
        assert!(!nests("a", "a::b"));
        assert!(!nests("a::bc", "a::b"));
        assert!(!nests("ab", "a"));
        assert!(nests("\"acme::pkg\"::app", "\"acme::pkg\""));
        assert!(!nests("\"acme::pkg\"::app", "acme"));
        // A lone segment starting with `"` is keyed raw but quoted once another segment follows
        assert!(nests(r#""\"a\""::b"#, r#""a""#));
        assert!(!nests(r#""a::b"::c"#, r#""a"#));
    }

    /// Joins `segments` into a canonical key and splits it back.
    fn round_trip(segments: &[&str]) -> (SymbolName, alloc::vec::Vec<String>) {
        let components = segments
            .iter()
            .map(|segment| SymbolNameComponent::Component(SymbolName::intern(segment)))
            .collect::<SmallVec<[SymbolNameComponent; 4]>>();
        let key = SymbolPath::join_components(&components);
        let split = SymbolPath::segments_of(key).iter().map(|s| s.as_str().to_string()).collect();
        (key, split)
    }

    #[test]
    fn canonical_keys_round_trip() {
        for (segments, key) in [
            (&["app"][..], "app"),
            (&["miden", "a", "b"][..], "miden::a::b"),
            (&["acme::pkg", "app"][..], r#""acme::pkg"::app"#),
            (&["acme::pkg"][..], r#""acme::pkg""#),
            (&["::"][..], r#""::""#),
            (&["a", "::", "b"][..], r#"a::"::"::b"#),
            (&[r#"q"u\o"#, "x"][..], r#"q"u\o::x"#),
            (&[r#""masm-dep""#][..], r#""masm-dep""#),
            (&[r#""masm-dep""#, "x"][..], r#""\"masm-dep\""::x"#),
            (&[r#"a::"b\"#, "c"][..], r#""a::\"b\\"::c"#),
            (&["a:", "b::c"][..], r#""a:"::"b::c""#),
            (&["a", ":b"][..], r#"a::":b""#),
            (&["a:", "b"][..], r#""a:"::b"#),
        ] {
            let (joined, split) = round_trip(segments);
            assert_eq!(joined.as_str(), key, "the key of {segments:?}");
            assert_eq!(split, segments, "the segments of `{key}`");
        }
    }

    /// A `:` next to a segment boundary does not merge with the `::` separator.
    #[test]
    fn distinct_segment_lists_give_distinct_keys() {
        let (left, left_split) = round_trip(&["a:", "b"]);
        let (right, right_split) = round_trip(&["a", ":b"]);
        assert_ne!(left, right);
        assert_eq!(left_split, ["a:", "b"]);
        assert_eq!(right_split, ["a", ":b"]);
    }

    /// A component segment containing `::` prints quoted; a leaf is opaque and prints raw.
    #[test]
    fn display_quotes_only_component_segments() {
        let path = SymbolPath::from_iter([
            SymbolNameComponent::Root,
            SymbolNameComponent::Component(SymbolName::intern("acme::pkg")),
            SymbolNameComponent::Leaf(SymbolName::intern("core::ptr::drop_in_place<T>")),
        ]);
        assert_eq!(path.to_string(), r#"::"acme::pkg"::core::ptr::drop_in_place<T>"#);
    }

    #[test]
    fn segments_of_an_empty_path_and_a_single_segment() {
        assert_eq!(SymbolPath::join_components(&[]).as_str(), "");
        assert_eq!(
            SymbolPath::segments_of(SymbolName::intern("")).as_slice(),
            &[SymbolName::intern("")]
        );
        assert_eq!(
            SymbolPath::segments_of(SymbolName::intern("app")).as_slice(),
            &[SymbolName::intern("app")]
        );
    }

    /// A key that is not canonical is still split: a quote that does not close a segment before
    /// `::` or the end leaves the segment raw.
    #[test]
    fn a_malformed_quoted_segment_is_read_raw() {
        let segments = |key: &str| {
            SymbolPath::segments_of(SymbolName::intern(key))
                .iter()
                .map(|s| s.as_str().to_string())
                .collect::<alloc::vec::Vec<_>>()
        };
        assert_eq!(segments(r#""a::b"#), [r#""a"#, "b"]);
        assert_eq!(segments(r#""a"b::c"#), [r#""a"b"#, "c"]);
    }

    /// A quoted Miden Assembly segment holding `::` stays one segment through the component key,
    /// the printed path and the library path.
    #[test]
    fn a_quoted_library_path_segment_survives_the_component_key() {
        use midenc_session::miden_assembly_syntax::Path;

        let source = "::\"acme::pkg\"::app";
        let path = SymbolPath::from_library_path(Path::new(source));
        let key = path.to_symbol_name();
        assert_eq!(key.as_str(), r#""acme::pkg"::app"#);

        let segments = SymbolPath::segments_of(key);
        assert_eq!(
            segments.as_slice(),
            &[SymbolName::intern("acme::pkg"), SymbolName::intern("app")]
        );

        let rebuilt = SymbolPath::from_iter(
            core::iter::once(SymbolNameComponent::Root)
                .chain(segments.into_iter().map(SymbolNameComponent::Component)),
        );
        assert_eq!(rebuilt, path);
        assert_eq!(rebuilt.to_string(), source);
        assert_eq!(rebuilt.to_library_path().to_string(), source);
    }
}
