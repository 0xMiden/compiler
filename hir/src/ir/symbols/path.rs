use alloc::{borrow::Cow, string::String};
use core::fmt;

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

    /// Returns the `::`-separated segments of the name of a symbol-table op.
    ///
    /// A symbol path segment never contains `::`. A symbol-table op (in practice, a component)
    /// may be *named* by a `::`-joined path; such a name occupies one entry in its parent's
    /// symbol table, but contributes one [SymbolNameComponent::Component] per segment to every
    /// [SymbolPath] that passes through it. Leaf symbol names (functions, globals) are opaque and
    /// must never be split with this function.
    pub fn segments_of(name: SymbolName) -> SmallVec<[SymbolName; 3]> {
        let s = name.as_str();
        if !s.contains("::") {
            return smallvec![name];
        }
        s.split("::").map(SymbolName::intern).collect()
    }

    /// Returns all non-root components of this path (leaf included) joined with `::`.
    ///
    /// This is the symbol-table key of a component named by this path, and the inverse of
    /// [SymbolPath::segments_of].
    pub fn to_symbol_name(&self) -> SymbolName {
        let components = self
            .path
            .iter()
            .filter(|c| !c.is_root())
            .copied()
            .collect::<SmallVec<[SymbolNameComponent; 4]>>();
        Self::join_components(&components)
    }

    /// Joins the names of `components` with `::`.
    pub(crate) fn join_components(components: &[SymbolNameComponent]) -> SymbolName {
        match components {
            [single] => single.as_symbol_name(),
            _ => {
                let mut joined = String::new();
                for (i, component) in components.iter().enumerate() {
                    if i > 0 {
                        joined.push_str("::");
                    }
                    joined.push_str(component.as_symbol_name().as_str());
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

/// Prints the path Miden Assembly style: segments joined by `::`, with a leading `::` when the
/// path is absolute.
impl fmt::Display for SymbolPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut components = self.path.iter();

        if self.is_absolute() {
            let _ = components.next();
            f.write_str("::")?;
        }

        match components.next() {
            Some(component) => f.write_str(component.as_symbol_name().as_str())?,
            None => return Ok(()),
        }
        for component in components {
            f.write_str("::")?;
            f.write_str(component.as_symbol_name().as_str())?;
        }
        Ok(())
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
}
