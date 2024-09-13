use core::{
    any::{Any, TypeId},
    mem,
    ptr::{NonNull, Pointee},
};

use cranelift_entity::{packed_option::ReservedValue, EntityRef};
use downcast_rs::{impl_downcast, Downcast};
use intrusive_collections::{
    container_of, intrusive_adapter,
    linked_list::{LinkOps, LinkedListOps},
    LinkedListLink, UnsafeRef,
};
use smallvec::SmallVec;

use super::*;

pub type OpList = EntityList<Operation>;
pub type OpCursor<'a> = EntityCursor<'a, Operation>;
pub type OpCursorMut<'a> = EntityCursorMut<'a, Operation>;

/// An [OpSuccessor] is a BlockOperand + OpOperands for that block, attached to an Operation
struct OpSuccessor {
    block: TrackedEntityHandle<BlockOperand>,
    args: SmallVec<[TrackedEntityHandle<OpOperand>; 1]>,
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
    op: Operation,
    _marker: core::marker::PhantomData<T>,
}
impl<T: Op> OperationBuilder<T> {
    pub fn new(context: &'a Context) -> Self {
        let op = Operation::uninit::<T>();
        let handle = context.alloc_uninit_tracked(op);
        Self {
            context,
            op,
            _marker: core::marker::PhantomData,
        }
    }

    pub fn build(self) -> TrackedEntityHandle<T> {
        todo!()
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
    pub block: Option<EntityHandle<Block>>,
    /// The set of operands for this operation
    ///
    /// NOTE: If the op supports immediate operands, the storage for the immediates is handled
    /// by the op, rather than here. Additionally, the semantics of the immediate operands are
    /// determined by the op, e.g. whether the immediate operands are always applied first, or
    /// what they are used for.
    pub operands: SmallVec<[TrackedEntityHandle<OpOperand>; 1]>,
    /// The set of values produced by this operation.
    pub results: SmallVec<[Value; 1]>,
    /// If this operation represents control flow, this field stores the set of successors,
    /// and successor operands.
    pub successors: SmallVec<[OpSuccessor; 1]>,
    /// The set of regions belonging to this operation, if any
    pub regions: RegionList,
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
    fn uninit<T: Op>() -> Self {
        use crate::traits::MultiTraitVtable;

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
        Trait: ?Sized + Pointee<Metadata = *const ()> + 'static,
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
}

/// Attributes
impl Operation {
    /// Return the value associated with attribute `name` for this function
    pub fn get_attribute<Q>(&self, name: &Q) -> Option<&AttributeValue>
    where
        Symbol: std::borrow::Borrow<Q>,
        Q: Ord + ?Sized,
    {
        self.attrs.get(name)
    }

    /// Return true if this function has an attributed named `name`
    pub fn has_attribute<Q>(&self, name: &Q) -> bool
    where
        Symbol: std::borrow::Borrow<Q>,
        Q: Ord + ?Sized,
    {
        self.attrs.has(name)
    }

    /// Set the attribute `name` with `value` for this function.
    pub fn set_attribute(&mut self, name: impl Into<Symbol>, value: impl Into<AttributeValue>) {
        self.attrs.insert(name, value);
    }

    /// Remove any attribute with the given name from this function
    pub fn remove_attribute<Q>(&mut self, name: &Q)
    where
        Symbol: std::borrow::Borrow<Q>,
        Q: Ord + ?Sized,
    {
        self.attrs.remove(name);
    }
}

/// Navigation
impl Operation {
    pub fn prev(&self) -> Option<OpId> {
        unsafe {
            let current = core::ptr::NonNull::new_unchecked(&self.link);
            LinkOps.prev(current).map(Self::link_to_key)
        }
    }

    pub fn next(&self) -> Option<OpId> {
        unsafe {
            let current = core::ptr::NonNull::new_unchecked(&self.link);
            LinkOps.next(current).map(Self::link_to_key)
        }
    }

    #[inline]
    unsafe fn link_to_key(link: NonNull<LinkedListLink>) -> OpId {
        let link = link.as_ref();
        let operation = container_of!(link, Operation, link);
        let key_offset = mem::offset_of!(Operation, key);
        let prev_key = operation.byte_add(key_offset as isize) as *const OpId;
        *prev_key
    }
}

/// Operands
impl Operation {
    pub fn replaces_uses_of_with(&mut self, from: Value, to: Value) {
        if from == to {
            return;
        }

        for operand in self.operands.iter_mut() {
            if operand == &from {
                *operand = to;
            }
        }
    }
}

pub trait Op: Downcast {
    type Id: Copy + PartialEq + Eq + PartialOrd + Ord;

    fn id(&self) -> Self::Id;
    fn name(&self) -> &'static str;
    fn parent(&self) -> Option<OpId> {
        let parent = self.as_operation().parent;
        if parent.is_reserved_value() {
            None
        } else {
            Some(parent)
        }
    }
    fn prev(&self) -> Option<OpId> {
        self.as_operation().prev()
    }
    fn next(&self) -> Option<OpId> {
        self.as_operation().next()
    }
    fn parent_block(&self) -> Option<Block> {
        let block = self.as_operation().block;
        if block.is_reserved_value() {
            None
        } else {
            Some(block)
        }
    }
    fn regions(&self) -> &[RegionId] {
        self.as_operation().regions.as_slice()
    }
    fn operands(&self) -> &ValueList {
        &self.as_operation().operands
    }
    fn results(&self) -> &ValueList {
        &self.as_operation().results
    }
    fn successors(&self) -> &[Successor] {
        self.as_operation().successors.as_slice()
    }
    fn as_operation(&self) -> &Operation;
    fn as_operation_mut(&mut self) -> &mut Operation;
}

impl_downcast!(Op assoc Id where Id: Copy + PartialEq + Eq + PartialOrd + Ord);

impl miden_assembly::Spanned for dyn Op {
    fn span(&self) -> SourceSpan {
        self.as_operation().span
    }
}

pub trait OpExt {
    /// Return the value associated with attribute `name` for this function
    fn get_attribute<Q>(&self, name: &Q) -> Option<&AttributeValue>
    where
        Symbol: std::borrow::Borrow<Q>,
        Q: Ord + ?Sized;

    /// Return true if this function has an attributed named `name`
    fn has_attribute<Q>(&self, name: &Q) -> bool
    where
        Symbol: std::borrow::Borrow<Q>,
        Q: Ord + ?Sized;

    /// Set the attribute `name` with `value` for this function.
    fn set_attribute(&mut self, name: impl Into<Symbol>, value: impl Into<AttributeValue>);

    /// Remove any attribute with the given name from this function
    fn remove_attribute<Q>(&mut self, name: &Q)
    where
        Symbol: std::borrow::Borrow<Q>,
        Q: Ord + ?Sized;
}

impl<T: Op> OpExt for T {
    /// Return the value associated with attribute `name` for this function
    #[inline]
    fn get_attribute<Q>(&self, name: &Q) -> Option<&AttributeValue>
    where
        Symbol: std::borrow::Borrow<Q>,
        Q: Ord + ?Sized,
    {
        self.as_operation().get_attribute(name)
    }

    /// Return true if this function has an attributed named `name`
    #[inline]
    fn has_attribute<Q>(&self, name: &Q) -> bool
    where
        Symbol: std::borrow::Borrow<Q>,
        Q: Ord + ?Sized,
    {
        self.as_operation().has_attribute(name)
    }

    /// Set the attribute `name` with `value` for this function.
    #[inline]
    fn set_attribute(&mut self, name: impl Into<Symbol>, value: impl Into<AttributeValue>) {
        self.as_operation_mut().insert(name, value);
    }

    /// Remove any attribute with the given name from this function
    #[inline]
    fn remove_attribute<Q>(&mut self, name: &Q)
    where
        Symbol: std::borrow::Borrow<Q>,
        Q: Ord + ?Sized,
    {
        self.as_operation_mut().remove(name);
    }
}
