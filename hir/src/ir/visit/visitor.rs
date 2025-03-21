use super::WalkResult;
use crate::{Op, Operation, Symbol};

/// A generic trait that describes visitors for all kinds
pub trait Visitor<T: ?Sized> {
    /// The type of output produced by visiting an item.
    type Output;

    /// The function which is applied to each `T` as it is visited.
    fn visit(&mut self, current: &T) -> WalkResult<Self::Output>;
}

/// We can automatically convert any closure of appropriate type to a `Visitor`
impl<T: ?Sized, U, F> Visitor<T> for F
where
    F: FnMut(&T) -> WalkResult<U>,
{
    type Output = U;

    #[inline]
    fn visit(&mut self, op: &T) -> WalkResult<Self::Output> {
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
