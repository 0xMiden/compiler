use alloc::collections::VecDeque;
use core::fmt;

use smallvec::SmallVec;

use super::{
    SymbolName, SymbolNameAttr, SymbolNameComponents, SymbolTable, SymbolUse, SymbolUseRef,
    SymbolUsesIter,
};
use crate::{
    Op, Operation, OperationRef, RegionRef, Report, UnsafeIntrusiveEntityRef, Usable, Visibility,
};

pub type SymbolRef = UnsafeIntrusiveEntityRef<dyn Symbol>;

impl fmt::Debug for SymbolRef {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        fmt::Display::fmt(self, f)
    }
}
impl fmt::Display for SymbolRef {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", &self.borrow().name())
    }
}

/// A [Symbol] is an IR entity with an associated _symbol_, or name, which is expected to be unique
/// amongst all other symbols in the same namespace.
///
/// For example, functions are named, and are expected to be unique within the same module,
/// otherwise it would not be possible to unambiguously refer to a function by name. Likewise
/// with modules in a program, etc.
pub trait Symbol: Usable<Use = SymbolUse> + 'static {
    fn as_symbol_operation(&self) -> &Operation;
    fn as_symbol_operation_mut(&mut self) -> &mut Operation;
    /// Get the name of this symbol
    fn name(&self) -> SymbolName;
    /// Get an iterator over the components of the fully-qualified path of this symbol.
    fn components(&self) -> SymbolNameComponents {
        let mut parts = VecDeque::default();
        if let Some(symbol_table) = self.root_symbol_table() {
            let symbol_table = symbol_table.borrow();
            symbol_table.walk_symbol_tables(true, |symbol_table, _| {
                if let Some(sym) = symbol_table.as_symbol_table_operation().as_symbol() {
                    parts.push_back(sym.name().as_str());
                }
            });
        }
        SymbolNameComponents::from_raw_parts(parts, self.name())
    }
    /// Set the name of this symbol
    fn set_name(&mut self, name: SymbolName);
    /// Get the visibility of this symbol
    fn visibility(&self) -> Visibility;
    /// Returns true if this symbol has private visibility
    #[inline]
    fn is_private(&self) -> bool {
        self.visibility().is_private()
    }
    /// Returns true if this symbol has public visibility
    #[inline]
    fn is_public(&self) -> bool {
        self.visibility().is_public()
    }
    /// Sets the visibility of this symbol
    fn set_visibility(&mut self, visibility: Visibility);
    /// Sets the visibility of this symbol to private
    fn set_private(&mut self) {
        self.set_visibility(Visibility::Private);
    }
    /// Sets the visibility of this symbol to internal
    fn set_internal(&mut self) {
        self.set_visibility(Visibility::Internal);
    }
    /// Sets the visibility of this symbol to public
    fn set_public(&mut self) {
        self.set_visibility(Visibility::Public);
    }
    /// Get all of the uses of this symbol that are nested within `from`
    fn symbol_uses_in(&self, from: OperationRef) -> SymbolUsesIter {
        let mut uses = VecDeque::default();
        let from = from.borrow();
        for user in self.iter_uses() {
            if from.is_ancestor_of(&user.owner.borrow()) {
                uses.push_back(unsafe { SymbolUseRef::from_raw(&*user) });
            }
        }
        SymbolUsesIter::from(uses)
    }
    /// Get all of the uses of this symbol that are nested within `from`
    fn symbol_uses_in_region(&self, from: RegionRef) -> SymbolUsesIter {
        let mut uses = VecDeque::default();
        let from = from.borrow();

        // Filter the set of uses we wish to match to only those that occur within the parent op
        // of `from`, as all other uses cannot, by definition, be in `from`.
        let from_op = from.parent().unwrap();
        let from_op = from_op.borrow();
        let mut scoped_uses = self
            .iter_uses()
            .filter(|user| from_op.is_ancestor_of(&user.owner.borrow()))
            .collect::<SmallVec<[_; 8]>>();

        // Don't bother looking in `from` if there aren't any uses to begin with
        if scoped_uses.is_empty() {
            return SymbolUsesIter::from(uses);
        }

        // Visit the body of `from`, to determine which of the uses of this symbol that belong to
        // the parent operation of `from`, occur in the `from` region itself.
        for block in from.body() {
            for op in block.body() {
                // Find all uses of `self` which occur within `op`, and add them to the result set,
                // while also removing them from the set of uses to match against, reducing the
                // work needed by future iterations.
                scoped_uses.retain(|user| {
                    if op.is_ancestor_of(&user.owner.borrow()) {
                        uses.push_back(unsafe { SymbolUseRef::from_raw(&**user) });
                        false
                    } else {
                        true
                    }
                });

                // If there are no more uses remaining, we're done, and can stop searching
                if scoped_uses.is_empty() {
                    return SymbolUsesIter::from(uses);
                }
            }
        }

        SymbolUsesIter::from(uses)
    }
    /// Return true if there are no uses of this symbol nested within `from`
    fn symbol_uses_known_empty(&self, from: OperationRef) -> bool {
        let from = from.borrow();
        !self.iter_uses().any(|user| from.is_ancestor_of(&user.owner.borrow()))
    }
    /// Attempt to replace all uses of this symbol nested within `from`, with the provided replacement
    fn replace_all_uses(
        &mut self,
        replacement: SymbolRef,
        from: OperationRef,
    ) -> Result<(), Report> {
        for symbol_use in self.symbol_uses_in(from) {
            let (mut owner, attr_name) = {
                let user = symbol_use.borrow();
                (user.owner.clone(), user.attr)
            };
            let mut owner = owner.borrow_mut();
            // Unlink previously used symbol
            {
                let current_symbol = owner
                    .get_typed_attribute_mut::<SymbolNameAttr>(attr_name)
                    .expect("stale symbol user");
                unsafe {
                    self.uses_mut().cursor_mut_from_ptr(current_symbol.user.clone()).remove();
                }
            }
            // Link replacement symbol
            owner.set_symbol_attribute(attr_name, replacement.clone());
        }

        Ok(())
    }
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
    /// Return the root symbol table in which this symbol is contained, if one exists.
    ///
    /// The root symbol table does not necessarily know about this symbol, rather the symbol table
    /// which "owns" this symbol may itself be a symbol that belongs to another symbol table. This
    /// function traces this chain as far as it goes, and returns the highest ancestor in the tree.
    fn root_symbol_table(&self) -> Option<OperationRef> {
        self.as_symbol_operation().root_symbol_table()
    }
}

impl dyn Symbol {
    pub fn is<T: Op + Symbol>(&self) -> bool {
        let op = self.as_symbol_operation();
        op.is::<T>()
    }

    pub fn downcast_ref<T: Op + Symbol>(&self) -> Option<&T> {
        let op = self.as_symbol_operation();
        op.downcast_ref::<T>()
    }

    pub fn downcast_mut<T: Op + Symbol>(&mut self) -> Option<&mut T> {
        let op = self.as_symbol_operation_mut();
        op.downcast_mut::<T>()
    }

    /// Get an [OperationRef] for the operation underlying this symbol
    ///
    /// NOTE: This relies on the assumption that all ops are allocated via the arena, and that all
    /// [Symbol] implementations are ops.
    pub fn as_operation_ref(&self) -> OperationRef {
        self.as_symbol_operation().as_operation_ref()
    }
}

impl<T> crate::Verify<dyn Symbol> for T
where
    T: Op + Symbol,
{
    fn verify(&self, context: &crate::Context) -> Result<(), Report> {
        verify_symbol(self, context)
    }
}

impl crate::Verify<dyn Symbol> for Operation {
    fn should_verify(&self, _context: &crate::Context) -> bool {
        self.implements::<dyn Symbol>()
    }

    fn verify(&self, context: &crate::Context) -> Result<(), Report> {
        verify_symbol(
            self.as_trait::<dyn Symbol>()
                .expect("this operation does not implement the `Symbol` trait"),
            context,
        )
    }
}

fn verify_symbol(symbol: &dyn Symbol, context: &crate::Context) -> Result<(), Report> {
    use midenc_session::diagnostics::{Severity, Spanned};

    // Symbols must either have no parent, or be an immediate child of a SymbolTable
    let op = symbol.as_symbol_operation();
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
