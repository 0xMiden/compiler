mod builder;
mod name;

use alloc::rc::Rc;
use core::{
    fmt,
    ptr::{DynMetadata, NonNull, Pointee},
    sync::atomic::AtomicU32,
};

use smallvec::SmallVec;

pub use self::{builder::OperationBuilder, name::OperationName};
use super::*;
use crate::{AttributeSet, AttributeValue};

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
    /// The [Context] in which this [Operation] was allocated.
    context: NonNull<Context>,
    /// The dialect and opcode name for this operation, as well as trait implementation metadata
    name: OperationName,
    /// The offset of the field containing this struct inside the concrete [Op] it represents.
    ///
    /// This is required in order to be able to perform casts from [Operation]. An [Operation]
    /// cannot be constructed without providing it to the `uninit` function, and callers of that
    /// function are required to ensure that it is correct.
    offset: usize,
    /// The order of this operation in its containing block
    ///
    /// This is atomic to ensure that even if a mutable reference to this operation is held, loads
    /// of this field cannot be elided, as the value can still be mutated at any time. In practice,
    /// the only time this is ever written, is when all operations in a block have their orders
    /// recomputed, or when a single operation is updating its own order.
    order: AtomicU32,
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
    pub results: OpResultStorage,
    /// If this operation represents control flow, this field stores the set of successors,
    /// and successor operands.
    pub successors: OpSuccessorStorage,
    /// The set of regions belonging to this operation, if any
    pub regions: RegionList,
}
impl fmt::Debug for Operation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Operation")
            .field_with("name", |f| write!(f, "{}", &self.name()))
            .field("offset", &self.offset)
            .field("order", &self.order)
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
        self.name.upcast(self.container()).unwrap()
    }
}

impl AsMut<dyn Op> for Operation {
    fn as_mut(&mut self) -> &mut dyn Op {
        self.name.upcast_mut(self.container().cast_mut()).unwrap()
    }
}

impl Entity for Operation {}
impl EntityWithParent for Operation {
    type Parent = Block;

    fn on_inserted_into_parent(
        mut this: UnsafeIntrusiveEntityRef<Self>,
        parent: UnsafeIntrusiveEntityRef<Self::Parent>,
    ) {
        let mut op = this.borrow_mut();
        op.block = Some(parent);
        op.order.store(Self::INVALID_ORDER, std::sync::atomic::Ordering::Release);
    }

    fn on_removed_from_parent(
        mut this: UnsafeIntrusiveEntityRef<Self>,
        _parent: UnsafeIntrusiveEntityRef<Self::Parent>,
    ) {
        this.borrow_mut().block = None;
    }

    fn on_transfered_to_new_parent(
        from: UnsafeIntrusiveEntityRef<Self::Parent>,
        mut to: UnsafeIntrusiveEntityRef<Self::Parent>,
        transferred: impl IntoIterator<Item = UnsafeIntrusiveEntityRef<Self>>,
    ) {
        // Invalidate the ordering of the new parent block
        to.borrow_mut().invalidate_op_order();

        // If we are transferring operations within the same block, the block pointer doesn't
        // need to be updated
        if BlockRef::ptr_eq(&from, &to) {
            return;
        }

        for mut transferred_op in transferred {
            transferred_op.borrow_mut().block = Some(to.clone());
        }
    }
}

/// Construction
impl Operation {
    #[doc(hidden)]
    pub unsafe fn uninit<T: Op>(context: Rc<Context>, name: OperationName, offset: usize) -> Self {
        assert!(name.is::<T>());

        Self {
            context: unsafe { NonNull::new_unchecked(Rc::as_ptr(&context).cast_mut()) },
            name,
            offset,
            order: AtomicU32::new(0),
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
    /// Get the name of this operation
    ///
    /// An operation name consists of both its dialect, and its opcode.
    pub fn name(&self) -> OperationName {
        self.name.clone()
    }

    /// Get the dialect associated with this operation
    pub fn dialect(&self) -> Rc<dyn Dialect> {
        self.context().get_registered_dialect(self.name.dialect())
    }

    /// Set the source location associated with this operation
    #[inline]
    pub fn set_span(&mut self, span: SourceSpan) {
        self.span = span;
    }

    /// Get a borrowed reference to the owning [Context] of this operation
    #[inline(always)]
    pub fn context(&self) -> &Context {
        // SAFETY: This is safe so long as this operation is allocated in a Context, since the
        // Context by definition outlives the allocation.
        unsafe { self.context.as_ref() }
    }

    /// Get a owned reference to the owning [Context] of this operation
    pub fn context_rc(&self) -> Rc<Context> {
        // SAFETY: This is safe so long as this operation is allocated in a Context, since the
        // Context by definition outlives the allocation.
        //
        // Additionally, constructing the Rc from a raw pointer is safe here, as the pointer was
        // obtained using `Rc::as_ptr`, so the only requirement to call `Rc::from_raw` is to
        // increment the strong count, as `as_ptr` does not preserve the count for the reference
        // held by this operation. Incrementing the count first is required to manufacture new
        // clones of the `Rc` safely.
        unsafe {
            let ptr = self.context.as_ptr().cast_const();
            Rc::increment_strong_count(ptr);
            Rc::from_raw(ptr)
        }
    }
}

/// Verification
impl Operation {
    /// Run any verifiers for this operation
    pub fn verify(&self, context: &Context) -> Result<(), Report> {
        let dyn_op: &dyn Op = self.as_ref();
        dyn_op.verify(context)
    }

    /// Run any verifiers for this operation, and all of its nested operations, recursively.
    ///
    /// The verification is performed in post-order, so that when the verifier(s) for `self` are
    /// run, it is known that all of its children have successfully verified.
    pub fn recursively_verify(&self, context: &Context) -> Result<(), Report> {
        self.postwalk_interruptible(|op: OperationRef| {
            let op = op.borrow();
            op.verify(context).into()
        })
        .into_result()
    }
}

/// Traits/Casts
impl Operation {
    pub(super) const fn container(&self) -> *const () {
        unsafe {
            let ptr = self as *const Self;
            ptr.byte_sub(self.offset).cast()
        }
    }

    #[inline(always)]
    pub fn as_operation_ref(&self) -> OperationRef {
        // SAFETY: This is safe under the assumption that we always allocate Operations using the
        // arena, i.e. it is a child of a RawEntityMetadata structure.
        unsafe { OperationRef::from_raw(self) }
    }

    /// Returns true if the concrete type of this operation is `T`
    #[inline]
    pub fn is<T: Op>(&self) -> bool {
        self.name.is::<T>()
    }

    /// Returns true if this operation implements `Trait`
    #[inline]
    pub fn implements<Trait>(&self) -> bool
    where
        Trait: ?Sized + Pointee<Metadata = DynMetadata<Trait>> + 'static,
    {
        self.name.implements::<Trait>()
    }

    /// Attempt to downcast to the concrete [Op] type of this operation
    pub fn downcast_ref<T: Op>(&self) -> Option<&T> {
        self.name.downcast_ref::<T>(self.container())
    }

    /// Attempt to downcast to the concrete [Op] type of this operation
    pub fn downcast_mut<T: Op>(&mut self) -> Option<&mut T> {
        self.name.downcast_mut::<T>(self.container().cast_mut())
    }

    /// Attempt to cast this operation reference to an implementation of `Trait`
    pub fn as_trait<Trait>(&self) -> Option<&Trait>
    where
        Trait: ?Sized + Pointee<Metadata = DynMetadata<Trait>> + 'static,
    {
        self.name.upcast(self.container())
    }

    /// Attempt to cast this operation reference to an implementation of `Trait`
    pub fn as_trait_mut<Trait>(&mut self) -> Option<&mut Trait>
    where
        Trait: ?Sized + Pointee<Metadata = DynMetadata<Trait>> + 'static,
    {
        self.name.upcast_mut(self.container().cast_mut())
    }
}

/// Attributes
impl Operation {
    /// Get the underlying attribute set for this operation
    #[inline(always)]
    pub fn attributes(&self) -> &AttributeSet {
        &self.attrs
    }

    /// Get a mutable reference to the underlying attribute set for this operation
    #[inline(always)]
    pub fn attributes_mut(&mut self) -> &mut AttributeSet {
        &mut self.attrs
    }

    /// Return the value associated with attribute `name` for this function
    pub fn get_attribute(&self, name: impl Into<interner::Symbol>) -> Option<&dyn AttributeValue> {
        self.attrs.get_any(name.into())
    }

    /// Return the value associated with attribute `name` for this function
    pub fn get_attribute_mut(
        &mut self,
        name: impl Into<interner::Symbol>,
    ) -> Option<&mut dyn AttributeValue> {
        self.attrs.get_any_mut(name.into())
    }

    /// Return the value associated with attribute `name` for this function, as its concrete type
    /// `T`, _if_ the attribute by that name, is of that type.
    pub fn get_typed_attribute<T>(&self, name: impl Into<interner::Symbol>) -> Option<&T>
    where
        T: AttributeValue,
    {
        self.attrs.get(name.into())
    }

    /// Return the value associated with attribute `name` for this function, as its concrete type
    /// `T`, _if_ the attribute by that name, is of that type.
    pub fn get_typed_attribute_mut<T>(
        &mut self,
        name: impl Into<interner::Symbol>,
    ) -> Option<&mut T>
    where
        T: AttributeValue,
    {
        self.attrs.get_mut(name.into())
    }

    /// Return true if this function has an attributed named `name`
    pub fn has_attribute(&self, name: impl Into<interner::Symbol>) -> bool {
        self.attrs.has(name.into())
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
    pub fn remove_attribute(&mut self, name: impl Into<interner::Symbol>) {
        self.attrs.remove(name.into());
    }
}

/// Symbol Attributes
impl Operation {
    pub fn set_symbol_attribute(
        &mut self,
        name: impl Into<interner::Symbol>,
        symbol: impl AsSymbolRef,
    ) {
        let name = name.into();
        let mut symbol = symbol.as_symbol_ref();

        // Store the underlying attribute value
        let user = self.context().alloc_tracked(SymbolUse {
            owner: self.as_operation_ref(),
            symbol: name,
        });
        if self.has_attribute(name) {
            let attr = self.get_typed_attribute_mut::<SymbolNameAttr>(name).unwrap();
            let symbol = symbol.borrow();
            assert!(
                !attr.user.is_linked(),
                "attempted to replace symbol use without unlinking the previously used symbol \
                 first"
            );
            attr.user = user.clone();
            attr.name = symbol.name();
            attr.path = symbol.components().into_path(true);
        } else {
            let attr = {
                let symbol = symbol.borrow();
                let name = symbol.name();
                let path = symbol.components().into_path(true);
                SymbolNameAttr {
                    name,
                    path,
                    user: user.clone(),
                }
            };
            self.set_attribute(name, Some(attr));
        }

        // Add `self` as a user of `symbol`, unless `self` is `symbol`
        let (data_ptr, _) = SymbolRef::as_ptr(&symbol).to_raw_parts();
        if core::ptr::addr_eq(data_ptr, self.container()) {
            return;
        }

        let mut symbol = symbol.borrow_mut();
        let symbol_uses = symbol.uses_mut();
        symbol_uses.push_back(user);
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
    /// Returns true if this operation has any regions
    #[inline]
    pub fn has_regions(&self) -> bool {
        !self.regions.is_empty()
    }

    /// Returns the number of regions owned by this operation.
    ///
    /// NOTE: This does not include regions of nested operations, just those directly attached
    /// to this operation.
    #[inline]
    pub fn num_regions(&self) -> usize {
        self.regions.len()
    }

    /// Get a reference to the region list for this operation
    #[inline(always)]
    pub fn regions(&self) -> &RegionList {
        &self.regions
    }

    /// Get a mutable reference to the region list for this operation
    #[inline(always)]
    pub fn regions_mut(&mut self) -> &mut RegionList {
        &mut self.regions
    }

    /// Get a reference to a specific region, given its index.
    ///
    /// This function will panic if the index is invalid.
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

    /// Get a mutable reference to a specific region, given its index.
    ///
    /// This function will panic if the index is invalid.
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
    /// Returns true if this operation has any successor blocks
    #[inline]
    pub fn has_successors(&self) -> bool {
        !self.successors.is_empty()
    }

    /// Returns the number of successor blocks this operation may transfer control to
    #[inline]
    pub fn num_successors(&self) -> usize {
        self.successors.len()
    }

    /// Get a reference to the successors of this operation
    #[inline(always)]
    pub fn successors(&self) -> &OpSuccessorStorage {
        &self.successors
    }

    /// Get a mutable reference to the successors of this operation
    #[inline(always)]
    pub fn successors_mut(&mut self) -> &mut OpSuccessorStorage {
        &mut self.successors
    }

    /// Get a reference to the successor group at `index`
    #[inline]
    pub fn successor_group(&self, index: usize) -> OpSuccessorRange<'_> {
        self.successors.group(index)
    }

    /// Get a mutable reference to the successor group at `index`
    #[inline]
    pub fn successor_group_mut(&mut self, index: usize) -> OpSuccessorRangeMut<'_> {
        self.successors.group_mut(index)
    }

    /// Get a reference to the keyed successor group at `index`
    #[inline]
    pub fn keyed_successor_group<T>(&self, index: usize) -> KeyedSuccessorRange<'_, T>
    where
        T: KeyedSuccessor,
    {
        let range = self.successors.group(index);
        KeyedSuccessorRange::new(range, &self.operands)
    }

    /// Get a mutable reference to the keyed successor group at `index`
    #[inline]
    pub fn keyed_successor_group_mut<T>(&mut self, index: usize) -> KeyedSuccessorRangeMut<'_, T>
    where
        T: KeyedSuccessor,
    {
        let range = self.successors.group_mut(index);
        KeyedSuccessorRangeMut::new(range, &mut self.operands)
    }

    /// Get a reference to the successor at `index` in the group at `group_index`
    #[inline]
    pub fn successor_in_group(&self, group_index: usize, index: usize) -> OpSuccessor<'_> {
        let info = &self.successors.group(group_index)[index];
        OpSuccessor {
            dest: info.block.clone(),
            arguments: self.operands.group(info.operand_group as usize),
        }
    }

    /// Get a mutable reference to the successor at `index` in the group at `group_index`
    #[inline]
    pub fn successor_in_group_mut(
        &mut self,
        group_index: usize,
        index: usize,
    ) -> OpSuccessorMut<'_> {
        let info = &self.successors.group(group_index)[index];
        OpSuccessorMut {
            dest: info.block.clone(),
            arguments: self.operands.group_mut(info.operand_group as usize),
        }
    }

    /// Get a reference to the successor at `index`
    #[inline]
    pub fn successor(&self, index: usize) -> OpSuccessor<'_> {
        let info = &self.successors[index];
        OpSuccessor {
            dest: info.block.clone(),
            arguments: self.operands.group(info.operand_group as usize),
        }
    }

    /// Get a mutable reference to the successor at `index`
    #[inline]
    pub fn successor_mut(&mut self, index: usize) -> OpSuccessorMut<'_> {
        let info = self.successors[index].clone();
        OpSuccessorMut {
            dest: info.block,
            arguments: self.operands.group_mut(info.operand_group as usize),
        }
    }

    /// Get an iterator over the successors of this operation
    pub fn successor_iter(&self) -> impl DoubleEndedIterator<Item = OpSuccessor<'_>> + '_ {
        self.successors.iter().map(|info| OpSuccessor {
            dest: info.block.clone(),
            arguments: self.operands.group(info.operand_group as usize),
        })
    }
}

/// Operands
impl Operation {
    /// Returns true if this operation has at least one operand
    #[inline]
    pub fn has_operands(&self) -> bool {
        !self.operands.is_empty()
    }

    /// Returns the number of operands given to this operation
    #[inline]
    pub fn num_operands(&self) -> usize {
        self.operands.len()
    }

    /// Get a reference to the operand storage for this operation
    #[inline]
    pub fn operands(&self) -> &OpOperandStorage {
        &self.operands
    }

    /// Get a mutable reference to the operand storage for this operation
    #[inline]
    pub fn operands_mut(&mut self) -> &mut OpOperandStorage {
        &mut self.operands
    }

    /// Replace the current operands of this operation with the ones provided in `operands`.
    pub fn set_operands(&mut self, operands: impl Iterator<Item = ValueRef>) {
        self.operands.clear();
        let context = self.context_rc();
        let owner = self.as_operation_ref();
        self.operands.extend(
            operands
                .into_iter()
                .enumerate()
                .map(|(index, value)| context.make_operand(value, owner.clone(), index as u8)),
        );
    }

    /// Replace any uses of `from` with `to` within this operation
    pub fn replaces_uses_of_with(&mut self, mut from: ValueRef, mut to: ValueRef) {
        if ValueRef::ptr_eq(&from, &to) {
            return;
        }

        for operand in self.operands.iter_mut() {
            debug_assert!(operand.is_linked());
            if ValueRef::ptr_eq(&from, &operand.borrow().value) {
                // Remove use of `from` by `operand`
                {
                    let mut from_mut = from.borrow_mut();
                    let from_uses = from_mut.uses_mut();
                    let mut cursor = unsafe { from_uses.cursor_mut_from_ptr(operand.clone()) };
                    cursor.remove();
                }
                // Add use of `to` by `operand`
                operand.borrow_mut().value = to.clone();
                to.borrow_mut().insert_use(operand.clone());
            }
        }
    }

    /// Replace all uses of this operation's results with `values`
    ///
    /// The number of results and the number of values in `values` must be exactly the same,
    /// otherwise this function will panic.
    pub fn replace_all_uses_with(&mut self, values: impl ExactSizeIterator<Item = ValueRef>) {
        assert_eq!(self.num_results(), values.len());
        for (result, replacement) in self.results.iter_mut().zip(values) {
            if ValueRef::ptr_eq(&result.clone().upcast(), &replacement) {
                continue;
            }
            result.borrow_mut().replace_all_uses_with(replacement);
        }
    }

    /// Replace uses of this operation's results with `values`, for each use which, when provided
    /// to the given callback, returns true.
    ///
    /// The number of results and the number of values in `values` must be exactly the same,
    /// otherwise this function will panic.
    pub fn replace_uses_with_if<F, V>(&mut self, values: V, should_replace: F)
    where
        V: ExactSizeIterator<Item = ValueRef>,
        F: Fn(&OpOperandImpl) -> bool,
    {
        assert_eq!(self.num_results(), values.len());
        for (result, replacement) in self.results.iter_mut().zip(values) {
            let mut result = result.clone().upcast();
            if ValueRef::ptr_eq(&result, &replacement) {
                continue;
            }
            result.borrow_mut().replace_uses_with_if(replacement, &should_replace);
        }
    }
}

/// Results
impl Operation {
    /// Returns true if this operation produces any results
    #[inline]
    pub fn has_results(&self) -> bool {
        !self.results.is_empty()
    }

    /// Returns the number of results produced by this operation
    #[inline]
    pub fn num_results(&self) -> usize {
        self.results.len()
    }

    /// Get a reference to the result set of this operation
    #[inline]
    pub fn results(&self) -> &OpResultStorage {
        &self.results
    }

    /// Get a mutable reference to the result set of this operation
    #[inline]
    pub fn results_mut(&mut self) -> &mut OpResultStorage {
        &mut self.results
    }

    /// Get a reference to the result at `index` among all results of this operation
    #[inline]
    pub fn get_result(&self, index: usize) -> &OpResultRef {
        &self.results[index]
    }

    /// Returns true if the results of this operation are used
    pub fn is_used(&self) -> bool {
        self.results.iter().any(|result| result.borrow().is_used())
    }

    /// Returns true if the results of this operation have exactly one user
    pub fn has_exactly_one_use(&self) -> bool {
        let mut used_by = None;
        for result in self.results.iter() {
            let result = result.borrow();
            if !result.is_used() {
                continue;
            }

            for used in result.iter_uses() {
                if used_by.as_ref().is_some_and(|user| !OperationRef::eq(user, &used.owner)) {
                    // We found more than one user
                    return false;
                } else if used_by.is_none() {
                    used_by = Some(used.owner.clone());
                }
            }
        }

        // If we reach here, and we have a `used_by` set, we have exactly one user
        used_by.is_some()
    }

    /// Returns true if the results of this operation are used outside of the given block
    pub fn is_used_outside_of_block(&self, block: &BlockRef) -> bool {
        self.results
            .iter()
            .any(|result| result.borrow().is_used_outside_of_block(block))
    }

    /// Returns true if this operation is unused and has no side effects that prevent it being erased
    pub fn is_trivially_dead(&self) -> bool {
        !self.is_used() && self.would_be_trivially_dead()
    }

    /// Returns true if this operation would be dead if unused, and has no side effects that would
    /// prevent erasing it. This is equivalent to checking `is_trivially_dead` if `self` is unused.
    ///
    /// NOTE: Terminators and symbols are never considered to be trivially dead by this function.
    pub fn would_be_trivially_dead(&self) -> bool {
        if self.implements::<dyn crate::traits::Terminator>() || self.implements::<dyn Symbol>() {
            false
        } else {
            self.would_be_trivially_dead_even_if_terminator()
        }
    }

    /// Implementation of `would_be_trivially_dead` that also considers terminator operations as
    /// dead if they have no side effects. This allows for marking region operations as trivially
    /// dead without always being conservative about terminators.
    pub fn would_be_trivially_dead_even_if_terminator(&self) -> bool {
        // The set of operations to consider when checking for side effects
        let mut effecting_ops = SmallVec::<[OperationRef; 1]>::from_iter([self.as_operation_ref()]);
        while let Some(op) = effecting_ops.pop() {
            let op = op.borrow();
            // If the operation has recursive effects, push all of the nested operations on to the
            // stack to consider.
            let has_recursive_effects =
                op.implements::<dyn crate::traits::HasRecursiveMemoryEffects>();
            if has_recursive_effects {
                for region in op.regions() {
                    for block in region.body() {
                        let mut cursor = block.body().front();
                        while let Some(op) = cursor.as_pointer() {
                            effecting_ops.push(op);
                            cursor.move_next();
                        }
                    }
                }
            }

            // If the op has memory effects, try to characterize them to see if the op is trivially
            // dead here.
            if op.implements::<dyn crate::traits::MemoryWrite>()
                || op.implements::<dyn crate::traits::MemoryFree>()
            {
                return false;
            }

            // If there were no effect interfaces, we treat this op as conservatively having effects
            if !op.implements::<dyn crate::traits::MemoryRead>()
                && !op.implements::<dyn crate::traits::MemoryAlloc>()
            {
                return false;
            }
        }

        // If we get here, none of the operations had effects that prevented marking this operation
        // as dead.
        true
    }
}

/// Insertion
impl Operation {
    pub fn insert_at_start(&mut self, mut block: BlockRef) {
        assert!(
            self.block.is_none(),
            "cannot insert operation that is already attached to another block"
        );
        {
            let mut block = block.borrow_mut();
            block.body_mut().push_front(unsafe { OperationRef::from_raw(self) });
        }
        self.block = Some(block);
    }

    pub fn insert_at_end(&mut self, mut block: BlockRef) {
        assert!(
            self.block.is_none(),
            "cannot insert operation that is already attached to another block"
        );
        {
            let mut block = block.borrow_mut();
            block.body_mut().push_back(unsafe { OperationRef::from_raw(self) });
        }
        self.block = Some(block);
    }

    pub fn insert_before(&mut self, before: OperationRef) {
        assert!(
            self.block.is_none(),
            "cannot insert operation that is already attached to another block"
        );
        let mut block =
            before.borrow().parent().expect("'before' block is not attached to a block");
        {
            let mut block = block.borrow_mut();
            let block_body = block.body_mut();
            let mut cursor = unsafe { block_body.cursor_mut_from_ptr(before) };
            cursor.insert_before(unsafe { OperationRef::from_raw(self) });
        }
        self.block = Some(block);
    }

    pub fn insert_after(&mut self, after: OperationRef) {
        assert!(
            self.block.is_none(),
            "cannot insert operation that is already attached to another block"
        );
        let mut block = after.borrow().parent().expect("'after' block is not attached to a block");
        {
            let mut block = block.borrow_mut();
            let block_body = block.body_mut();
            let mut cursor = unsafe { block_body.cursor_mut_from_ptr(after) };
            cursor.insert_after(unsafe { OperationRef::from_raw(self) });
        }
        self.block = Some(block);
    }
}

/// Movement
impl Operation {
    /// Remove this operation (and its descendants) from its containing block, and delete them
    #[inline]
    pub fn erase(&mut self) {
        // We don't delete entities currently, so for now this is just an alias for `remove`
        self.remove()
    }

    /// Remove the operation from its parent block, but don't delete it.
    pub fn remove(&mut self) {
        let Some(mut parent) = self.block.take() else {
            return;
        };
        let mut block = parent.borrow_mut();
        let body = block.body_mut();
        let mut cursor = unsafe { body.cursor_mut_from_ptr(OperationRef::from_raw(self)) };
        cursor.remove();
    }

    /// Unlink this operation from its current block and insert it right before `ip`, which may
    /// be in the same or another block in the same function.
    pub fn move_before(&mut self, ip: ProgramPoint) {
        self.remove();
        match ip {
            ProgramPoint::Op(other) => {
                self.insert_before(other);
            }
            ProgramPoint::Block(block) => {
                self.insert_at_start(block);
            }
        }
    }

    /// Unlink this operation from its current block and insert it right after `ip`, which may
    /// be in the same or another block in the same function.
    pub fn move_after(&mut self, ip: ProgramPoint) {
        self.remove();
        match ip {
            ProgramPoint::Op(other) => {
                self.insert_after(other);
            }
            ProgramPoint::Block(block) => {
                self.insert_at_end(block);
            }
        }
    }

    /// This drops all operand uses from this operation, which is used to break cyclic dependencies
    /// between references when they are to be deleted
    pub fn drop_all_references(&mut self) {
        self.operands.clear();

        {
            let mut region_cursor = self.regions.front_mut();
            while let Some(mut region) = region_cursor.as_pointer() {
                region.borrow_mut().drop_all_references();
                region_cursor.move_next();
            }
        }

        self.successors.clear();
    }

    /// This drops all uses of any values defined by this operation or its nested regions,
    /// wherever they are located.
    pub fn drop_all_defined_value_uses(&mut self) {
        for result in self.results.iter_mut() {
            let mut res = result.borrow_mut();
            res.uses_mut().clear();
        }

        let mut regions = self.regions.front_mut();
        while let Some(mut region) = regions.as_pointer() {
            let mut region = region.borrow_mut();
            let blocks = region.body_mut();
            let mut cursor = blocks.front_mut();
            while let Some(mut block) = cursor.as_pointer() {
                block.borrow_mut().drop_all_defined_value_uses();
                cursor.move_next();
            }
            regions.move_next();
        }
    }

    /// Drop all uses of results of this operation
    pub fn drop_all_uses(&mut self) {
        for result in self.results.iter_mut() {
            result.borrow_mut().uses_mut().clear();
        }
    }
}

/// Ordering
impl Operation {
    /// This value represents an invalid index ordering for an operation within its containing block
    const INVALID_ORDER: u32 = u32::MAX;
    /// This value represents the stride to use when computing a new order for an operation
    const ORDER_STRIDE: u32 = 5;

    /// Returns true if this operation is an ancestor of `other`.
    ///
    /// An operation is considered its own ancestor, use [Self::is_proper_ancestor_of] if you do not
    /// want this behavior.
    pub fn is_ancestor_of(&self, other: &OperationRef) -> bool {
        let this = self.as_operation_ref();
        OperationRef::ptr_eq(&this, other) || Self::is_a_proper_ancestor_of_b(&this, other)
    }

    /// Returns true if this operation is a proper ancestor of `other`
    pub fn is_proper_ancestor_of(&self, other: &OperationRef) -> bool {
        let this = self.as_operation_ref();
        Self::is_a_proper_ancestor_of_b(&this, other)
    }

    /// Returns true if operation `a` is a proper ancestor of operation `b`
    fn is_a_proper_ancestor_of_b(a: &OperationRef, b: &OperationRef) -> bool {
        let mut next = b.borrow().parent_op();
        while let Some(b) = next.take() {
            if OperationRef::ptr_eq(a, &b) {
                return true;
            }
        }
        false
    }

    /// Given an operation `other` that is within the same parent block, return whether the current
    /// operation is before it in the operation list.
    ///
    /// NOTE: This function has an average complexity of O(1), but worst case may take O(N) where
    /// N is the number of operations within the parent block.
    pub fn is_before_in_block(&self, other: &OperationRef) -> bool {
        use core::sync::atomic::Ordering;

        let block = self.block.clone().expect("operations without parent blocks have no order");
        let other = other.borrow();
        assert!(
            other
                .block
                .as_ref()
                .is_some_and(|other_block| BlockRef::ptr_eq(&block, other_block)),
            "expected both operations to have the same parent block"
        );

        // If the order of the block is already invalid, directly recompute the parent
        if !block.borrow().is_op_order_valid() {
            Self::recompute_block_order(block);
        } else {
            // Update the order of either operation if necessary.
            self.update_order_if_necessary();
            other.update_order_if_necessary();
        }

        self.order.load(Ordering::Relaxed) < other.order.load(Ordering::Relaxed)
    }

    /// Update the order index of this operation of this operation if necessary,
    /// potentially recomputing the order of the parent block.
    fn update_order_if_necessary(&self) {
        use core::sync::atomic::Ordering;

        assert!(self.block.is_some(), "expected valid parent");

        // If the order is valid for this operation there is nothing to do.
        let block = self.block.clone().unwrap();
        if self.has_valid_order() || block.borrow().body().iter().count() == 1 {
            return;
        }

        let this = self.as_operation_ref();
        let prev = this.prev();
        let next = this.next();
        assert!(prev.is_some() || next.is_some(), "expected more than one operation in block");

        // If the operation is at the end of the block.
        if next.is_none() {
            let prev = prev.unwrap();
            let prev = prev.borrow();
            let prev_order = prev.order.load(Ordering::Acquire);
            if prev_order == Self::INVALID_ORDER {
                return Self::recompute_block_order(block);
            }

            // Add the stride to the previous operation.
            self.order.store(prev_order + Self::ORDER_STRIDE, Ordering::Release);
            return;
        }

        // If this is the first operation try to use the next operation to compute the
        // ordering.
        if prev.is_none() {
            let next = next.unwrap();
            let next = next.borrow();
            let next_order = next.order.load(Ordering::Acquire);
            match next_order {
                Self::INVALID_ORDER | 0 => {
                    return Self::recompute_block_order(block);
                }
                // If we can't use the stride, just take the middle value left. This is safe
                // because we know there is at least one valid index to assign to.
                order if order <= Self::ORDER_STRIDE => {
                    self.order.store(order / 2, Ordering::Release);
                }
                _ => {
                    self.order.store(Self::ORDER_STRIDE, Ordering::Release);
                }
            }
            return;
        }

        // Otherwise, this operation is between two others. Place this operation in
        // the middle of the previous and next if possible.
        let prev = prev.unwrap().borrow().order.load(Ordering::Acquire);
        let next = next.unwrap().borrow().order.load(Ordering::Acquire);
        if prev == Self::INVALID_ORDER || next == Self::INVALID_ORDER {
            return Self::recompute_block_order(block);
        }

        // Check to see if there is a valid order between the two.
        if prev + 1 == next {
            return Self::recompute_block_order(block);
        }
        self.order.store(prev + ((next - prev) / 2), Ordering::Release);
    }

    fn recompute_block_order(mut block: BlockRef) {
        use core::sync::atomic::Ordering;

        let mut block = block.borrow_mut();
        let mut cursor = block.body().front();
        let mut index = 0;
        while let Some(op) = cursor.as_pointer() {
            index += Self::ORDER_STRIDE;
            cursor.move_next();
            let ptr = OperationRef::as_ptr(&op);
            unsafe {
                let order_addr = core::ptr::addr_of!((*ptr).order);
                (*order_addr).store(index, Ordering::Release);
            }
        }

        block.mark_op_order_valid();
    }

    /// Returns `None` if this operation has invalid ordering
    #[inline]
    pub(super) fn order(&self) -> Option<u32> {
        use core::sync::atomic::Ordering;
        match self.order.load(Ordering::Acquire) {
            Self::INVALID_ORDER => None,
            order => Some(order),
        }
    }

    /// Returns true if this operation has a valid order
    #[inline(always)]
    pub(super) fn has_valid_order(&self) -> bool {
        self.order().is_some()
    }
}

impl crate::traits::Foldable for Operation {
    fn fold(&self, results: &mut smallvec::SmallVec<[OpFoldResult; 1]>) -> FoldResult {
        use crate::traits::Foldable;

        if let Some(foldable) = self.as_trait::<dyn Foldable>() {
            foldable.fold(results)
        } else {
            FoldResult::Failed
        }
    }

    fn fold_with<'operands>(
        &self,
        operands: &[Option<Box<dyn AttributeValue>>],
        results: &mut smallvec::SmallVec<[OpFoldResult; 1]>,
    ) -> FoldResult {
        use crate::traits::Foldable;

        if let Some(foldable) = self.as_trait::<dyn Foldable>() {
            foldable.fold_with(operands, results)
        } else {
            FoldResult::Failed
        }
    }
}
