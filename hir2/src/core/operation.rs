use core::{
    fmt,
    marker::Unsize,
    ptr::{DynMetadata, Pointee},
};

use smallvec::SmallVec;

use super::*;

pub type OperationRef = UnsafeIntrusiveEntityRef<Operation>;
pub type OpList = EntityList<Operation>;
pub type OpCursor<'a> = EntityCursor<'a, Operation>;
pub type OpCursorMut<'a> = EntityCursorMut<'a, Operation>;

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct OperationName {
    pub dialect: DialectName,
    pub name: interner::Symbol,
}
impl OperationName {
    pub fn new<S>(dialect: DialectName, name: S) -> Self
    where
        S: Into<interner::Symbol>,
    {
        Self {
            dialect,
            name: name.into(),
        }
    }
}
impl fmt::Debug for OperationName {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}
impl fmt::Display for OperationName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}", &self.dialect, &self.name)
    }
}

/// An [OpSuccessor] is a BlockOperand + OpOperands for that block, attached to an Operation
pub struct OpSuccessor {
    pub block: BlockOperandRef,
    pub args: SmallVec<[OpOperand; 1]>,
}
impl fmt::Debug for OpSuccessor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OpSuccessor")
            .field("block", &self.block.borrow().block_id())
            .field("args", &self.args)
            .finish()
    }
}

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
        op.as_operation_mut().attrs.insert(interner::Symbol::intern(name), Some(value));
    }

    /// Set the operands given to this op
    pub fn with_operands<I>(&mut self, operands: I)
    where
        I: IntoIterator<Item = ValueRef>,
    {
        let mut op = self.op.borrow_mut();
        // TODO: Verify the safety of this conversion
        let owner = unsafe {
            let ptr = op.as_operation() as *const Operation;
            UnsafeIntrusiveEntityRef::from_raw(ptr)
        };
        let operands = operands.into_iter().enumerate().map(|(index, value)| {
            self.context
                .alloc_tracked(value::OpOperandImpl::new(value, owner.clone(), index as u8))
        });
        let op_mut = op.as_operation_mut();
        op_mut.operands.clear();
        op_mut.operands.extend(operands);
    }

    /// Allocate `n` results for this op, of unknown type, to be filled in later
    pub fn with_results(&mut self, n: usize) {
        let mut op = self.op.borrow_mut();
        let owner = unsafe {
            let ptr = op.as_operation() as *const Operation;
            UnsafeIntrusiveEntityRef::from_raw(ptr)
        };
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
}

#[derive(Spanned)]
pub struct Operation {
    /// In order to support upcasting from [Operation] to its concrete [Op] type, as well as
    /// casting to any of the operation traits it implements, we need our own vtable that lets
    /// us track the individual vtables for each type and trait we need to cast to for this
    /// instance.
    pub(crate) vtable: traits::MultiTraitVtable,
    #[span]
    pub span: SourceSpan,
    /// Attributes that apply to this operation
    pub attrs: AttributeSet,
    /// The containing block of this operation
    ///
    /// Is set to `None` if this operation is detached
    pub block: Option<BlockRef>,
    /// The set of operands for this operation
    ///
    /// NOTE: If the op supports immediate operands, the storage for the immediates is handled
    /// by the op, rather than here. Additionally, the semantics of the immediate operands are
    /// determined by the op, e.g. whether the immediate operands are always applied first, or
    /// what they are used for.
    pub operands: SmallVec<[OpOperand; 1]>,
    /// The set of values produced by this operation.
    pub results: SmallVec<[OpResultRef; 1]>,
    /// If this operation represents control flow, this field stores the set of successors,
    /// and successor operands.
    pub successors: SmallVec<[OpSuccessor; 1]>,
    /// The set of regions belonging to this operation, if any
    pub regions: RegionList,
}
impl fmt::Debug for Operation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Operation")
            .field("attrs", &self.attrs)
            .field("block", &self.block.as_ref().map(|b| b.borrow().id()))
            .field("operands", &self.operands)
            .field("results", &self.results)
            .field("successors", &self.successors)
            .finish_non_exhaustive()
    }
}
impl AsRef<dyn Op> for Operation {
    fn as_ref(&self) -> &dyn Op {
        self.vtable.downcast_trait().unwrap()
    }
}
impl AsMut<dyn Op> for Operation {
    fn as_mut(&mut self) -> &mut dyn Op {
        self.vtable.downcast_trait_mut().unwrap()
    }
}
impl Operation {
    pub fn uninit<T: Op>() -> Self {
        use super::traits::MultiTraitVtable;

        let mut vtable = MultiTraitVtable::new::<T>();
        vtable.register_trait::<T, dyn Op>();

        Self {
            vtable,
            span: Default::default(),
            attrs: Default::default(),
            block: Default::default(),
            operands: Default::default(),
            results: Default::default(),
            successors: Default::default(),
            regions: Default::default(),
        }
    }
}

/// Verification
impl Operation {
    pub fn verify(&self, context: &Context) -> Result<(), Report> {
        let dyn_op: &dyn Op = self.as_ref();
        dyn_op.verify(context)
    }
}

/// Traits/Casts
impl Operation {
    /// Returns true if the concrete type of this operation is `T`
    #[inline]
    pub fn is<T: Op>(&self) -> bool {
        self.vtable.is::<T>()
    }

    /// Returns true if this operation implements `Trait`
    #[inline]
    pub fn implements<Trait>(&self) -> bool
    where
        Trait: ?Sized + Pointee<Metadata = DynMetadata<Trait>> + 'static,
    {
        self.vtable.implements::<Trait>()
    }

    /// Attempt to downcast to the concrete [Op] type of this operation
    pub fn downcast_ref<T: Op>(&self) -> Option<&T> {
        self.vtable.downcast_ref::<T>()
    }

    /// Attempt to downcast to the concrete [Op] type of this operation
    pub fn downcast_mut<T: Op>(&mut self) -> Option<&mut T> {
        self.vtable.downcast_mut::<T>()
    }

    /// Attempt to cast this operation reference to an implementation of `Trait`
    pub fn as_trait<Trait>(&self) -> Option<&Trait>
    where
        Trait: ?Sized + Pointee<Metadata = DynMetadata<Trait>> + 'static,
    {
        self.vtable.downcast_trait()
    }

    /// Attempt to cast this operation reference to an implementation of `Trait`
    pub fn as_trait_mut<Trait>(&mut self) -> Option<&mut Trait>
    where
        Trait: ?Sized + Pointee<Metadata = DynMetadata<Trait>> + 'static,
    {
        self.vtable.downcast_trait_mut()
    }
}

/// Attributes
impl Operation {
    /// Return the value associated with attribute `name` for this function
    pub fn get_attribute<Q>(&self, name: &Q) -> Option<&dyn AttributeValue>
    where
        interner::Symbol: std::borrow::Borrow<Q>,
        Q: Ord + ?Sized,
    {
        self.attrs.get_any(name)
    }

    /// Return true if this function has an attributed named `name`
    pub fn has_attribute<Q>(&self, name: &Q) -> bool
    where
        interner::Symbol: std::borrow::Borrow<Q>,
        Q: Ord + ?Sized,
    {
        self.attrs.has(name)
    }

    /// Set the attribute `name` with `value` for this function.
    pub fn set_attribute(
        &mut self,
        name: impl Into<interner::Symbol>,
        value: Option<impl AttributeValue>,
    ) {
        self.attrs.insert(name, value);
    }

    /// Remove any attribute with the given name from this function
    pub fn remove_attribute<Q>(&mut self, name: &Q)
    where
        interner::Symbol: std::borrow::Borrow<Q>,
        Q: Ord + ?Sized,
    {
        self.attrs.remove(name);
    }
}

/// Navigation
impl Operation {
    /// Returns a handle to the containing [Block] of this operation, if it is attached to one
    pub fn parent(&self) -> Option<BlockRef> {
        self.block.clone()
    }

    /// Returns a handle to the containing [Region] of this operation, if it is attached to one
    pub fn parent_region(&self) -> Option<RegionRef> {
        self.block.as_ref().and_then(|block| block.borrow().parent())
    }

    /// Returns a handle to the nearest containing [Operation] of this operation, if it is attached
    /// to one
    pub fn parent_op(&self) -> Option<OperationRef> {
        self.block.as_ref().and_then(|block| block.borrow().parent_op())
    }

    /// Returns a handle to the nearest containing [Operation] of type `T` for this operation, if it
    /// is attached to one
    pub fn nearest_parent_op<T: Op>(&self) -> Option<UnsafeIntrusiveEntityRef<T>> {
        let mut parent = self.parent_op();
        while let Some(op) = parent.take() {
            let entity_ref = op.borrow();
            parent = entity_ref.parent_op();
            if let Some(t_ref) = entity_ref.downcast_ref::<T>() {
                return Some(unsafe { UnsafeIntrusiveEntityRef::from_raw(t_ref) });
            }
        }
        None
    }
}

/// Regions
impl Operation {
    #[inline]
    pub fn has_regions(&self) -> bool {
        !self.regions.is_empty()
    }

    #[inline]
    pub fn num_regions(&self) -> usize {
        self.regions.len()
    }

    #[inline(always)]
    pub fn regions(&self) -> &RegionList {
        &self.regions
    }

    #[inline(always)]
    pub fn regions_mut(&mut self) -> &mut RegionList {
        &mut self.regions
    }
}

/// Operands
impl Operation {
    #[inline]
    pub fn has_operands(&self) -> bool {
        !self.operands.is_empty()
    }

    #[inline]
    pub fn num_operands(&self) -> usize {
        self.operands.len()
    }

    #[inline]
    pub fn operands(&self) -> &[OpOperand] {
        self.operands.as_slice()
    }

    pub fn replaces_uses_of_with(&mut self, mut from: ValueRef, mut to: ValueRef) {
        if ValueRef::ptr_eq(&from, &to) {
            return;
        }

        let from_id = from.borrow().id();
        if from_id == to.borrow().id() {
            return;
        }

        for mut operand in self.operands.iter().cloned() {
            if operand.borrow().value.borrow().id() == from_id {
                debug_assert!(operand.is_linked());
                // Remove the operand from `from`
                {
                    let mut from_mut = from.borrow_mut();
                    let from_uses = from_mut.uses_mut();
                    let mut cursor = unsafe { from_uses.cursor_mut_from_ptr(operand.clone()) };
                    cursor.remove();
                }
                // Add the operand to `to`
                operand.borrow_mut().value = to.clone();
                to.borrow_mut().insert_use(operand);
            }
        }
    }
}
