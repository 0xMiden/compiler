use core::{
    marker::Unsize,
    ptr::{DynMetadata, Pointee},
};

use super::{Operation, OperationRef};
use crate::{
    verifier, AttributeValue, Context, Op, OpOperandImpl, OpSuccessor, Region, Report, Type,
    UnsafeIntrusiveEntityRef, ValueRef,
};

// TODO: We need a safe way to construct arbitrary Ops imperatively:
//
// * Allocate an uninit instance of T
// * Initialize the Operartion field of T with the empty Operation data
// * Use the primary builder methods to mutate Operation fields
// * Use generated methods on Op-specific builders to mutate Op fields
// * At the end, convert uninit T to init T, return handle to caller
//
// Problems:
//
// * How do we default-initialize an instance of T for this purpose
// * If we use MaybeUninit, how do we compute field offsets for the Operation field
// * Generated methods can compute offsets, but how do we generate the specialized builders?
pub struct OperationBuilder<'a, T> {
    context: &'a Context,
    op: UnsafeIntrusiveEntityRef<T>,
    _marker: core::marker::PhantomData<T>,
}
impl<'a, T: Op> OperationBuilder<'a, T> {
    pub fn new(context: &'a Context, op: T) -> Self {
        let mut op = context.alloc_tracked(op);

        // SAFETY: Setting the data pointer of the multi-trait vtable must ensure
        // that it points to the concrete type of the allocation, which we can guarantee here,
        // having just allocated it. Until the data pointer is set, casts using the vtable are
        // undefined behavior, so by never allowing the uninitialized vtable to be accessed,
        // we can ensure the multi-trait impl is safe
        unsafe {
            let data_ptr = UnsafeIntrusiveEntityRef::as_ptr(&op);
            let mut op_mut = op.borrow_mut();
            op_mut.as_operation_mut().vtable.set_data_ptr(data_ptr.cast_mut());
        }

        Self {
            context,
            op,
            _marker: core::marker::PhantomData,
        }
    }

    /// Register this op as an implementation of `Trait`.
    ///
    /// This is enforced statically by the type system, as well as dynamically via verification.
    ///
    /// This must be called for any trait that you wish to be able to cast the type-erased
    /// [Operation] to later, or if you wish to get a `dyn Trait` reference from a `dyn Op`
    /// reference.
    ///
    /// If `Trait` has a verifier implementation, it will be automatically applied when calling
    /// [Operation::verify].
    pub fn implement<Trait>(&mut self)
    where
        Trait: ?Sized + Pointee<Metadata = DynMetadata<Trait>> + 'static,
        T: Unsize<Trait> + verifier::Verifier<Trait> + 'static,
    {
        let mut op = self.op.borrow_mut();
        let operation = op.as_operation_mut();
        operation.vtable.register_trait::<T, Trait>();
    }

    /// Set attribute `name` on this op to `value`
    pub fn with_attr<A>(&mut self, name: &'static str, value: A)
    where
        A: AttributeValue,
    {
        let mut op = self.op.borrow_mut();
        op.as_operation_mut().attrs.insert(name, Some(value));
    }

    /// Add a new [Region] to this operation.
    ///
    /// NOTE: You must ensure this is called _after_ [Self::with_operands], and [Self::implements]
    /// if the op implements the [traits::NoRegionArguments] trait. Otherwise, the inserted region
    /// may not be valid for this op.
    pub fn create_region(&mut self) {
        let mut region = Region::default();
        unsafe {
            region.set_owner(Some(self.as_operation_ref()));
        }
        let region = self.context.alloc_tracked(region);
        let mut op = self.op.borrow_mut();
        op.as_operation_mut().regions.push_back(region);
    }

    pub fn with_successor(&mut self, succ: OpSuccessor) {
        todo!()
    }

    pub fn with_successors<I, S>(&mut self, succs: I)
    where
        S: Into<OpSuccessor>,
        I: IntoIterator<Item = S>,
    {
        todo!()
    }

    /// Set the operands given to this op
    pub fn with_operands<I>(&mut self, operands: I)
    where
        I: IntoIterator<Item = ValueRef>,
    {
        // TODO: Verify the safety of this conversion
        let owner = self.as_operation_ref();
        let mut op = self.op.borrow_mut();
        let operands = operands.into_iter().enumerate().map(|(index, value)| {
            self.context
                .alloc_tracked(OpOperandImpl::new(value, owner.clone(), index as u8))
        });
        let op_mut = op.as_operation_mut();
        op_mut.operands.clear();
        op_mut.operands.extend(operands);
    }

    pub fn with_operands_in_group<I>(&mut self, group: usize, operands: I)
    where
        I: IntoIterator<Item = ValueRef>,
    {
        let owner = self.as_operation_ref();
        let mut op = self.op.borrow_mut();
        let operands = operands.into_iter().enumerate().map(|(index, value)| {
            self.context
                .alloc_tracked(OpOperandImpl::new(value, owner.clone(), index as u8))
        });
        let op_operands = op.operands_mut();
        op_operands.push_operands_to_group(group, operands);
    }

    /// Allocate `n` results for this op, of unknown type, to be filled in later
    pub fn with_results(&mut self, n: usize) {
        let owner = self.as_operation_ref();
        let mut op = self.op.borrow_mut();
        let results =
            (0..n).map(|idx| self.context.make_result(Type::Unknown, owner.clone(), idx as u8));
        let op_mut = op.as_operation_mut();
        op_mut.results.clear();
        op_mut.results.extend(results);
    }

    /// Consume this builder, verify the op, and return a handle to it, or an error if validation
    /// failed.
    pub fn build(self) -> Result<UnsafeIntrusiveEntityRef<T>, Report> {
        {
            let op = self.op.borrow();
            op.as_operation().verify(self.context)?;
        }
        Ok(self.op)
    }

    #[inline]
    fn as_operation_ref(&self) -> OperationRef {
        let op = self.op.borrow();
        unsafe {
            let ptr = op.as_operation() as *const Operation;
            OperationRef::from_raw(ptr)
        }
    }
}
