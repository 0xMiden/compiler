use alloc::{borrow::Cow, format};
use core::fmt;

use midenc_session::{
    LibraryPath,
    miden_assembly_syntax::{Path, PathComponent, ast::Ident},
};

use crate::{
    FunctionIdent, Report, SmallVec, SymbolName,
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
    /// The id is read as a Miden Assembly path: each component holds the raw identifier, without
    /// the quotes its spelling may carry, and a quoted segment containing `::` stays one component
    /// (`"acme::pkg"::app` has the components `acme::pkg` and `app`). An id that is not a valid
    /// Miden Assembly path is split on every `::` instead, and a segment wrapped in one pair of
    /// quotes loses them: [FunctionIdent]s are also parsed from arbitrary source names
    /// (`<T as Trait>::f`, `core::ptr::drop_in_place<T>`), which are matched against known paths.
    ///
    /// The components are raw segments, also when the path names a module tree to declare (see
    /// `WorldBuilder::declare_module_tree`); the builders reject a segment Miden Assembly cannot
    /// spell.
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
        let root = core::iter::once(SymbolNameComponent::Root);
        match Path::validate(id) {
            Ok(path) => Self::from_iter(
                root.chain(
                    normal_components(path)
                        .map(|segment| SymbolNameComponent::Component(SymbolName::intern(segment))),
                ),
            ),
            Err(_) => Self::from_iter(root.chain(id.split("::").map(|segment| {
                let segment = segment
                    .strip_prefix('"')
                    .and_then(|segment| segment.strip_suffix('"'))
                    .unwrap_or(segment);
                SymbolNameComponent::Component(SymbolName::intern(segment))
            }))),
        }
    }

    /// Returns the segments of the name of a symbol-table op, the inverse of
    /// [SymbolPath::to_symbol_name] for segments Miden Assembly can spell (see
    /// [SymbolPath::is_spellable]).
    ///
    /// A component may be *named* by a multi-segment path; such a name occupies one entry in its
    /// parent's symbol table, but contributes one [SymbolNameComponent::Component] per segment to
    /// every [SymbolPath] that passes through it. The name of a component or module is the
    /// Miden Assembly spelling of its raw segments: the segments joined with `::`, each quoted,
    /// `"..."`, when it is not a bare Miden Assembly identifier (`PathBuf::push_component`). So
    /// `["acme::pkg", "app"]` is named `"acme::pkg"::app`, `["a:b@1.0.0"]` is named `"a:b@1.0.0"`,
    /// and a bare identifier names itself. Module names are always one segment. Leaf symbol names
    /// (functions, globals) are opaque and must never be split with this function.
    ///
    /// A name that is not a valid Miden Assembly path is returned whole, as one segment; such a
    /// name is rejected by the symbol-table verifiers.
    pub fn segments_of(name: SymbolName) -> SmallVec<[SymbolName; 3]> {
        let s = name.as_str();
        // Neither a separator nor a quote: the name is one segment whether or not it is valid
        if !s.contains("::") && !s.contains('"') {
            return smallvec![name];
        }
        let mut segments = SmallVec::new();
        for component in Path::new(s).components() {
            match component {
                Ok(PathComponent::Root) => (),
                Ok(component) => segments.push(SymbolName::intern(component.as_str())),
                Err(_) => return smallvec![name],
            }
        }
        // A lone `::` is the root, which names nothing
        if segments.is_empty() {
            return smallvec![name];
        }
        segments
    }

    /// Returns the Miden Assembly spelling of the segments of `name` (see
    /// [SymbolPath::segments_of]), i.e. the name a component given `name` gets when `name` is
    /// already spelled. A module or interface reads its name as one raw segment instead (see
    /// `Module::canonical_name`).
    ///
    /// Idempotent: a spelled name is returned as is, and a raw single segment that is not a bare
    /// identifier comes back quoted (`masm-dep` is named `"masm-dep"`).
    pub fn canonical_name(name: SymbolName) -> SymbolName {
        Self::spell(&Self::segments_of(name))
    }

    /// Returns the Miden Assembly spelling of the raw `segments`.
    pub(crate) fn spell(segments: &[SymbolName]) -> SymbolName {
        let components = segments
            .iter()
            .copied()
            .map(SymbolNameComponent::Component)
            .collect::<SmallVec<[SymbolNameComponent; 4]>>();
        Self::join_components(&components)
    }

    /// Returns an error when `name` cannot name a symbol-table op of `kind`: it is not the Miden
    /// Assembly spelling of its raw segments, a segment cannot be spelled in Miden Assembly at
    /// all, or `kind` is named by one segment (see [SymbolTableKind::is_named_by_a_path]) and the
    /// name has several, or one containing `::`.
    pub(crate) fn validate_table_name(
        kind: SymbolTableKind,
        name: SymbolName,
    ) -> Result<(), Report> {
        let segments = Self::segments_of(name);
        for segment in &segments {
            Self::validate_segment(kind, segment.as_str())?;
        }
        if Self::spell(&segments) != name {
            let advice = if kind.is_named_by_a_path() {
                "; name components through `SymbolPath::to_symbol_name`"
            } else {
                ""
            };
            return Err(Report::msg(format!(
                "{kind} `{name}` is not the Miden Assembly spelling of its segments{advice}"
            )));
        }
        match segments.as_slice() {
            _ if kind.is_named_by_a_path() => Ok(()),
            [segment] => Self::validate_single_segment(kind, segment.as_str()),
            _ => Self::validate_single_segment(kind, name.as_str()),
        }
    }

    /// Returns the name a symbol-table op of `kind` given `name` gets, or an error when it cannot
    /// name one (see [SymbolPath::validate_table_name]).
    ///
    /// For a module or an interface, `name` is one raw segment, which must not contain `::`. For
    /// a component, a name that already is the Miden Assembly spelling of its segments is taken
    /// as is (`miden::a::b`, `"acme::pkg"::app`); any other name is one raw segment, which is
    /// spelled (`masm-dep` is named `"masm-dep"`).
    pub(crate) fn table_name(
        kind: SymbolTableKind,
        name: SymbolName,
    ) -> Result<SymbolName, Report> {
        if kind.is_named_by_a_path() && Self::canonical_name(name) == name {
            Self::validate_table_name(kind, name)?;
            return Ok(name);
        }
        Self::validate_segment(kind, name.as_str())?;
        if !kind.is_named_by_a_path() {
            Self::validate_single_segment(kind, name.as_str())?;
        }
        Ok(Self::join_components(&[SymbolNameComponent::Component(name)]))
    }

    /// Returns true when Miden Assembly can spell the raw segment `segment`: it is not empty, and
    /// every character is ASCII graphic and neither `"`, `\` nor `#`.
    ///
    /// The characters are the Miden Assembly lexer's rule for quoted identifiers (`lex_quoted`
    /// reads quoted text made of anything else as a string) narrowed by `Ident::validate`, which
    /// rejects `#`.
    pub fn is_spellable(segment: &str) -> bool {
        !segment.is_empty()
            && segment.chars().all(|c| c.is_ascii_graphic() && !matches!(c, '"' | '\\' | '#'))
    }

    /// Returns the Miden Assembly spelling of the raw segment `segment`, i.e. the name of a
    /// symbol table named by it, or `None` when Miden Assembly cannot spell it (see
    /// [SymbolPath::is_spellable]).
    pub(crate) fn spell_segment(segment: SymbolName) -> Option<SymbolName> {
        Self::is_spellable(segment.as_str())
            .then(|| Self::join_components(&[SymbolNameComponent::Component(segment)]))
    }

    /// Returns an error when Miden Assembly cannot spell `segment`, a raw segment of the name of a
    /// symbol-table op of `kind` (see [SymbolPath::is_spellable]).
    pub(crate) fn validate_segment(kind: SymbolTableKind, segment: &str) -> Result<(), Report> {
        if Self::is_spellable(segment) {
            return Ok(());
        }
        Err(Report::msg(format!(
            "invalid {kind} name: the segment `{segment}` cannot be spelled in Miden Assembly"
        )))
    }

    /// Returns an error when `segment`, the one raw segment naming a module or an interface,
    /// contains `::`: only components are named by `::`-joined paths.
    fn validate_single_segment(kind: SymbolTableKind, segment: &str) -> Result<(), Report> {
        if !segment.contains("::") {
            return Ok(());
        }
        Err(Report::msg(format!(
            "{kind} `{segment}`: a {kind} name cannot contain `::` (only components are named by \
             `::`-joined paths)"
        )))
    }

    /// Returns the absolute path of the component named `name`, the Miden Assembly spelling of its
    /// segments (see [SymbolPath::segments_of]): the root followed by one
    /// [SymbolNameComponent::Component] per raw segment of the name.
    pub fn from_symbol_name(name: SymbolName) -> SymbolPath {
        Self::from_iter(
            core::iter::once(SymbolNameComponent::Root)
                .chain(Self::segments_of(name).into_iter().map(SymbolNameComponent::Component)),
        )
    }

    /// Returns true when the name `inner` nests in the name `outer`, i.e. when `outer` is a strict
    /// segment-prefix of `inner`: `a::bc` nests in `a` but not in `a::b`, and no name nests in
    /// itself. Both are names of symbol-table ops (see [SymbolPath::segments_of]) and are compared
    /// segment by segment: `"acme::pkg"::app` nests in `"acme::pkg"`, not in `acme`.
    pub fn nests_in(inner: SymbolName, outer: SymbolName) -> bool {
        // A one-segment name nests in nothing; skip splitting (and interning) both names.
        if !inner.as_str().contains("::") {
            return false;
        }
        let (inner, outer) = (Self::segments_of(inner), Self::segments_of(outer));
        inner.len() > outer.len() && inner.starts_with(&outer)
    }

    /// Returns the Miden Assembly spelling of all non-root components of this path (leaf
    /// included), relative.
    ///
    /// This is the name of a component named by this path, and the inverse of
    /// [SymbolPath::segments_of]: the raw segments joined with `::`, each quoted when it is not a
    /// bare Miden Assembly identifier, e.g. `"acme::pkg"::app`.
    ///
    /// Returns an error when Miden Assembly cannot spell a segment (see
    /// [SymbolPath::is_spellable]); in particular, a raw segment wrapped in quotes is not read as
    /// the segment it quotes.
    pub fn to_symbol_name(&self) -> Result<SymbolName, Report> {
        for component in self.path.iter().filter(|component| !component.is_root()) {
            Self::validate_segment(
                SymbolTableKind::Component,
                component.as_symbol_name().as_str(),
            )?;
        }
        Ok(Self::join_components(&self.path))
    }

    /// Returns the Miden Assembly spelling of the non-root `components`, relative (see
    /// [SymbolPath::to_symbol_name]).
    pub(crate) fn join_components(components: &[SymbolNameComponent]) -> SymbolName {
        let components =
            components.strip_prefix(&[SymbolNameComponent::Root]).unwrap_or(components);
        // The common case of one bare identifier names itself, without building the path
        if let [single] = components {
            let name = single.as_symbol_name();
            if !Ident::requires_quoting(name.as_str()) {
                return name;
            }
        }
        let mut path = LibraryPath::default();
        for component in components {
            push_segment(&mut path, component.as_symbol_name().as_str());
        }
        // `push_component` roots a leading `$kernel`/`$exec`; a name is relative
        SymbolName::intern(path.to_relative().as_str())
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
    pub fn to_library_path(&self) -> LibraryPath {
        let mut path = LibraryPath::default();
        for component in self.path.iter() {
            match component {
                SymbolNameComponent::Root => path.push_component("::"),
                component => push_segment(&mut path, component.as_symbol_name().as_str()),
            }
        }
        path
    }

    /// Derive a symbol path from the Miden Assembly path `path`, the inverse of
    /// [SymbolPath::to_library_path]: one [SymbolNameComponent::Component] per segment of the
    /// path, holding the raw identifier (without quotes), rooted when the path is absolute.
    ///
    /// A quoted segment stays one component. Segments that fail to parse are skipped; a valid
    /// path has none.
    pub fn from_library_path(path: &Path) -> SymbolPath {
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

/// Prints the path in its Miden Assembly spelling ([SymbolPath::to_library_path]): the segments
/// joined with `::`, each quoted when it is not a bare Miden Assembly identifier, with a leading
/// `::` when the path is absolute, e.g. `::"acme::pkg"::app`. A relative path stays relative,
/// also when it starts with `$kernel` or `$exec`.
impl fmt::Display for SymbolPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let path = self.to_library_path();
        if self.is_absolute() {
            fmt::Display::fmt(&path, f)
        } else {
            // `push_component` roots a leading `$kernel`/`$exec`
            fmt::Display::fmt(path.to_relative(), f)
        }
    }
}

/// Appends the raw identifier `segment` to `path`, quoted when it is not a bare Miden Assembly
/// identifier.
///
/// `segment` is expected to be spellable (see [SymbolPath::is_spellable]); names are validated
/// before they reach here, while opaque leaf names only print. Three cases follow
/// `PathBuf::push_component`: a leading `$kernel` or `$exec` makes the path absolute, a segment
/// wrapped in `"` is taken as already quoted, and an empty segment is dropped.
fn push_segment(path: &mut LibraryPath, segment: &str) {
    match segment {
        // `push_component` reads a lone `::` as the root, not as a segment
        "::" => path.push_component("\"::\""),
        segment => path.push_component(segment),
    }
}

/// The raw identifiers of the non-root components of the valid Miden Assembly path `path`.
fn normal_components(path: &Path) -> impl Iterator<Item = &str> {
    path.components().filter_map(|component| match component.ok()? {
        PathComponent::Root => None,
        component => Some(component.as_str()),
    })
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

/// The symbol-table ops named by the Miden Assembly spelling of their raw segments (see
/// [SymbolPath::segments_of]).
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum SymbolTableKind {
    Component,
    Module,
    Interface,
}

impl SymbolTableKind {
    /// Returns true when an op of this kind may be named by a multi-segment path; the others are
    /// named by one segment.
    pub fn is_named_by_a_path(self) -> bool {
        matches!(self, Self::Component)
    }
}

impl fmt::Display for SymbolTableKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Component => "component",
            Self::Module => "module",
            Self::Interface => "interface",
        })
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
    use alloc::string::{String, ToString};

    use super::*;

    #[test]
    fn from_library_path_inverts_to_library_path() {
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
        assert!(!nests("\"a::b\"", "a"));
        assert!(nests("\"masm-dep\"::math", "\"masm-dep\""));
        // A non-canonical name is one segment
        assert!(!nests(r#""a::b"#, r#""a"#));
    }

    /// Joins `segments` into a name and splits it back.
    fn round_trip(segments: &[&str]) -> (SymbolName, alloc::vec::Vec<String>) {
        let components = segments
            .iter()
            .map(|segment| SymbolNameComponent::Component(SymbolName::intern(segment)))
            .collect::<SmallVec<[SymbolNameComponent; 4]>>();
        let name = SymbolPath::join_components(&components);
        let split = SymbolPath::segments_of(name).iter().map(|s| s.as_str().to_string()).collect();
        (name, split)
    }

    #[test]
    fn names_round_trip() {
        for (segments, name) in [
            (&["app"][..], "app"),
            (&["miden", "a", "b"][..], "miden::a::b"),
            (&["acme::pkg", "app"][..], r#""acme::pkg"::app"#),
            (&["acme::pkg"][..], r#""acme::pkg""#),
            (&["a:b@1.0.0"][..], r#""a:b@1.0.0""#),
            (&["masm-dep", "math"][..], r#""masm-dep"::math"#),
            (&["::"][..], r#""::""#),
            (&["a", "::", "b"][..], r#"a::"::"::b"#),
            (&["a:", "b::c"][..], r#""a:"::"b::c""#),
            (&["a", ":b"][..], r#"a::":b""#),
            (&["a:", "b"][..], r#""a:"::b"#),
            (&["$kernel", "x"][..], "$kernel::x"),
        ] {
            let (joined, split) = round_trip(segments);
            assert_eq!(joined.as_str(), name, "the name of {segments:?}");
            assert_eq!(split, segments, "the segments of `{name}`");
            assert_eq!(SymbolPath::canonical_name(joined), joined, "`{name}` is canonical");
        }
    }

    /// A `:` next to a segment boundary does not merge with the `::` separator.
    #[test]
    fn distinct_segment_lists_give_distinct_names() {
        let (left, left_split) = round_trip(&["a:", "b"]);
        let (right, right_split) = round_trip(&["a", ":b"]);
        assert_ne!(left, right);
        assert_eq!(left_split, ["a:", "b"]);
        assert_eq!(right_split, ["a", ":b"]);
        assert_ne!(round_trip(&["a::b"]).0, round_trip(&["a", "b"]).0);
    }

    /// A raw segment is re-spelled the way Miden Assembly does; a canonical name is kept.
    #[test]
    fn canonical_name_quotes_a_raw_segment_once() {
        let canonical = |name: &str| SymbolPath::canonical_name(SymbolName::intern(name));
        assert_eq!(canonical("masm-dep").as_str(), r#""masm-dep""#);
        assert_eq!(canonical(r#""masm-dep""#).as_str(), r#""masm-dep""#);
        assert_eq!(canonical(r#""app""#).as_str(), "app");
        assert_eq!(canonical("miden::a::b").as_str(), "miden::a::b");
    }

    /// Miden Assembly reads quoted text as an identifier only when it is made of ASCII graphic
    /// characters other than `"`, `\` and `#`.
    #[test]
    fn spellable_segments_mirror_the_miden_assembly_lexer() {
        for segment in ["app", "masm-dep", "a:b@1.0.0", "::", "$kernel", "1st"] {
            assert!(SymbolPath::is_spellable(segment), "`{segment}`");
        }
        for segment in ["", "q\"u", "\"x-y\"", "a\\b", "a#b", "my lib", "ü", "a\tb"] {
            assert!(!SymbolPath::is_spellable(segment), "`{segment}`");
        }
    }

    /// A raw segment wrapped in quotes is rejected rather than read as the segment it quotes, so
    /// `x-y` and `"x-y"` cannot both name `"x-y"`.
    #[test]
    fn a_quoted_raw_segment_does_not_name_a_component() {
        let name = |segment: &str| {
            SymbolPath::from_iter([
                SymbolNameComponent::Root,
                SymbolNameComponent::Component(SymbolName::intern(segment)),
            ])
            .to_symbol_name()
            .map(|name| name.as_str())
            .map_err(|err| err.to_string())
        };
        assert_eq!(name("x-y"), Ok(r#""x-y""#));
        assert_eq!(
            name(r#""x-y""#),
            Err(String::from(
                r#"invalid component name: the segment `"x-y"` cannot be spelled in Miden Assembly"#
            ))
        );
    }

    /// Every segment, the leaf included, prints in its Miden Assembly spelling.
    #[test]
    fn display_is_the_miden_assembly_spelling() {
        let path = SymbolPath::from_iter([
            SymbolNameComponent::Root,
            SymbolNameComponent::Component(SymbolName::intern("acme::pkg")),
            SymbolNameComponent::Leaf(SymbolName::intern("core::ptr::drop_in_place<T>")),
        ]);
        assert_eq!(path.to_string(), r#"::"acme::pkg"::"core::ptr::drop_in_place<T>""#);

        let display = |segments: &[&str]| {
            SymbolPath::from_iter(
                segments.iter().map(|s| SymbolNameComponent::Component(SymbolName::intern(s))),
            )
            .to_string()
        };
        assert_eq!(display(&["acme:first/api", "read"]), r#""acme:first/api"::read"#);
        assert_ne!(display(&["a:", "b"]), display(&["a", ":b"]));
        // A relative path stays relative, whatever its first segment
        assert_eq!(display(&["$kernel", "x"]), "$kernel::x");
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

    /// A name that is not a valid Miden Assembly path is one segment.
    #[test]
    fn a_non_canonical_name_is_one_segment() {
        for name in [r#""a::b"#, r#""a"b::c"#, "a b::c"] {
            assert_eq!(
                SymbolPath::segments_of(SymbolName::intern(name)).as_slice(),
                &[SymbolName::intern(name)],
                "`{name}`"
            );
        }
    }

    /// A Miden Assembly module id holds raw identifiers; one that is not a valid path is split on
    /// every `::`.
    #[test]
    fn from_masm_module_id_strips_quotes() {
        let components = |id: &str| {
            SymbolPath::from_masm_module_id(id)
                .components()
                .skip(1)
                .map(|c| c.as_symbol_name().as_str().to_string())
                .collect::<alloc::vec::Vec<_>>()
        };
        assert_eq!(components(r#""masm-dep"::math"#), ["masm-dep", "math"]);
        assert_eq!(components(r#""acme::pkg"::app"#), ["acme::pkg", "app"]);
        assert_eq!(components("<T as Trait>::f"), ["<T as Trait>", "f"]);
        // Split on every `::`, a quoted segment still loses its quotes
        assert_eq!(components(r#""masm-dep"::::f"#), ["masm-dep", "", "f"]);
        assert_eq!(
            SymbolPath::from_masm_module_id(r#""masm-dep"::math"#),
            SymbolPath::from_library_path(Path::new(r#"::"masm-dep"::math"#))
        );
    }

    /// A quoted Miden Assembly segment holding `::` stays one segment through the component name,
    /// the printed path and the library path.
    #[test]
    fn a_quoted_library_path_segment_survives_the_component_name() {
        let source = "::\"acme::pkg\"::app";
        let path = SymbolPath::from_library_path(Path::new(source));
        let key = path.to_symbol_name().expect("the segments are spellable");
        assert_eq!(key.as_str(), r#""acme::pkg"::app"#);

        let segments = SymbolPath::segments_of(key);
        assert_eq!(
            segments.as_slice(),
            &[SymbolName::intern("acme::pkg"), SymbolName::intern("app")]
        );

        let rebuilt = SymbolPath::from_symbol_name(key);
        assert_eq!(rebuilt, path);
        assert_eq!(rebuilt.to_string(), source);
        assert_eq!(rebuilt.to_library_path().to_string(), source);
    }
}
