mod builder;
mod name;

use core::{
    fmt,
    ptr::{DynMetadata, Pointee},
};

use smallvec::SmallVec;

pub use self::{builder::OperationBuilder, name::OperationName};
use super::*;

pub type OperationRef = UnsafeIntrusiveEntityRef<Operation>;
pub type OpList = EntityList<Operation>;
pub type OpCursor<'a> = EntityCursor<'a, Operation>;
pub type OpCursorMut<'a> = EntityCursorMut<'a, Operation>;

/// The [Operation] struct provides the common foundation for all [Op] implementations.
///
/// It provides:
///
/// * Support for casting between the concrete operation type `T`, `dyn Op`, the underlying
///   `Operation`, and any of the operation traits that the op implements. Not only can the casts
///   be performed, but an [Operation] can be queried to see if it implements a specific trait at
///   runtime to conditionally perform some behavior. This makes working with operations in the IR
///   very flexible and allows for adding or modifying operations without needing to change most of
///   the compiler, which predominately works on operation traits rather than concrete ops.
/// * Storage for all IR entities attached to an operation, e.g. operands, results, nested regions,
///   attributes, etc.
/// * Navigation of the IR graph; navigate up to the containing block/region/op, down to nested
///   regions/blocks/ops, or next/previous sibling operations in the same block. Additionally, you
///   can navigate directly to the definitions of operands used, to users of results produced, and
///   to successor blocks.
/// * Many utility functions related to working with operations, many of which are also accessible
///   via the [Op] trait, so that working with an [Op] or an [Operation] are largely
///   indistinguishable.
///
/// All [Op] implementations can be cast to the underlying [Operation], but most of the
/// fucntionality is re-exported via default implementations of methods on the [Op] trait. The main
/// benefit is avoiding any potential overhead of casting when going through the trait, rather than
/// calling the underlying [Operation] method directly.
///
/// # Safety
///
/// [Operation] is implemented as part of a larger structure that relies on assumptions which depend
/// on IR entities being allocated via [Context], i.e. the arena. Those allocations produce an
/// [UnsafeIntrusiveEntityRef] or [UnsafeEntityRef], which allocate the pointee type inside a struct
/// that provides metadata about the pointee that can be accessed without aliasing the pointee
/// itself - in particular, links for intrusive collections. This is important, because while these
/// pointer types are a bit like raw pointers in that they lack any lifetime information, and are
/// thus unsafe to dereference in general, they _do_ ensure that the pointee can be safely reified
/// as a reference without violating Rust's borrow checking rules, i.e. they are dynamically borrow-
/// checked.
///
/// The reason why we are able to generally treat these "unsafe" references as safe, is because we
/// require that all IR entities be allocated via [Context]. This makes it essential to keep the
/// context around in order to work with the IR, and effectively guarantees that no [RawEntityRef]
/// will be dereferenced after the context is dropped. This is not a guarantee provided by the
/// compiler however, but one that is imposed in practice, as attempting to work with the IR in
/// any capacity without a [Context] is almost impossible. We must ensure however, that we work
/// within this set of rules to uphold the safety guarantees.
///
/// This "fragility" is a tradeoff - we get the performance characteristics of an arena-allocated
/// IR, with the flexibility and power of using pointers rather than indexes as handles, while also
/// maintaining the safety guarantees of Rust's borrowing system. The downside is that we can't just
/// allocate IR entities wherever we want and use them the same way.
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
    pub operands: OpOperandStorage,
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
            .field_with("name", |f| write!(f, "{}", &self.name()))
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

/// Construction
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

/// Metadata
impl Operation {
    pub fn name(&self) -> OperationName {
        AsRef::<dyn Op>::as_ref(self).name()
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
    #[inline(always)]
    pub fn as_operation_ref(&self) -> OperationRef {
        // SAFETY: This is safe under the assumption that we always allocate Operations using the
        // arena, i.e. it is a child of a RawEntityMetadata structure.
        unsafe { OperationRef::from_raw(self) }
    }

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
        interner::Symbol: core::borrow::Borrow<Q>,
        Q: Ord + ?Sized,
    {
        self.attrs.get_any(name)
    }

    /// Return the value associated with attribute `name` for this function
    pub fn get_attribute_mut<Q>(&mut self, name: &Q) -> Option<&mut dyn AttributeValue>
    where
        interner::Symbol: core::borrow::Borrow<Q>,
        Q: Ord + ?Sized,
    {
        self.attrs.get_any_mut(name)
    }

    /// Return the value associated with attribute `name` for this function, as its concrete type
    /// `T`, _if_ the attribute by that name, is of that type.
    pub fn get_typed_attribute<T, Q>(&self, name: &Q) -> Option<&T>
    where
        T: AttributeValue,
        interner::Symbol: core::borrow::Borrow<Q>,
        Q: Ord + ?Sized,
    {
        self.attrs.get(name)
    }

    /// Return the value associated with attribute `name` for this function, as its concrete type
    /// `T`, _if_ the attribute by that name, is of that type.
    pub fn get_typed_attribute_mut<T, Q>(&mut self, name: &Q) -> Option<&mut T>
    where
        T: AttributeValue,
        interner::Symbol: core::borrow::Borrow<Q>,
        Q: Ord + ?Sized,
    {
        self.attrs.get_mut(name)
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

    pub fn region(&self, index: usize) -> EntityRef<'_, Region> {
        let mut cursor = self.regions.front();
        let mut count = 0;
        while !cursor.is_null() {
            if index == count {
                return cursor.into_borrow().unwrap();
            }
            cursor.move_next();
            count += 1;
        }
        panic!("invalid region index {index}: out of bounds");
    }

    pub fn region_mut(&mut self, index: usize) -> EntityMut<'_, Region> {
        let mut cursor = self.regions.front_mut();
        let mut count = 0;
        while !cursor.is_null() {
            if index == count {
                return cursor.into_borrow_mut().unwrap();
            }
            cursor.move_next();
            count += 1;
        }
        panic!("invalid region index {index}: out of bounds");
    }
}

/// Successors
impl Operation {
    #[inline]
    pub fn has_successors(&self) -> bool {
        !self.successors.is_empty()
    }

    #[inline]
    pub fn num_successors(&self) -> usize {
        self.successors.len()
    }

    #[inline(always)]
    pub fn successors(&self) -> &[OpSuccessor] {
        &self.successors
    }

    #[inline(always)]
    pub fn successors_mut(&mut self) -> &mut [OpSuccessor] {
        &mut self.successors
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
    pub fn operands(&self) -> &OpOperandStorage {
        &self.operands
    }

    #[inline]
    pub fn operands_mut(&mut self) -> &mut OpOperandStorage {
        &mut self.operands
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

/// Results
impl Operation {
    #[inline]
    pub fn has_results(&self) -> bool {
        !self.results.is_empty()
    }

    #[inline]
    pub fn num_results(&self) -> usize {
        self.results.len()
    }

    #[inline]
    pub fn results(&self) -> &[OpResultRef] {
        self.results.as_slice()
    }

    #[inline]
    pub fn results_mut(&mut self) -> &mut [OpResultRef] {
        self.results.as_mut_slice()
    }
}
