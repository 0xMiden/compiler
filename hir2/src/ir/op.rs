use super::*;
use crate::{any::AsAny, AttributeValue};

pub trait OpRegistration: Op {
    fn name() -> ::midenc_hir_symbol::Symbol;
}

pub trait BuildableOp<Args: core::marker::Tuple>: Op {
    type Builder<'a, T>: FnOnce<Args, Output = Result<UnsafeIntrusiveEntityRef<Self>, crate::Report>>
        + 'a
    where
        T: ?Sized + Builder + 'a;
    fn builder<'b, B>(builder: &'b mut B, span: SourceSpan) -> Self::Builder<'b, B>
    where
        B: ?Sized + Builder + 'b;
}

pub trait Op: AsAny + OpVerifier {
    /// The name of this operation's opcode
    ///
    /// The opcode must be distinct from all other opcodes in the same dialect
    fn name(&self) -> OperationName;
    fn as_operation(&self) -> &Operation;
    fn as_operation_mut(&mut self) -> &mut Operation;

    fn set_span(&mut self, span: SourceSpan) {
        self.as_operation_mut().set_span(span);
    }
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
        self.as_operation().regions()
    }
    fn regions_mut(&mut self) -> &mut RegionList {
        self.as_operation_mut().regions_mut()
    }
    fn region(&self, index: usize) -> EntityRef<'_, Region> {
        self.as_operation().region(index)
    }
    fn region_mut(&mut self, index: usize) -> EntityMut<'_, Region> {
        self.as_operation_mut().region_mut(index)
    }
    fn has_successors(&self) -> bool {
        self.as_operation().has_successors()
    }
    fn num_successors(&self) -> usize {
        self.as_operation().num_successors()
    }
    fn has_operands(&self) -> bool {
        self.as_operation().has_operands()
    }
    fn num_operands(&self) -> usize {
        self.as_operation().num_operands()
    }
    fn operands(&self) -> &OpOperandStorage {
        self.as_operation().operands()
    }
    fn operands_mut(&mut self) -> &mut OpOperandStorage {
        self.as_operation_mut().operands_mut()
    }
    fn results(&self) -> &OpResultStorage {
        self.as_operation().results()
    }
    fn results_mut(&mut self) -> &mut OpResultStorage {
        self.as_operation_mut().results_mut()
    }
    fn successors(&self) -> &OpSuccessorStorage {
        self.as_operation().successors()
    }
    fn successors_mut(&mut self) -> &mut OpSuccessorStorage {
        self.as_operation_mut().successors_mut()
    }
}

impl Spanned for dyn Op {
    fn span(&self) -> SourceSpan {
        self.as_operation().span
    }
}

pub trait OpExt {
    /// Return the value associated with attribute `name` for this function
    fn get_attribute(&self, name: impl Into<interner::Symbol>) -> Option<&dyn AttributeValue>;

    /// Return true if this function has an attributed named `name`
    fn has_attribute(&self, name: impl Into<interner::Symbol>) -> bool;

    /// Set the attribute `name` with `value` for this function.
    fn set_attribute(
        &mut self,
        name: impl Into<interner::Symbol>,
        value: Option<impl AttributeValue>,
    );

    /// Remove any attribute with the given name from this function
    fn remove_attribute(&mut self, name: impl Into<interner::Symbol>);

    /// Returns a handle to the nearest containing [Operation] of type `T` for this operation, if it
    /// is attached to one
    fn nearest_parent_op<T: Op>(&self) -> Option<UnsafeIntrusiveEntityRef<T>>;
}

impl<T: ?Sized + Op> OpExt for T {
    #[inline]
    fn get_attribute(&self, name: impl Into<interner::Symbol>) -> Option<&dyn AttributeValue> {
        self.as_operation().get_attribute(name)
    }

    #[inline]
    fn has_attribute(&self, name: impl Into<interner::Symbol>) -> bool {
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
    fn remove_attribute(&mut self, name: impl Into<interner::Symbol>) {
        self.as_operation_mut().remove_attribute(name);
    }

    #[inline]
    fn nearest_parent_op<U: Op>(&self) -> Option<UnsafeIntrusiveEntityRef<U>> {
        self.as_operation().nearest_parent_op()
    }
}
