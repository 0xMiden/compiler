use alloc::collections::VecDeque;
use core::fmt;

use smallvec::SmallVec;

use super::SymbolUseRef;
use crate::{define_attr_type, interner};

/// Represents the name of a [Symbol] in its local [SymbolTable]
pub type SymbolName = interner::Symbol;

/// This type is a custom [Attribute] for [Symbol] references.
#[derive(Clone)]
pub struct SymbolNameAttr {
    /// The [SymbolUse] corresponding to this use of the referenced symbol.
    pub user: SymbolUseRef,
    /// The path through the abstract symbol space to the containing symbol table
    ///
    /// It is assumed that all symbol tables are also symbols themselves, and thus the path to
    /// `name` is formed from the names of all parent symbol tables, in hierarchical order.
    ///
    /// For example, consider a program consisting of a single component `@test_component`,
    /// containing a module `@foo`, which in turn contains a function `@a`. The `path` for `@a`
    /// would be `@test_component::@foo`, and `name` would be `@a`.
    ///
    /// If set to `interner::symbols::Empty`, the symbol `name` is in the global namespace.
    ///
    /// If set to any other value, then we recover the components of the path by splitting the
    /// value on `::`. If not present, the path represents a single namespace. If multiple parts
    /// are present, then each part represents a nested namespace starting from the global one.
    pub path: SymbolName,
    /// The name of the symbol
    pub name: SymbolName,
}

define_attr_type!(SymbolNameAttr);

impl SymbolNameAttr {
    #[inline(always)]
    pub const fn name(&self) -> SymbolName {
        self.name
    }

    #[inline(always)]
    pub const fn path(&self) -> SymbolName {
        self.path
    }

    /// Returns true if this symbol name is fully-qualified
    pub fn is_absolute(&self) -> bool {
        self.path.as_str().starts_with("::")
    }

    #[inline]
    pub fn has_parent(&self) -> bool {
        self.path != interner::symbols::Empty
    }

    pub fn components(&self) -> SymbolNameComponents {
        SymbolNameComponents::new(self.path, self.name)
    }
}

impl fmt::Display for SymbolNameAttr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.has_parent() {
            write!(f, "{}::{}", &self.path, &self.name)
        } else {
            f.write_str(self.name.as_str())
        }
    }
}

impl fmt::Debug for SymbolNameAttr {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("SymbolNameAttr")
            .field("user", &self.user.borrow())
            .field("path", &self.path)
            .field("name", &self.name)
            .finish()
    }
}
impl crate::formatter::PrettyPrint for SymbolNameAttr {
    fn render(&self) -> crate::formatter::Document {
        use crate::formatter::*;
        display(self)
    }
}
impl Eq for SymbolNameAttr {}
impl PartialEq for SymbolNameAttr {
    fn eq(&self, other: &Self) -> bool {
        self.path == other.path && self.name == other.name
    }
}
impl PartialOrd for SymbolNameAttr {
    #[inline]
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for SymbolNameAttr {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        self.path.cmp(&other.path).then_with(|| self.name.cmp(&other.name))
    }
}
impl core::hash::Hash for SymbolNameAttr {
    fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
        self.name.hash(state);
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
#[derive(Copy, Clone, PartialEq, Eq)]
pub enum SymbolNameComponent {
    /// A component that signals the path is relative to the root symbol table
    Root,
    /// A component of the symbol name path
    Component(SymbolName),
    /// The name of the symbol in its local symbol table
    Leaf(SymbolName),
}
impl fmt::Display for SymbolNameComponent {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Root => f.write_str("::"),
            Self::Component(name) | Self::Leaf(name) => f.write_str(name.as_str()),
        }
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

/// An iterator over [SymbolNameComponent] derived from a path symbol and leaf symbol.
pub struct SymbolNameComponents {
    parts: VecDeque<&'static str>,
    name: SymbolName,
    done: bool,
}

impl SymbolNameComponents {
    /// Construct a new [SymbolNameComponents] iterator for a symbol `name` qualified with `path`.
    pub(super) fn new(path: SymbolName, name: SymbolName) -> Self {
        let mut parts = VecDeque::default();
        if path == interner::symbols::Empty {
            return Self {
                parts,
                name,
                done: true,
            };
        }

        let mut split = path.as_str().split("::");
        let start = split.next().unwrap();
        if start.is_empty() {
            parts.push_back("::");
        }

        while let Some(part) = split.next() {
            if part.is_empty() {
                if let Some(part2) = split.next() {
                    if part2.is_empty() {
                        parts.push_back("::");
                    } else {
                        parts.push_back(part2);
                    }
                } else {
                    break;
                }
            } else {
                parts.push_back(part);
            }
        }

        Self {
            parts,
            name,
            done: false,
        }
    }

    pub(super) fn from_raw_parts(parts: VecDeque<&'static str>, name: SymbolName) -> Self {
        Self {
            parts,
            name,
            done: false,
        }
    }

    /// Convert this iterator into a symbol name representing the path prefix of a [Symbol].
    ///
    /// If `absolute` is set to true, then the resulting path will be prefixed with `::`
    pub fn into_path(self, absolute: bool) -> SymbolName {
        if self.parts.is_empty() {
            return ::midenc_hir_symbol::symbols::Empty;
        }

        let mut buf =
            String::with_capacity(2usize + self.parts.iter().map(|p| p.len()).sum::<usize>());
        if absolute {
            buf.push_str("::");
        }
        for part in self.parts {
            buf.push_str(part);
        }
        SymbolName::intern(buf)
    }
}

impl core::iter::FusedIterator for SymbolNameComponents {}
impl Iterator for SymbolNameComponents {
    type Item = SymbolNameComponent;

    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }
        if let Some(part) = self.parts.pop_front() {
            if part == "::" {
                return Some(SymbolNameComponent::Root);
            }
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

/// This type represents computed metadata about a symbol reference, i.e. in addition to the name
/// itself, it parses the components of the symbol reference so that it can be reasoned about in
/// more precise terms.
#[derive(Debug, Eq, PartialEq)]
pub struct SymbolNameInfo {
    /// The raw path name
    path: SymbolName,
    /// The raw symbol name
    name: SymbolName,
    /// The parsed components of the symbol
    components: SmallVec<[SymbolNameComponent; 8]>,
}
impl SymbolNameInfo {
    pub fn new(path: SymbolName, name: SymbolName) -> Self {
        let components = SymbolNameComponents::new(path, name);
        let components = components.collect::<SmallVec<[_; 8]>>();
        Self {
            path,
            name,
            components,
        }
    }

    /// If this symbol name is prefixed with `::`, it is considered absolute
    pub fn is_absolute(&self) -> bool {
        matches!(&self.components[0], SymbolNameComponent::Root)
    }

    /// Get the prefix path of this reference
    pub fn path(&self) -> SymbolName {
        self.path
    }

    /// Get the symbol name represented by this reference
    pub fn name(&self) -> SymbolName {
        self.name
    }

    /// Get the basename component of this symbol name
    pub fn basename(&self) -> SymbolName {
        match self.components.last().copied().unwrap() {
            SymbolNameComponent::Leaf(name) => name,
            component => unreachable!("invalid trailing symbol name component: {component:?}"),
        }
    }

    /// Get a reference to the underlying components
    pub fn components(&self) -> &[SymbolNameComponent] {
        &self.components
    }

    /// Consume this value and convert it into the raw underlying components
    pub fn into_components(self) -> SmallVec<[SymbolNameComponent; 8]> {
        self.components
    }

    /// Returns true if `self` is a prefix of `other`, i.e. `self` is a further qualified symbol
    /// reference.
    ///
    /// NOTE: If `self` and `other` are equal, `self` is considered a prefix. The caller should
    /// check if the two references are identical if they wish to distinguish the two cases.
    pub fn is_prefix_of(&self, other: &Self) -> bool {
        // If the symbols are identical, then `self` is trivially a prefix of `other`
        if self.path == other.path && self.name == other.name {
            return true;
        }

        // If one path is absolute and the other is not, we can't reason about prefixes
        if self.is_absolute() != other.is_absolute() {
            return false;
        }

        // Otherwise, if the components of `self` are the same length or greater than `other`,
        // then `self` cannot be a prefix
        if self.components.len() >= other.components.len() {
            return false;
        }

        // All components of `self` (sans the leaf) must match all leading components of `other`
        let (_leaf, prefix) = self.components.split_last().unwrap();
        prefix == &other.components[..prefix.len()]
    }
}

impl fmt::Display for SymbolNameInfo {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}::{}", &self.path, &self.name)
    }
}

/// Generate a unique symbol name.
///
/// Iteratively increase `counter` and use it as a suffix for symbol names until `is_unique` does
/// not detect any conflict.
pub fn generate_symbol_name<F>(name: SymbolName, counter: &mut usize, is_unique: F) -> SymbolName
where
    F: Fn(&str) -> bool,
{
    use core::fmt::Write;

    use crate::SmallStr;

    if is_unique(name.as_str()) {
        return name;
    }

    let base_len = name.as_str().len();
    let mut buf = SmallStr::with_capacity(base_len + 2);
    buf.push_str(name.as_str());
    loop {
        *counter += 1;
        buf.truncate(base_len);
        buf.push('_');
        write!(&mut buf, "{counter}").unwrap();

        if is_unique(buf.as_str()) {
            break SymbolName::intern(buf);
        }
    }
}
