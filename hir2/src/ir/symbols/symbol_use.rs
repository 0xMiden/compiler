use alloc::collections::VecDeque;
use core::fmt;

use super::SymbolPathAttr;
use crate::{Entity, EntityListItem, EntityRef, OperationRef, UnsafeIntrusiveEntityRef};

pub type SymbolUseRef = UnsafeIntrusiveEntityRef<SymbolUse>;
pub type SymbolUseList = crate::EntityList<SymbolUse>;
pub type SymbolUseIter<'a> = crate::EntityIter<'a, SymbolUse>;
pub type SymbolUseCursor<'a> = crate::EntityCursor<'a, SymbolUse>;
pub type SymbolUseCursorMut<'a> = crate::EntityCursorMut<'a, SymbolUse>;

/// A [SymbolUse] represents a use of a [Symbol] by an [Operation]
#[derive(Copy, Clone)]
pub struct SymbolUse {
    /// The user of the symbol
    pub owner: OperationRef,
    /// The symbol attribute of the op that stores the symbol
    pub attr: crate::interner::Symbol,
}
impl SymbolUse {
    #[inline]
    pub fn new(owner: OperationRef, symbol: crate::interner::Symbol) -> Self {
        Self {
            owner,
            attr: symbol,
        }
    }

    pub fn symbol(&self) -> EntityRef<'_, SymbolPathAttr> {
        EntityRef::map(self.owner.borrow(), |owner| {
            owner.get_typed_attribute::<SymbolPathAttr>(self.attr).expect("expected symbol")
        })
    }
}
impl fmt::Debug for SymbolUse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let op = self.owner.borrow();
        let value = op.get_typed_attribute::<SymbolPathAttr>(self.attr);
        f.debug_struct("SymbolUse")
            .field("attr", &self.attr)
            .field("symbol", &value.as_ref().map(|value| &value.path))
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
