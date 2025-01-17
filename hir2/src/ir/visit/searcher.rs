use super::{OpVisitor, OperationVisitor, SymbolVisitor, Visitor, WalkResult, Walkable};
use crate::{Op, Operation, OperationRef, Symbol};

/// [Searcher] is a driver for [Visitor] impls as applied to some root [Operation].
///
/// The searcher traverses the object graph in depth-first preorder, from operations to regions to
/// blocks to operations, etc. All nested items of an entity are visited before its siblings, i.e.
/// a region is fully visited before the next region of the same containing operation.
///
/// This is effectively control-flow order, from an abstract interpretation perspective, i.e. an
/// actual program might only execute one region of a multi-region op, but this traversal will visit
/// all of them unless otherwise directed by a `WalkResult`.
pub struct Searcher<V, T: ?Sized> {
    visitor: V,
    root: OperationRef,
    _marker: core::marker::PhantomData<T>,
}
impl<T: ?Sized, V: Visitor<T>> Searcher<V, T> {
    pub fn new(root: OperationRef, visitor: V) -> Self {
        Self {
            visitor,
            root,
            _marker: core::marker::PhantomData,
        }
    }
}

impl<V: OperationVisitor> Searcher<V, Operation> {
    pub fn visit(&mut self) -> WalkResult<<V as Visitor<Operation>>::Output> {
        self.root.borrow().prewalk_interruptible(|op: OperationRef| {
            let op = op.borrow();
            self.visitor.visit(&op)
        })
    }
}

impl<T: Op, V: OpVisitor<T>> Searcher<V, T> {
    pub fn visit(&mut self) -> WalkResult<<V as Visitor<T>>::Output> {
        self.root.borrow().prewalk_interruptible(|op: OperationRef| {
            let op = op.borrow();
            if let Some(op) = op.downcast_ref::<T>() {
                self.visitor.visit(op)
            } else {
                WalkResult::Continue(())
            }
        })
    }
}

impl<V: SymbolVisitor> Searcher<V, dyn Symbol> {
    pub fn visit(&mut self) -> WalkResult<<V as Visitor<dyn Symbol>>::Output> {
        self.root.borrow().prewalk_interruptible(|op: OperationRef| {
            let op = op.borrow();
            if let Some(sym) = op.as_symbol() {
                self.visitor.visit(sym)
            } else {
                WalkResult::Continue(())
            }
        })
    }
}
