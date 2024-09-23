use alloc::collections::VecDeque;
pub use core::ops::ControlFlow;

use crate::{BlockRef, Op, Operation, OperationRef, Symbol};

/// A generic trait that describes visitors for all kinds
pub trait Visitor<T: ?Sized> {
    /// The type of output produced by visiting an item.
    type Output;

    /// The function which is applied to each `T` as it is visited.
    fn visit(&mut self, current: &T) -> ControlFlow<Self::Output>;
}

/// We can automatically convert any closure of appropriate type to a `Visitor`
impl<T: ?Sized, U, F> Visitor<T> for F
where
    F: FnMut(&T) -> ControlFlow<U>,
{
    type Output = U;

    #[inline]
    fn visit(&mut self, op: &T) -> ControlFlow<Self::Output> {
        self(op)
    }
}

/// Represents a visitor over [Operation]
pub trait OperationVisitor: Visitor<Operation> {}
impl<V> OperationVisitor for V where V: Visitor<Operation> {}

/// Represents a visitor over [Op] of type `T`
pub trait OpVisitor<T: Op>: Visitor<T> {}
impl<T: Op, V> OpVisitor<T> for V where V: Visitor<T> {}

/// Represents a visitor over [Symbol]
pub trait SymbolVisitor: Visitor<dyn Symbol> {}
impl<V> SymbolVisitor for V where V: Visitor<dyn Symbol> {}

/// [Searcher] is a driver for [Visitor] impls as applied to some root [Operation].
///
/// It traverses the objects reachable from the root as follows:
///
/// * The root operation is visited first
/// * Then for each region of the root, the entry block is visited top to bottom, enqueing any nested
///   blocks of those operations to be visited after all blocks of region have been visited. When the
///   entry block has been visited, the process is repeated for the remaining blocks of the region.
/// * When all regions of the root have been visited, and no more blocks remain in the queue, the
///   traversal is complete
///
/// This traversal is _not_ in control flow order, _or_ data flow order, so you should not rely on
/// the order in which operations are visited for your [Visitor] implementation.
pub struct Searcher<V, T: ?Sized> {
    visitor: V,
    queue: VecDeque<BlockRef>,
    current: Option<OperationRef>,
    started: bool,
    _marker: core::marker::PhantomData<T>,
}
impl<T: ?Sized, V: Visitor<T>> Searcher<V, T> {
    pub fn new(root: OperationRef, visitor: V) -> Self {
        Self {
            visitor,
            queue: VecDeque::default(),
            current: Some(root),
            started: false,
            _marker: core::marker::PhantomData,
        }
    }

    #[inline]
    fn next(&mut self) -> Option<OperationRef> {
        visit_next(&mut self.started, &mut self.current, &mut self.queue)
    }
}

impl<V: OperationVisitor> Searcher<V, Operation> {
    pub fn visit(&mut self) -> ControlFlow<<V as Visitor<Operation>>::Output> {
        while let Some(op) = self.next() {
            let op = op.borrow();
            self.visitor.visit(&op)?;
        }

        ControlFlow::Continue(())
    }
}

impl<T: Op, V: OpVisitor<T>> Searcher<V, T> {
    pub fn visit(&mut self) -> ControlFlow<<V as Visitor<T>>::Output> {
        while let Some(op) = self.next() {
            let op = op.borrow();
            if let Some(op) = op.downcast_ref::<T>() {
                self.visitor.visit(op)?;
            }
        }

        ControlFlow::Continue(())
    }
}

impl<V: SymbolVisitor> Searcher<V, dyn Symbol> {
    pub fn visit(&mut self) -> ControlFlow<<V as Visitor<dyn Symbol>>::Output> {
        while let Some(op) = self.next() {
            let op = op.borrow();
            if let Some(op) = op.as_symbol() {
                self.visitor.visit(op)?;
            }
        }

        ControlFlow::Continue(())
    }
}

/// Outlined implementation of the traversal performed by `Searcher`
#[inline(never)]
fn visit_next(
    started: &mut bool,
    current: &mut Option<OperationRef>,
    queue: &mut VecDeque<BlockRef>,
) -> Option<OperationRef> {
    if !*started {
        *started = true;
        let curr = current.take()?;
        // When just starting, we're at the root, so we descend into the operation, rather
        // than visiting its next sibling.
        {
            let op = curr.borrow();
            for region in op.regions().iter() {
                let mut cursor = region.body().front();
                if current.is_none() {
                    let entry = cursor.as_pointer().expect("invalid region: has no entry block");
                    let entry = entry.borrow();
                    let next = entry.body().front().as_pointer();
                    *current = next;
                    cursor.move_next();
                }
                while let Some(block) = cursor.as_pointer() {
                    queue.push_back(block);
                    cursor.move_next();
                }
            }
        }
        return Some(curr);
    }

    // Here, we've already visited the root operation, so one of the following is true:
    //
    // * `current` is `None`, so pop the next block from the queue, if there are no more blocks,
    //   then we're done visiting and can return `None`. If there is a block, then we set
    //   `current` to the first operation in that block, and retry
    // * `current` is `Some`, so obtain the next value of `current` by obtaining the next
    //   sibling operation of the current operation.
    while current.is_none() {
        let block = queue.pop_front()?;
        let block = block.borrow();
        *current = block.body().front().as_pointer();
    }

    let next = current.as_ref().and_then(|curr| curr.next());

    core::mem::replace(current, next)
}
