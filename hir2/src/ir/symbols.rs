mod name;
mod symbol;
mod symbol_use;
mod table;

use alloc::collections::VecDeque;

use midenc_session::diagnostics::{miette, Diagnostic};
use smallvec::SmallVec;

pub use self::{
    name::*,
    symbol::{Symbol, SymbolRef},
    symbol_use::*,
    table::*,
};
use super::{Region, RegionRef, WalkResult};
use crate::{Operation, OperationRef, UnsafeIntrusiveEntityRef, Walkable};

#[derive(Debug, thiserror::Error, Diagnostic)]
pub enum InvalidSymbolRefError {
    #[error("invalid symbol reference: no symbol table available")]
    NoSymbolTable {
        #[label("cannot resolve this symbol")]
        symbol: crate::SourceSpan,
        #[label(
            "because this operation has no parent symbol table with which to resolve the reference"
        )]
        user: crate::SourceSpan,
    },
    #[error("invalid symbol reference: undefined symbol")]
    UnknownSymbol {
        #[label("failed to resolve this symbol")]
        symbol: crate::SourceSpan,
        #[label("in the nearest symbol table from this operation")]
        user: crate::SourceSpan,
    },
    #[error("invalid symbol reference: undefined component '{component}' of symbol")]
    UnknownSymbolComponent {
        #[label("failed to resolve this symbol")]
        symbol: crate::SourceSpan,
        #[label("from the root symbol table of this operation")]
        user: crate::SourceSpan,
        component: &'static str,
    },
    #[error("invalid symbol reference: expected callable")]
    NotCallable {
        #[label("expected this symbol to implement the CallableOpInterface")]
        symbol: crate::SourceSpan,
    },
    #[error("invalid symbol reference: symbol is not the correct type")]
    InvalidType {
        #[label(
            "expected this symbol to be a '{expected}', but symbol referenced a '{got}' operation"
        )]
        symbol: crate::SourceSpan,
        expected: &'static str,
        got: crate::OperationName,
    },
}

/// A trait which allows multiple types to be coerced into a [SymbolRef].
///
/// This is primarily intended for use in operation builders.
pub trait AsSymbolRef {
    fn as_symbol_ref(&self) -> SymbolRef;
}
impl<T: Symbol> AsSymbolRef for &T {
    #[inline]
    fn as_symbol_ref(&self) -> SymbolRef {
        unsafe { SymbolRef::from_raw(*self as &dyn Symbol) }
    }
}
impl<T: Symbol> AsSymbolRef for UnsafeIntrusiveEntityRef<T> {
    #[inline]
    fn as_symbol_ref(&self) -> SymbolRef {
        let t_ptr = Self::as_ptr(self);
        unsafe { SymbolRef::from_raw(t_ptr as *const dyn Symbol) }
    }
}
impl AsSymbolRef for SymbolRef {
    #[inline(always)]
    fn as_symbol_ref(&self) -> SymbolRef {
        Self::clone(self)
    }
}

impl Operation {
    /// Returns true if this operation implements [Symbol]
    #[inline]
    pub fn is_symbol(&self) -> bool {
        self.implements::<dyn Symbol>()
    }

    /// Returns the symbol name of this operation, if it implements [Symbol]
    pub fn symbol_name_if_symbol(&self) -> Option<SymbolName> {
        self.as_symbol().map(|symbol| symbol.name())
    }

    /// Get this operation as a [Symbol], if this operation implements the trait.
    #[inline]
    pub fn as_symbol(&self) -> Option<&dyn Symbol> {
        self.as_trait::<dyn Symbol>()
    }

    /// Get this operation as a [SymbolTable], if this operation implements the trait.
    #[inline]
    pub fn as_symbol_table(&self) -> Option<&dyn SymbolTable> {
        self.as_trait::<dyn SymbolTable>()
    }

    /// Return the root symbol table in which this symbol is contained, if one exists.
    ///
    /// The root symbol table does not necessarily know about this symbol, rather the symbol table
    /// which "owns" this symbol may itself be a symbol that belongs to another symbol table. This
    /// function traces this chain as far as it goes, and returns the highest ancestor in the tree.
    pub fn root_symbol_table(&self) -> Option<OperationRef> {
        let mut parent = self.nearest_symbol_table();
        let mut found = None;
        while let Some(nearest_symbol_table) = parent.take() {
            found = Some(nearest_symbol_table.clone());
            parent = nearest_symbol_table.borrow().nearest_symbol_table();
        }
        found
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
        F: FnMut(&dyn SymbolTable, bool),
    {
        self.prewalk(|op: OperationRef| {
            let op = op.borrow();
            if let Some(sym) = op.as_symbol_table() {
                callback(sym, all_symbol_uses_visible);
            }
        });
    }

    /// Walk all of the operations nested under, and including this operation, without traversing
    /// into any nested symbol tables (including this operation, if it is a symbol table).
    ///
    /// Stops walking if the result of the callback is anything other than `WalkResult::Continue`.
    pub fn walk_symbol_table<F>(&self, mut callback: F) -> WalkResult
    where
        F: FnMut(&Operation) -> WalkResult,
    {
        callback(self)?;
        if self.implements::<dyn SymbolTable>() {
            return WalkResult::Continue(());
        }

        for region in self.regions() {
            Self::walk_symbol_table_region(&region, &mut callback)?;
        }

        WalkResult::Continue(())
    }

    /// Walk all of the operations within the given set of regions, without traversing into any
    /// nested symbol tables. If `WalkResult::Skip` is returned for an op, none of that op's regions
    /// will be visited.
    pub fn walk_symbol_table_region<F>(region: &Region, mut callback: F) -> WalkResult
    where
        F: FnMut(&Operation) -> WalkResult,
    {
        let mut regions = SmallVec::<[RegionRef; 4]>::from_iter([region.as_region_ref()]);
        while let Some(region) = regions.pop() {
            let region = region.borrow();
            for block in region.body() {
                for op in block.body() {
                    match callback(&op) {
                        WalkResult::Continue(_) => {
                            // If this op defines a new symbol table scope, we can't traverse. Any symbol
                            // references nested within this op are different semantically.
                            if !op.implements::<dyn SymbolTable>() {
                                regions.extend(op.regions().iter().map(|r| r.as_region_ref()));
                            }
                        }
                        err @ WalkResult::Break(_) => return err,
                        WalkResult::Skip => (),
                    }
                }
            }
        }

        WalkResult::Continue(())
    }

    /// Walk all of the uses, for any symbol, that are nested within this operation, invoking the
    /// provided callback for each use.
    ///
    /// This does not traverse into any nested symbol tables.
    pub fn walk_symbol_uses<F>(&self, mut callback: F) -> WalkResult
    where
        F: FnMut(&SymbolUse) -> WalkResult,
    {
        // Walk the uses on this operation.
        Self::walk_symbol_refs(self, &mut callback)?;

        // Only recurse if this operation is not a symbol table. A symbol table defines a new scope,
        // so we can't walk the attributes from within the symbol table op.
        if !self.implements::<dyn SymbolTable>() {
            for region in self.regions() {
                Self::walk_symbol_table_region(&region, |op| {
                    Self::walk_symbol_refs(op, &mut callback)
                })?;
            }
        }

        WalkResult::Continue(())
    }

    /// Walk all of the uses, for any symbol, that are nested within the given region, invoking the
    /// provided callback for each use.
    ///
    /// This does not traverse into any nested symbol tables.
    pub fn walk_symbol_uses_in_region<F>(from: &Region, mut callback: F) -> WalkResult
    where
        F: FnMut(&SymbolUse) -> WalkResult,
    {
        Self::walk_symbol_table_region(from, |op| Self::walk_symbol_refs(op, &mut callback))
    }

    /// Get an iterator over all of the uses, for any symbol, that are nested within the given
    /// operation 'from'.
    ///
    /// This does not traverse into any nested symbol tables, and will also only return uses on
    /// 'from' if it does not also define a symbol table. This is because we treat the region as the
    /// boundary of the symbol table, and not the op itself.
    pub fn all_symbol_uses(&self) -> SymbolUsesIter {
        let mut uses = VecDeque::new();
        if self.implements::<dyn SymbolTable>() {
            return SymbolUsesIter::from(uses);
        }
        let _ = Self::walk_symbol_refs(self, |symbol_use| {
            // SAFETY: The symbol use reference given to us is borrowed from a `SymbolUseRef`,
            // so recovering the original `SymbolUseRef` is always valid.
            uses.push_back(unsafe { SymbolUseRef::from_raw(symbol_use) });
            WalkResult::Continue(())
        });
        for region in self.regions() {
            let _ = Self::walk_symbol_uses_in_region(&region, |symbol_use| {
                // SAFETY: Same as above
                uses.push_back(unsafe { SymbolUseRef::from_raw(symbol_use) });
                WalkResult::Continue(())
            });
        }
        SymbolUsesIter::from(uses)
    }

    /// Get an iterator over all of the uses, for any symbol, that are nested within the given
    /// region 'from'.
    ///
    /// This does not traverse into any nested symbol tables.
    pub fn all_symbol_uses_in_region(from: &Region) -> SymbolUsesIter {
        let mut uses = VecDeque::new();
        let _ = Self::walk_symbol_uses_in_region(from, |symbol_use| {
            // SAFETY: The symbol use reference given to us is borrowed from a `SymbolUseRef`,
            // so recovering the original `SymbolUseRef` is always valid.
            uses.push_back(unsafe { SymbolUseRef::from_raw(symbol_use) });
            WalkResult::Continue(())
        });
        SymbolUsesIter::from(uses)
    }

    /// Walk all of the symbol references within the given operation, invoking the provided callback
    /// for each found use.
    ///
    /// The callbacks takes the symbol use.
    pub fn walk_symbol_refs<F>(op: &Operation, mut callback: F) -> WalkResult
    where
        F: FnMut(&SymbolUse) -> WalkResult,
    {
        for attr in op.attrs.iter() {
            if let Some(symbol_name_attr) = attr.value_as::<SymbolNameAttr>() {
                callback(&symbol_name_attr.user.borrow())?;
            }
        }

        WalkResult::Continue(())
    }
}
