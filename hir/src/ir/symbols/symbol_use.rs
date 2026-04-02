use alloc::collections::VecDeque;
use core::fmt;

use super::{SymbolPath, SymbolRef};
use crate::{
    Entity, EntityListItem, OperationRef, SymbolTable, UnsafeIntrusiveEntityRef,
    dialects::builtin::attributes::SymbolRefAttr,
};

pub type SymbolUseRef = UnsafeIntrusiveEntityRef<SymbolUse>;
pub type SymbolUseList = crate::EntityList<SymbolUse>;
pub type SymbolUseIter<'a> = crate::EntityListIter<'a, SymbolUse>;
pub type SymbolUseCursor<'a> = crate::EntityListCursor<'a, SymbolUse>;
pub type SymbolUseCursorMut<'a> = crate::EntityListCursorMut<'a, SymbolUse>;

/// A [SymbolUse] represents a use of a [super::Symbol] by an [crate::Operation]
#[derive(Copy, Clone)]
pub struct SymbolUse {
    /// The user of the symbol
    pub owner: OperationRef,
    /// The symbol attribute of the op that stores the symbol
    pub attr: UnsafeIntrusiveEntityRef<SymbolRefAttr>,
    /// The symbol currently linked to this use, if any.
    pub used: Option<SymbolRef>,
}

impl SymbolUse {
    #[inline]
    pub fn new(owner: OperationRef, attr: UnsafeIntrusiveEntityRef<SymbolRefAttr>) -> Self {
        Self {
            owner,
            attr,
            used: None,
        }
    }

    /// Returns the symbol attribute that owns this use.
    pub fn symbol(&self) -> UnsafeIntrusiveEntityRef<SymbolRefAttr> {
        self.attr
    }

    /// Returns the symbol currently linked to this use, if any.
    #[inline(always)]
    pub fn used_symbol(&self) -> Option<SymbolRef> {
        self.used
    }

    /// Updates the symbol currently linked to this use.
    #[inline]
    pub fn set_used_symbol(&mut self, symbol: Option<SymbolRef>) {
        self.used = symbol;
    }

    /// Resolves the referenced symbol relative to the owning operation of this use.
    pub fn resolve_symbol(&self, path: &SymbolPath) -> Option<SymbolRef> {
        let symbol_table = {
            let owner = self.owner.borrow();
            owner
                .nearest_symbol_table()
                .or_else(|| owner.implements::<dyn SymbolTable>().then_some(self.owner))
        }?;
        let symbol_table = symbol_table.borrow();
        symbol_table.as_symbol_table()?.resolve(path)
    }

    /// Unlinks this use from its current symbol, if any.
    pub fn unlink_from_symbol(
        &mut self,
        user: SymbolUseRef,
        path: &SymbolPath,
    ) -> Option<SymbolRef> {
        if !user.is_linked() {
            return self.used.take();
        }

        let mut symbol = self
            .used
            .or_else(|| self.resolve_symbol(path))
            .expect("linked symbol uses must track or resolve their target symbol");
        unsafe {
            symbol.borrow_mut().uses_mut().cursor_mut_from_ptr(user).remove();
        }
        self.used = None;

        Some(symbol)
    }

    /// Links this use to `symbol`.
    pub fn link_to_symbol(&mut self, user: SymbolUseRef, mut symbol: SymbolRef) {
        debug_assert!(
            !user.is_linked(),
            "symbol use must be unlinked before it can be linked again"
        );
        symbol.borrow_mut().insert_use(user);
        self.used = Some(symbol);
    }
}

impl fmt::Debug for SymbolUse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = self.attr.borrow();
        f.debug_struct("SymbolUse")
            .field("symbol", &value.path())
            .finish_non_exhaustive()
    }
}

impl Entity for SymbolUse {}
impl EntityListItem for SymbolUse {}

/// An iterator over [SymbolUse] which owns the collection it iterates over.
///
/// This is primarily used in contexts where the set of symbol uses is being gathered from many
/// places, and thus [SymbolUseIter] is not able to be used.
pub struct SymbolUsesIter {
    items: VecDeque<SymbolUse>,
}
impl SymbolUsesIter {
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}
impl ExactSizeIterator for SymbolUsesIter {
    #[inline(always)]
    fn len(&self) -> usize {
        self.items.len()
    }
}
impl From<VecDeque<SymbolUse>> for SymbolUsesIter {
    fn from(items: VecDeque<SymbolUse>) -> Self {
        Self { items }
    }
}
impl FromIterator<SymbolUseRef> for SymbolUsesIter {
    fn from_iter<T: IntoIterator<Item = SymbolUseRef>>(iter: T) -> Self {
        Self {
            items: iter.into_iter().map(|user| *user.borrow()).collect(),
        }
    }
}
impl core::iter::FusedIterator for SymbolUsesIter {}
impl Iterator for SymbolUsesIter {
    type Item = SymbolUse;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        self.items.pop_front()
    }
}

/// An iterator over [SymbolUseRef] which owns the collection it iterates over.
///
/// This is primarily used in contexts where the set of symbol uses is being gathered from many
/// places, and thus [SymbolUseIter] is not able to be used.
pub struct SymbolUseRefsIter {
    items: VecDeque<SymbolUseRef>,
}
impl SymbolUseRefsIter {
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}
impl ExactSizeIterator for SymbolUseRefsIter {
    #[inline(always)]
    fn len(&self) -> usize {
        self.items.len()
    }
}
impl From<VecDeque<SymbolUseRef>> for SymbolUseRefsIter {
    fn from(items: VecDeque<SymbolUseRef>) -> Self {
        Self { items }
    }
}
impl FromIterator<SymbolUseRef> for SymbolUseRefsIter {
    fn from_iter<T: IntoIterator<Item = SymbolUseRef>>(iter: T) -> Self {
        Self {
            items: iter.into_iter().collect(),
        }
    }
}
impl core::iter::FusedIterator for SymbolUseRefsIter {}
impl Iterator for SymbolUseRefsIter {
    type Item = SymbolUseRef;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        self.items.pop_front()
    }
}
