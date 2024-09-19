use downcast_rs::{impl_downcast, Downcast};

use super::*;

pub trait Op: Downcast + OpVerifier {
    /// The name of this operation's opcode
    ///
    /// The opcode must be distinct from all other opcodes in the same dialect
    fn name(&self) -> OperationName;
    fn as_operation(&self) -> &Operation;
    fn as_operation_mut(&mut self) -> &mut Operation;

    fn parent(&self) -> Option<BlockRef> {
        self.as_operation().parent()
    }
    fn parent_region(&self) -> Option<RegionRef> {
        self.as_operation().parent_region()
    }
    fn parent_op(&self) -> Option<OperationRef> {
        self.as_operation().parent_op()
    }
    fn regions(&self) -> &RegionList {
        &self.as_operation().regions
    }
    fn operands(&self) -> &[OpOperand] {
        self.as_operation().operands.as_slice()
    }
    fn results(&self) -> &[OpResultRef] {
        self.as_operation().results.as_slice()
    }
    fn successors(&self) -> &[OpSuccessor] {
        self.as_operation().successors.as_slice()
    }
}

impl_downcast!(Op);

impl Spanned for dyn Op {
    fn span(&self) -> SourceSpan {
        self.as_operation().span
    }
}

pub trait OpExt {
    /// Return the value associated with attribute `name` for this function
    fn get_attribute<Q>(&self, name: &Q) -> Option<&dyn AttributeValue>
    where
        interner::Symbol: std::borrow::Borrow<Q>,
        Q: Ord + ?Sized;

    /// Return true if this function has an attributed named `name`
    fn has_attribute<Q>(&self, name: &Q) -> bool
    where
        interner::Symbol: std::borrow::Borrow<Q>,
        Q: Ord + ?Sized;

    /// Set the attribute `name` with `value` for this function.
    fn set_attribute(
        &mut self,
        name: impl Into<interner::Symbol>,
        value: Option<impl AttributeValue>,
    );

    /// Remove any attribute with the given name from this function
    fn remove_attribute<Q>(&mut self, name: &Q)
    where
        interner::Symbol: std::borrow::Borrow<Q>,
        Q: Ord + ?Sized;

    /// Returns a handle to the nearest containing [Operation] of type `T` for this operation, if it
    /// is attached to one
    fn nearest_parent_op<T: Op>(&self) -> Option<UnsafeIntrusiveEntityRef<T>>;
}

impl<T: ?Sized + Op> OpExt for T {
    #[inline]
    fn get_attribute<Q>(&self, name: &Q) -> Option<&dyn AttributeValue>
    where
        interner::Symbol: std::borrow::Borrow<Q>,
        Q: Ord + ?Sized,
    {
        self.as_operation().get_attribute(name)
    }

    #[inline]
    fn has_attribute<Q>(&self, name: &Q) -> bool
    where
        interner::Symbol: std::borrow::Borrow<Q>,
        Q: Ord + ?Sized,
    {
        self.as_operation().has_attribute(name)
    }

    #[inline]
    fn set_attribute(
        &mut self,
        name: impl Into<interner::Symbol>,
        value: Option<impl AttributeValue>,
    ) {
        self.as_operation_mut().set_attribute(name, value);
    }

    #[inline]
    fn remove_attribute<Q>(&mut self, name: &Q)
    where
        interner::Symbol: std::borrow::Borrow<Q>,
        Q: Ord + ?Sized,
    {
        self.as_operation_mut().remove_attribute(name);
    }

    #[inline]
    fn nearest_parent_op<U: Op>(&self) -> Option<UnsafeIntrusiveEntityRef<U>> {
        self.as_operation().nearest_parent_op()
    }
}
