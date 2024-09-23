use alloc::collections::VecDeque;
use core::fmt;

use crate::{
    define_attr_type, interner, InsertionPoint, Op, Operation, OperationRef, Report, Searcher,
    UnsafeIntrusiveEntityRef, Usable, Visibility,
};

/// Represents the name of a [Symbol] in its local [SymbolTable]
pub type SymbolName = interner::Symbol;

#[derive(Debug, Copy, Clone)]
pub struct SymbolNameAttr {
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

    pub fn components(&self) -> impl Iterator<Item = SymbolNameComponent> {
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

#[derive(Copy, Clone, PartialEq, Eq)]
pub enum SymbolNameComponent {
    /// A component that signals the path is relative to the root symbol table
    Root,
    /// A component of the symbol name path
    Component(SymbolName),
    /// The name of the symbol in its local symbol table
    Leaf(SymbolName),
}

struct SymbolNameComponents {
    parts: VecDeque<&'static str>,
    name: SymbolName,
    done: bool,
}
impl SymbolNameComponents {
    fn new(path: SymbolName, name: SymbolName) -> Self {
        let mut parts = VecDeque::default();
        if path == interner::symbols::Empty {
            return Self {
                parts,
                name,
                done: false,
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

/// A [SymbolTable] is an IR entity which contains other IR entities, called _symbols_, each of
/// which has a name, aka symbol, that uniquely identifies it amongst all other entities in the
/// same [SymbolTable].
///
/// The symbols in a [SymbolTable] do not need to all refer to the same entity type, however the
/// concrete value type of the symbol itself, e.g. `String`, must be the same. This is enforced
/// in the way that the [SymbolTable] and [Symbol] traits interact. A [SymbolTable] has an
/// associated `Key` type, and a [Symbol] has an associated `Id` type - only types whose `Id`
/// type matches the `Key` type of the [SymbolTable], can be stored in that table.
pub trait SymbolTable {
    /// Get a reference to the underlying [Operation]
    fn as_operation(&self) -> &Operation;

    /// Get a mutable reference to the underlying [Operation]
    fn as_operation_mut(&mut self) -> &mut Operation;

    /// Get the entry for `name` in this table
    fn get(&self, name: SymbolName) -> Option<SymbolRef>;

    /// Insert `entry` in the symbol table, but only if no other symbol with the same name exists.
    ///
    /// If provided, the symbol will be inserted at the given insertion point in the body of the
    /// symbol table operation.
    ///
    /// This function will panic if the symbol is attached to another symbol table.
    ///
    /// Returns `true` if successful, `false` if the symbol is already defined
    fn insert_new(&mut self, entry: SymbolRef, ip: Option<InsertionPoint>) -> bool;

    /// Like [SymbolTable::insert_new], except the symbol is renamed to avoid collisions.
    ///
    /// Returns the name of the symbol after insertion.
    fn insert(&mut self, entry: SymbolRef, ip: Option<InsertionPoint>) -> SymbolName;

    /// Remove the symbol `name`, and return the entry if one was present.
    fn remove(&mut self, name: SymbolName) -> Option<SymbolRef>;

    /// Renames the symbol named `from`, as `to`, as well as all uses of that symbol.
    ///
    /// Returns `Err` if unable to update all uses.
    fn rename(&mut self, from: SymbolName, to: SymbolName) -> Result<(), Report>;
}

impl dyn SymbolTable {
    /// Look up a symbol with the given name and concrete type, returning `None` if no such symbol
    /// exists
    pub fn find<T: Op + Symbol>(&self, name: SymbolName) -> Option<UnsafeIntrusiveEntityRef<T>> {
        let op = self.get(name)?;
        let op = op.borrow();
        let op = op.as_operation().downcast_ref::<T>()?;
        Some(unsafe { UnsafeIntrusiveEntityRef::from_raw(op) })
    }
}

/// A [Symbol] is an IR entity with an associated _symbol_, or name, which is expected to be unique
/// amongst all other symbols in the same namespace.
///
/// For example, functions are named, and are expected to be unique within the same module,
/// otherwise it would not be possible to unambiguously refer to a function by name. Likewise
/// with modules in a program, etc.
pub trait Symbol: Usable<Use = SymbolUse> + 'static {
    fn as_operation(&self) -> &Operation;
    fn as_operation_mut(&mut self) -> &mut Operation;
    /// Get the name of this symbol
    fn name(&self) -> SymbolName;
    /// Set the name of this symbol
    fn set_name(&mut self, name: SymbolName);
    /// Get the visibility of this symbol
    fn visibility(&self) -> Visibility;
    /// Returns true if this symbol has private visibility
    fn is_private(&self) -> bool;
    /// Returns true if this symbol has public visibility
    fn is_public(&self) -> bool;
    /// Sets the visibility of this symbol
    fn set_visibility(&mut self, visibility: Visibility);
    /// Sets the visibility of this symbol to private
    fn set_private(&mut self) {
        self.set_visibility(Visibility::Private);
    }
    /// Sets the visibility of this symbol to nested
    fn set_nested(&mut self) {
        self.set_visibility(Visibility::Nested);
    }
    /// Sets the visibility of this symbol to public
    fn set_public(&mut self) {
        self.set_visibility(Visibility::Public);
    }
    /// Get all of the uses of this symbol that are nested within `from`
    fn symbol_uses(&self, from: OperationRef) -> SymbolUseIter;
    /// Return true if there are no uses of this symbol nested within `from`
    fn symbol_uses_known_empty(&self, from: OperationRef) -> SymbolUseIter;
    /// Attempt to replace all uses of this symbol nested within `from`, with the provided replacement
    fn replace_all_uses(&self, replacement: SymbolRef, from: OperationRef) -> Result<(), Report>;
    /// Returns true if this operation can be discarded if it has no remaining symbol uses
    ///
    /// By default, if the visibility is non-public, a symbol is considered discardable
    fn can_discard_when_unused(&self) -> bool {
        !self.is_public()
    }
    /// Returns true if this operation is a declaration, rather than a definition, of a symbol
    ///
    /// The default implementation assumes that all operations are definitions
    fn is_declaration(&self) -> bool {
        false
    }
}

impl dyn Symbol {
    pub fn is<T: Op + Symbol>(&self) -> bool {
        let op = self.as_operation();
        op.is::<T>()
    }

    pub fn downcast_ref<T: Op + Symbol>(&self) -> Option<&T> {
        let op = self.as_operation();
        op.downcast_ref::<T>()
    }

    pub fn downcast_mut<T: Op + Symbol>(&mut self) -> Option<&mut T> {
        let op = self.as_operation_mut();
        op.downcast_mut::<T>()
    }

    /// Get an [OperationRef] for the operation underlying this symbol
    ///
    /// NOTE: This relies on the assumption that all ops are allocated via the arena, and that all
    /// [Symbol] implementations are ops.
    pub fn as_operation_ref(&self) -> OperationRef {
        unsafe { OperationRef::from_raw(self.as_operation()) }
    }
}

impl Operation {
    /// Returns true if this operation implements [Symbol]
    #[inline]
    pub fn is_symbol(&self) -> bool {
        self.implements::<dyn Symbol>()
    }

    /// Get this operation as a [Symbol], if this operation implements the trait.
    #[inline]
    pub fn as_symbol(&self) -> Option<&dyn Symbol> {
        self.as_trait::<dyn Symbol>()
    }

    /// Returns the nearest [SymbolTable] from this operation.
    ///
    /// Returns `None` if no parent of this operation is a valid symbol table.
    pub fn nearest_symbol_table(&self) -> Option<OperationRef> {
        let mut parent = self.parent_op();
        while let Some(parent_op) = parent.take() {
            let op = parent_op.borrow();
            if op.implements::<dyn SymbolTable>() {
                drop(op);
                return Some(parent_op);
            }
            parent = op.parent_op();
        }
        None
    }

    /// Returns the operation registered with the given symbol name within the closest symbol table
    /// including `self`.
    ///
    /// Returns `None` if the symbol is not found.
    pub fn nearest_symbol(&self, symbol: SymbolName) -> Option<SymbolRef> {
        if let Some(sym) = self.as_symbol() {
            if sym.name() == symbol {
                return Some(unsafe { UnsafeIntrusiveEntityRef::from_raw(sym) });
            }
        }
        let symbol_table_op = self.nearest_symbol_table()?;
        let op = symbol_table_op.borrow();
        let symbol_table = op.as_trait::<dyn SymbolTable>().unwrap();
        symbol_table.get(symbol)
    }

    /// Walks all symbol table operations nested within this operation, including itself.
    ///
    /// For each symbol table operation, the provided callback is invoked with the op and a boolean
    /// signifying if the symbols within that symbol table can be treated as if all uses within the
    /// IR are visible to the caller.
    pub fn walk_symbol_tables<F>(&self, all_symbol_uses_visible: bool, mut callback: F)
    where
        F: FnMut(&dyn Symbol, bool),
    {
        use core::ops::ControlFlow;

        let visitor = move |op: &dyn Symbol| {
            callback(op, all_symbol_uses_visible);
            ControlFlow::<()>::Continue(())
        };

        let op = self.as_operation_ref();
        let mut searcher = Searcher::new(op, visitor);

        searcher.visit();
    }
}

pub type SymbolRef = UnsafeIntrusiveEntityRef<dyn Symbol>;

impl<T> crate::Verify<dyn Symbol> for T
where
    T: Op + Symbol,
{
    fn verify(&self, context: &super::Context) -> Result<(), Report> {
        verify_symbol(self, context)
    }
}

impl crate::Verify<dyn Symbol> for Operation {
    fn should_verify(&self, _context: &super::Context) -> bool {
        self.implements::<dyn Symbol>()
    }

    fn verify(&self, context: &super::Context) -> Result<(), Report> {
        verify_symbol(
            self.as_trait::<dyn Symbol>()
                .expect("this operation does not implement the `Symbol` trait"),
            context,
        )
    }
}

fn verify_symbol(symbol: &dyn Symbol, context: &super::Context) -> Result<(), Report> {
    use midenc_session::diagnostics::{Severity, Spanned};

    // Symbols must either have no parent, or be an immediate child of a SymbolTable
    let op = symbol.as_operation();
    let parent = op.parent_op();
    if !parent.is_none_or(|parent| parent.borrow().implements::<dyn SymbolTable>()) {
        return Err(context
            .session
            .diagnostics
            .diagnostic(Severity::Error)
            .with_message("invalid operation")
            .with_primary_label(op.span(), "expected parent of this operation to be a symbol table")
            .with_help("required due to this operation implementing the 'Symbol' trait")
            .into_report());
    }
    Ok(())
}

pub type SymbolUseRef = UnsafeIntrusiveEntityRef<SymbolUse>;
pub type SymbolUseList = crate::EntityList<SymbolUse>;
pub type SymbolUseIter<'a> = crate::EntityIter<'a, SymbolUse>;
pub type SymbolUseCursor<'a> = crate::EntityCursor<'a, SymbolUse>;
pub type SymbolUseCursorMut<'a> = crate::EntityCursorMut<'a, SymbolUse>;

/// An [OpOperand] represents a use of a [Value] by an [Operation]
pub struct SymbolUse {
    /// The user of the symbol
    pub owner: OperationRef,
    /// The symbol used
    pub symbol: SymbolNameAttr,
}
impl SymbolUse {
    #[inline]
    pub fn new(owner: OperationRef, symbol: SymbolNameAttr) -> Self {
        Self { owner, symbol }
    }
}
impl fmt::Debug for SymbolUse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SymbolUse")
            .field("symbol", &self.symbol)
            .finish_non_exhaustive()
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
