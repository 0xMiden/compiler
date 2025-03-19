use alloc::collections::VecDeque;
use core::fmt;

use smallvec::SmallVec;

use super::{
    SymbolName, SymbolNameComponent, SymbolPath, SymbolTable, SymbolUse, SymbolUseRefsIter,
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
    /// Get the fully-qualified (absolute) path of this symbol
    ///
    /// # Panics
    ///
    /// This function traverses the parents of this operation to the top level. If called while
    /// mutably borrowing any of the ancestors of this operation, a panic will occur.
    fn path(&self) -> SymbolPath {
        let mut parts = VecDeque::from_iter([SymbolNameComponent::Leaf(self.name())]);
        let mut symbol_table = self.as_symbol_operation().nearest_symbol_table();

        while let Some(parent_symbol_table) = symbol_table.take() {
            let sym_table_op = parent_symbol_table.borrow();
            if let Some(sym) = sym_table_op.as_symbol() {
                parts.push_front(SymbolNameComponent::Component(sym.name()));
                symbol_table = sym_table_op.nearest_symbol_table();
            } else {
                // This is an anonymous symbol table - for now we require all symbol tables to be
                // symbols unless it is the root symbol table
                assert!(
                    sym_table_op.parent_op().is_none(),
                    "anonymous symbol tables cannot have parents"
                );
            }
        }

        parts.push_front(SymbolNameComponent::Root);

        SymbolPath::from_iter(parts)
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
    fn symbol_uses_in(&self, from: OperationRef) -> SymbolUseRefsIter {
        let mut uses = VecDeque::default();
        let from = from.borrow();
        let mut cursor = self.first_use();
        while let Some(user) = cursor.as_pointer() {
            let owner = user.borrow().owner;
            if from.is_ancestor_of(&owner.borrow()) {
                uses.push_back(user);
            }
            cursor.move_next();
        }
        SymbolUseRefsIter::from(uses)
    }
    /// Get all of the uses of this symbol that are nested within `from`
    fn symbol_uses_in_region(&self, from: RegionRef) -> SymbolUseRefsIter {
        let mut uses = VecDeque::default();
        let from = from.borrow();

        // Filter the set of uses we wish to match to only those that occur within the parent op
        // of `from`, as all other uses cannot, by definition, be in `from`.
        let from_op = from.parent().unwrap();
        let from_op = from_op.borrow();
        let mut scoped_uses = SmallVec::<[_; 8]>::default();
        {
            let mut cursor = self.first_use();
            while let Some(user) = cursor.as_pointer() {
                let owner = user.borrow().owner;
                if from_op.is_ancestor_of(&owner.borrow()) {
                    scoped_uses.push((owner, user));
                }
                cursor.move_next();
            }
        }

        // Don't bother looking in `from` if there aren't any uses to begin with
        if scoped_uses.is_empty() {
            return SymbolUseRefsIter::from(uses);
        }

        // Visit the body of `from`, to determine which of the uses of this symbol that belong to
        // the parent operation of `from`, occur in the `from` region itself.
        for block in from.body() {
            for op in block.body() {
                // Find all uses of `self` which occur within `op`, and add them to the result set,
                // while also removing them from the set of uses to match against, reducing the
                // work needed by future iterations.
                scoped_uses.retain(|(owner, user)| {
                    if op.is_ancestor_of(&owner.borrow()) {
                        uses.push_back(*user);
                        false
                    } else {
                        true
                    }
                });

                // If there are no more uses remaining, we're done, and can stop searching
                if scoped_uses.is_empty() {
                    return SymbolUseRefsIter::from(uses);
                }
            }
        }

        SymbolUseRefsIter::from(uses)
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
        for user in self.symbol_uses_in(from) {
            let SymbolUse { mut owner, attr } = *user.borrow();
            let mut owner = owner.borrow_mut();
            // Unlink previously used symbol
            unsafe {
                self.uses_mut().cursor_mut_from_ptr(user).remove();
            }
            // Link replacement symbol
            owner.set_symbol_attribute(attr, replacement);
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
            .diagnostics()
            .diagnostic(Severity::Error)
            .with_message(::alloc::format!("invalid operation {}", op.name()))
            .with_primary_label(op.span(), "expected parent of this operation to be a symbol table")
            .with_help("required due to this operation implementing the 'Symbol' trait")
            .into_report());
    }
    Ok(())
}
