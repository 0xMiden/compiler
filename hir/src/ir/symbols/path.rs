use alloc::{borrow::Cow, collections::VecDeque, format};
use core::fmt;

use midenc_session::diagnostics::{Diagnostic, miette};
use smallvec::{SmallVec, smallvec};

use super::SymbolUseRef;
use crate::{FunctionIdent, SymbolName, define_attr_type, interner};

#[derive(Debug, thiserror::Error, Diagnostic)]
pub enum InvalidSymbolPathError {
    #[error("invalid symbol path: cannot be empty")]
    Empty,
    #[error("invalid symbol path: invalid format")]
    #[diagnostic(help(
        "The grammar for symbols is `<namespace>:<package>[/<export>]*[@<version>]"
    ))]
    InvalidFormat,
    #[error("invalid symbol path: missing package")]
    #[diagnostic(help(
        "A fully-qualified symbol must namespace packages, i.e. `<namespace>:<package>`, but \
         you've only provided one of these"
    ))]
    MissingPackage,
    #[error("invalid symbol path: only fully-qualified symbols can be versioned")]
    UnexpectedVersion,
    #[error("invalid symbol path: unexpected character '{token}' at byte {pos}")]
    UnexpectedToken { token: char, pos: usize },
    #[error("invalid symbol path: no leaf component was provided")]
    MissingLeaf,
    #[error("invalid symbol path: unexpected components found after leaf")]
    UnexpectedTrailingComponents,
    #[error("invalid symbol path: only one root component is allowed, and it must come first")]
    UnexpectedRootPlacement,
}

#[derive(Clone, PartialEq, Eq)]
pub struct SymbolPathAttr {
    pub path: SymbolPath,
    pub user: SymbolUseRef,
}

impl fmt::Display for SymbolPathAttr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", &self.path)
    }
}

impl crate::formatter::PrettyPrint for SymbolPathAttr {
    fn render(&self) -> crate::formatter::Document {
        crate::formatter::display(self)
    }
}

impl fmt::Debug for SymbolPathAttr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SymbolPathAttr")
            .field("path", &self.path)
            .field("user", &self.user.borrow())
            .finish()
    }
}

impl core::hash::Hash for SymbolPathAttr {
    fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
        self.path.hash(state);
        self.user.hash(state);
    }
}

define_attr_type!(SymbolPathAttr);

/// This type is a custom [Attribute] for [Symbol] references.
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
        use midenc_session::{
            LibraryNamespace, LibraryPath,
            miden_assembly::{SourceSpan, Span, ast::Ident},
        };

        let mut components = self.path.iter();
        let mut parts = SmallVec::<[_; 3]>::default();
        if self.is_absolute() {
            let _ = components.next();
        }
        let ns = match components.next() {
            None => {
                return LibraryPath::new_from_components(LibraryNamespace::Anon, parts);
            }
            Some(component) => LibraryNamespace::from_ident_unchecked(Ident::from_raw_parts(
                Span::new(SourceSpan::default(), component.as_symbol_name().as_str().into()),
            )),
        };

        for component in components {
            let id = Ident::from_raw_parts(Span::new(
                SourceSpan::default(),
                component.as_symbol_name().as_str().into(),
            ));
            parts.push(id);
        }

        LibraryPath::new_from_components(ns, parts)
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

/// Print symbol path according to Wasm Component Model rules, i.e.:
///
/// ```text,ignore
/// PATH ::= NAMESPACE ":" PACKAGE PACKAGE_PATH? VERSION?
///
/// NAMESPACE ::= SYMBOL
/// PACKAGE ::= SYMBOL (":" SYMBOL)*
/// PACKAGE_PATH ::= ("/" SYMBOL)+
/// VERSION ::= "@" VERSION_STRING
/// ```
///
/// The first component of an absolute path (ignoring the `Root` node) is expected to be the package
/// name, i.e. the `NAMESPACE ":" PACKAGE` part as a single symbol.
///
/// The first component of a relative path is expected to be either `Component` or `Leaf`
impl fmt::Display for SymbolPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use core::fmt::Write;

        let mut components = self.path.iter();

        if self.is_absolute() {
            let _ = components.next();
        }

        match components.next() {
            Some(component) => f.write_str(component.as_symbol_name().as_str())?,
            None => return Ok(()),
        }
        for component in components {
            f.write_char('/')?;
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

/// An iterator over [SymbolNameComponent] derived from a path symbol and leaf symbol.
pub struct SymbolNameComponents {
    parts: VecDeque<&'static str>,
    name: SymbolName,
    absolute: bool,
    done: bool,
}

impl SymbolNameComponents {
    /// Construct a new [SymbolNameComponents] iterator from a Wasm Component Model symbol.
    ///
    /// The syntax for such symbols are described by the following EBNF-style grammar:
    ///
    /// ```text,ignore
    /// SYMBOL ::= ID
    /// QUALIFIED_SYMBOL ::= NAMESPACE ("/" ID)* ("@" VERSION)?
    /// NAMESPACE ::= (ID ":")+ ID
    /// ID ::= ID_CHAR+
    /// ID_CHAR ::= 'A'..'Z'
    ///           | 'a'..'z'
    ///           | '0'..'z'
    ///           | '-'
    /// ```text,ignore
    ///
    /// This corresponds to identifiers of the form:
    ///
    /// * `foo` (referencing `foo` in the current scope)
    /// * `miden:base/foo` (importing `foo` from the `miden:base` package)
    /// * `miden:base/foo/bar` (importing `bar` from the `foo` interface of `miden:base`)
    /// * `miden:base/foo/bar@1.0.0` (same as above, but specifying an exact package version)
    ///
    /// The following are not permitted:
    ///
    /// * `foo@1.0.0` (cannot reference a different version of the current package)
    /// * `miden/foo` (packages must be namespaced, i.e. `<namespace>:<package>`)
    pub fn from_component_model_symbol(symbol: SymbolName) -> Result<Self, crate::Report> {
        use core::{iter::Peekable, str::CharIndices};

        let mut parts = VecDeque::default();
        if symbol == interner::symbols::Empty {
            let done = symbol == interner::symbols::Empty;
            return Ok(Self {
                parts,
                name: symbol,
                done,
                absolute: false,
            });
        }

        #[inline(always)]
        fn is_valid_id_char(c: char) -> bool {
            c.is_ascii_alphanumeric() || c == '-'
        }

        fn lex_id<'a>(
            s: &'a str,
            start: usize,
            lexer: &mut Peekable<CharIndices<'a>>,
        ) -> Option<(usize, &'a str)> {
            let mut end = start;
            while let Some((i, c)) = lexer.next_if(|(_, c)| is_valid_id_char(*c)) {
                end = i + c.len_utf8();
            }
            if end == start {
                return None;
            }
            Some((end, unsafe { core::str::from_utf8_unchecked(&s.as_bytes()[start..end]) }))
        }

        let input = symbol.as_str();
        let mut chars = input.char_indices().peekable();
        let mut pos = 0;

        // Parse the package name
        let mut absolute = false;
        let package_end = loop {
            let (new_pos, _) = lex_id(input, pos, &mut chars).ok_or_else(|| {
                crate::Report::msg(format!(
                    "invalid component model symbol: '{symbol}' contains invalid characters"
                ))
            })?;
            pos = new_pos;

            if let Some((new_pos, c)) = chars.next_if(|(_, c)| *c == ':') {
                pos = new_pos + c.len_utf8();
                absolute = true;
            } else {
                break pos;
            }
        };

        // Check if this is just a local symbol or package name
        if chars.peek().is_none() {
            let symbol =
                unsafe { core::str::from_utf8_unchecked(&input.as_bytes()[pos..package_end]) };
            return Ok(Self {
                parts,
                name: SymbolName::intern(symbol),
                done: false,
                absolute,
            });
        }

        // Push the package name to `parts`
        let package_name =
            unsafe { core::str::from_utf8_unchecked(&input.as_bytes()[pos..package_end]) };
        parts.push_back(package_name);

        // The next character may be either a version (if absolute), or "/"
        //
        // Advance the lexer as appropriate
        match chars.next_if(|(_, c)| *c == '/') {
            None => {
                // If the next char is not '@', the format is invalid
                // If the char is '@', but the path is not absolute, the format is invalid
                if chars.next_if(|(_, c)| *c == '@').is_some() {
                    if !absolute {
                        return Err(crate::Report::msg(
                            "invalid component model symbol: unqualified symbols cannot be \
                             versioned",
                        ));
                    }
                    // TODO(pauls): Add support for version component
                    //
                    // For now we drop it
                    parts.clear();
                    return Ok(Self {
                        parts,
                        name: SymbolName::intern(package_name),
                        done: false,
                        absolute,
                    });
                } else {
                    return Err(crate::Report::msg(format!(
                        "invalid component model symbol: unexpected character in '{symbol}' \
                         starting at byte {pos}"
                    )));
                }
            }
            Some((new_pos, c)) => {
                pos = new_pos + c.len_utf8();
            }
        }

        // Parse `ID ("/" ID)*+` until we reach end of input, or `"@"`
        loop {
            let (new_pos, id) = lex_id(input, pos, &mut chars).ok_or_else(|| {
                crate::Report::msg(format!(
                    "invalid component model symbol: '{symbol}' contains invalid characters"
                ))
            })?;
            pos = new_pos;

            if let Some((new_pos, c)) = chars.next_if(|(_, c)| *c == '/') {
                pos = new_pos + c.len_utf8();
                parts.push_back(id);
            } else {
                break;
            }
        }

        // If the next char is '@', we have a version
        //
        // TODO(pauls): Add support for version component
        //
        // For now, ignore it
        if chars.next_if(|(_, c)| *c == '@').is_some() {
            let name = SymbolName::intern(parts.pop_back().unwrap());
            return Ok(Self {
                parts,
                name,
                done: false,
                absolute,
            });
        }

        // We should be at the end now, or the format is invalid
        if chars.peek().is_none() {
            let name = SymbolName::intern(parts.pop_back().unwrap());
            Ok(Self {
                parts,
                name,
                done: false,
                absolute,
            })
        } else {
            Err(crate::Report::msg(format!(
                "invalid component model symbol: '{symbol}' contains invalid character starting \
                 at byte {pos}"
            )))
        }
    }

    /// Convert this iterator into a single [Symbol] consisting of all components.
    ///
    /// Returns `None` if the input is empty.
    pub fn into_symbol_name(self) -> Option<SymbolName> {
        let attr = self.into_symbol_path()?;

        Some(SymbolName::intern(attr))
    }

    /// Convert this iterator into a [SymbolPath].
    ///
    ///
    /// Returns `None` if the input is empty.
    pub fn into_symbol_path(self) -> Option<SymbolPath> {
        if self.name == interner::symbols::Empty {
            return None;
        }

        if self.parts.is_empty() {
            return Some(SymbolPath {
                path: smallvec![SymbolNameComponent::Leaf(self.name)],
            });
        }

        // Pre-allocate the storage for the internal SymbolPath path
        let mut path = SmallVec::<[_; 3]>::with_capacity(self.parts.len() + 1);

        // Handle the first path component which tells us whether or not the path is rooted
        let mut parts = self.parts.into_iter();
        if let Some(part) = parts.next() {
            if part == "::" {
                path.push(SymbolNameComponent::Root);
            } else {
                path.push(SymbolNameComponent::Component(SymbolName::intern(part)));
            }
        }

        // Append the remaining parts as intermediate path components
        path.extend(parts.map(SymbolName::intern).map(SymbolNameComponent::Component));

        // Finish up with the leaf symbol
        path.push(SymbolNameComponent::Leaf(self.name));

        Some(SymbolPath { path })
    }
}

impl core::iter::FusedIterator for SymbolNameComponents {}
impl Iterator for SymbolNameComponents {
    type Item = SymbolNameComponent;

    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }
        if self.absolute {
            self.absolute = false;
            return Some(SymbolNameComponent::Root);
        }
        if let Some(part) = self.parts.pop_front() {
            return Some(SymbolNameComponent::Component(part.into()));
        }
        self.done = true;
        Some(SymbolNameComponent::Leaf(self.name))
    }
}
impl ExactSizeIterator for SymbolNameComponents {
    fn len(&self) -> usize {
        let is_empty = self.name == interner::symbols::Empty;
        if is_empty {
            assert_eq!(self.parts.len(), 0, "malformed symbol name components");
            0
        } else {
            self.parts.len() + 1
        }
    }
}
