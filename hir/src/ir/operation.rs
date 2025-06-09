mod builder;
pub mod equivalence;
mod name;

use alloc::{boxed::Box, rc::Rc};
use core::{
    fmt,
    ptr::{DynMetadata, NonNull, Pointee},
    sync::atomic::AtomicU32,
};

use smallvec::SmallVec;

pub use self::{builder::OperationBuilder, name::OperationName};
use super::{
    effects::{HasRecursiveMemoryEffects, MemoryEffect, MemoryEffectOpInterface},
    *,
};
use crate::{
    adt::SmallSet, patterns::RewritePatternSet, AttributeSet, AttributeValue, Forward, ProgramPoint,
};

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

/// Equality over operations is determined by reference identity, i.e. two operations are only equal
/// if they refer to the same address in memory, regardless of the content of the operation itself.
impl Eq for Operation {}
impl PartialEq for Operation {
    fn eq(&self, other: &Self) -> bool {
        core::ptr::addr_eq(self, other)
    }
}

/// The Hash implementation for operations is defined to match the equality implementation, i.e.
/// the hash of an operation is the hash of its address in memory.
impl core::hash::Hash for Operation {
    fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
        core::ptr::hash(self, state)
    }
}

impl fmt::Debug for Operation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Operation")
            .field_with("name", |f| write!(f, "{}", &self.name()))
            .field("offset", &self.offset)
            .field("order", &self.order)
            .field("attrs", &self.attrs)
            .field("block", &self.parent().as_ref().map(|b| b.borrow().id()))
            .field_with("operands", |f| {
                let mut list = f.debug_list();
                for operand in self.operands().all() {
                    list.entry(&operand.borrow());
                }
                list.finish()
            })
            .field("results", &self.results)
            .field("successors", &self.successors)
            .finish_non_exhaustive()
    }
}

impl fmt::Debug for OperationRef {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        fmt::Debug::fmt(&self.borrow(), f)
    }
}

impl fmt::Display for OperationRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", &self.borrow().name())
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
}
impl EntityListItem for Operation {
    fn on_inserted(this: OperationRef, _cursor: &mut EntityCursorMut<'_, Self>) {
        let parent = this.nearest_symbol_table();
        if let Some(mut parent) = parent {
            // NOTE: We use OperationName, instead of the Operation itself, to avoid borrowing.
            if this.name().implements::<dyn Symbol>()
                && parent.name().implements::<dyn SymbolTable>()
            {
                let mut symbol_table = parent.borrow_mut();
                let sym_manager = symbol_table.as_trait_mut::<dyn SymbolTable>().expect(
                    "Could not cast parent operation {parent.name()} as SymbolTable, even though \
                     it implements said trait",
                );
                let mut sym_manager = sym_manager.symbol_manager_mut();

                let symbol_ref = this.borrow().as_symbol_ref().expect(
                    "Could not cast operation {this.name()} as Symbol, even though it implements \
                     said trait",
                );

                let is_new = sym_manager.insert_new(symbol_ref, ProgramPoint::Invalid);
                assert!(is_new, "{} already exists in {}", this.name(), parent.name());
            };
        }

        let order_offset = core::mem::offset_of!(Operation, order);
        unsafe {
            let ptr = UnsafeIntrusiveEntityRef::as_ptr(&this);
            let order_ptr = ptr.byte_add(order_offset).cast::<AtomicU32>();
            (*order_ptr).store(Self::INVALID_ORDER, core::sync::atomic::Ordering::Release);
        }
    }

    fn on_transfer(_this: OperationRef, _from: &mut EntityList<Self>, to: &mut EntityList<Self>) {
        // Invalidate the ordering of the new parent block
        let mut to = to.parent();
        to.borrow_mut().invalidate_op_order();
    }

    fn on_removed(this: OperationRef, _list: &mut EntityCursorMut<'_, Self>) {
        let parent = this.nearest_symbol_table();
        if let Some(mut parent) = parent {
            // NOTE: We use OperationName, instead of the Operation itself, to avoid borrowing.
            if this.name().implements::<dyn Symbol>()
                && parent.name().implements::<dyn SymbolTable>()
            {
                let mut symbol_table = parent.borrow_mut();
                let sym_manager = symbol_table.as_trait_mut::<dyn SymbolTable>().expect(
                    "Could not cast parent operation {parent.name()} as SymbolTable, even though \
                     it implements said trait",
                );
                let mut sym_manager = sym_manager.symbol_manager_mut();

                let symbol_ref = this.borrow().as_symbol_ref().expect(
                    "Could not cast operation {this.name()} as Symbol, even though it implements \
                     said trait",
                );

                sym_manager.remove(symbol_ref);
            };
        }
    }
}

impl EntityParent<Region> for Operation {
    fn offset() -> usize {
        core::mem::offset_of!(Operation, regions)
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
            operands: Default::default(),
            results: Default::default(),
            successors: Default::default(),
            regions: Default::default(),
        }
    }
}

/// Read-only Metadata
impl OperationRef {
    pub fn name(&self) -> OperationName {
        let ptr = OperationRef::as_ptr(self);
        // SAFETY: The `name` field of Operation is read-only after an op is allocated, and the
        // safety guarantees of OperationRef require that the allocation never moves for the
        // lifetime of the ref. So it is always safe to read this field via direct pointer, even
        // if a mutable borrow of the containing op exists, because the field is never written to
        // after allocation.
        unsafe {
            let name_ptr = core::ptr::addr_of!((*ptr).name);
            OperationName::clone(&*name_ptr)
        }
    }

    pub fn insert_at_start(&self, mut block: BlockRef) {
        assert!(
            self.parent().is_none(),
            "cannot insert operation that is already attached to another block"
        );
        {
            let mut block = block.borrow_mut();
            block.body_mut().push_front(*self);
        }
    }

    pub fn insert_at_end(&self, mut block: BlockRef) {
        assert!(
            self.parent().is_none(),
            "cannot insert operation that is already attached to another block"
        );
        {
            let mut block = block.borrow_mut();
            block.body_mut().push_back(*self);
        }
    }

    /// Returns a handle to the nearest containing [Operation] of this operation, if it is attached
    /// to one
    pub fn parent_op(&self) -> Option<OperationRef> {
        self.parent_region().and_then(|region| region.parent())
    }

    /// Returns a handle to the containing [Region] of this operation, if it is attached to one
    pub fn parent_region(&self) -> Option<RegionRef> {
        self.parent().and_then(|block| block.parent())
    }

    /// Returns the nearest [SymbolTable] from this operation.
    ///
    /// Returns `None` if no parent of this operation is a valid symbol table.
    pub fn nearest_symbol_table(&self) -> Option<OperationRef> {
        let mut parent = self.parent_op();
        while let Some(parent_op) = parent.take() {
            if parent_op.name().implements::<dyn SymbolTable>() {
                return Some(parent_op);
            }
            parent = parent_op.parent_op();
        }
        None
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
    pub fn verify(&self) -> Result<(), Report> {
        let dyn_op: &dyn Op = self.as_ref();
        dyn_op.verify(self.context())
    }

    /// Run any verifiers for this operation, and all of its nested operations, recursively.
    ///
    /// The verification is performed in post-order, so that when the verifier(s) for `self` are
    /// run, it is known that all of its children have successfully verified.
    pub fn recursively_verify(&self) -> Result<(), Report> {
        self.postwalk(|op: &Operation| op.verify().into()).into_result()
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
        //
        // Additionally, this relies on the fact that Op implementations are #[repr(C)] and ensure
        // that their Operation field is always first in the generated struct
        unsafe { OperationRef::from_raw(self) }
    }

    /// Returns true if the concrete type of this operation is `T`
    #[inline]
    pub fn is<T: 'static>(&self) -> bool {
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
    pub fn downcast_ref<T: 'static>(&self) -> Option<&T> {
        self.name.downcast_ref::<T>(self.container())
    }

    /// Attempt to downcast to the concrete [Op] type of this operation
    pub fn downcast_mut<T: 'static>(&mut self) -> Option<&mut T> {
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

    /// Set the intrinsic attribute `name` with `value` for this function.
    pub fn set_intrinsic_attribute(
        &mut self,
        name: impl Into<interner::Symbol>,
        value: Option<impl AttributeValue>,
    ) {
        self.attrs.set(crate::Attribute {
            name: name.into(),
            value: value.map(|v| Box::new(v) as Box<dyn AttributeValue>),
            intrinsic: true,
        });
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
        attr_name: impl Into<interner::Symbol>,
        symbol: impl AsSymbolRef,
    ) {
        let attr_name = attr_name.into();
        let mut symbol = symbol.as_symbol_ref();

        // Do not allow self-references
        //
        // NOTE: We are using this somewhat convoluted way to check identity of the symbol,
        // so that we do not attempt to borrow `self` again if `symbol` and `self` are the
        // same operation. That would fail due to the mutable reference to `self` we are
        // already holding.
        let (data_ptr, _) = SymbolRef::as_ptr(&symbol).to_raw_parts();
        assert!(
            !core::ptr::addr_eq(data_ptr, self.container()),
            "a symbol cannot use itself, except via nested operations"
        );

        // Track the usage of `symbol` by `self`
        let user = self.context().alloc_tracked(SymbolUse {
            owner: self.as_operation_ref(),
            attr: attr_name,
        });

        // Store the underlying attribute value
        if self.has_attribute(attr_name) {
            let attr = self.get_typed_attribute_mut::<SymbolPathAttr>(attr_name).unwrap();
            let symbol = symbol.borrow();
            assert!(
                !attr.user.is_linked(),
                "attempted to replace symbol use without unlinking the previously used symbol \
                 first"
            );
            attr.user = user;
            attr.path = symbol.path();
        } else {
            let attr = {
                let symbol = symbol.borrow();
                SymbolPathAttr {
                    user,
                    path: symbol.path(),
                }
            };
            self.set_attribute(attr_name, Some(attr));
        }

        symbol.borrow_mut().insert_use(user);
    }
}

/// Navigation
impl Operation {
    /// Returns a handle to the containing [Block] of this operation, if it is attached to one
    #[inline]
    pub fn parent(&self) -> Option<BlockRef> {
        self.as_operation_ref().parent()
    }

    /// NOTE: this is a duplicate of OperationRef::parent_region
    /// Returns a handle to the containing [Region] of this operation, if it is attached to one
    pub fn parent_region(&self) -> Option<RegionRef> {
        self.parent().and_then(|block| block.parent())
    }

    /// NOTE: this is a duplicate of OperationRef::parent_region
    /// Returns a handle to the nearest containing [Operation] of this operation, if it is attached
    /// to one
    pub fn parent_op(&self) -> Option<OperationRef> {
        self.parent_region().and_then(|region| region.parent())
    }

    /// Returns a handle to the nearest containing [Operation] of type `T` for this operation, if it
    /// is attached to one
    pub fn nearest_parent_op<T: Op>(&self) -> Option<UnsafeIntrusiveEntityRef<T>> {
        let mut parent = self.parent_op();
        while let Some(op) = parent.take() {
            parent =
                op.parent().and_then(|block| block.parent()).and_then(|region| region.parent());
            let op = op.borrow();
            if let Some(t_ref) = op.downcast_ref::<T>() {
                return Some(unsafe { UnsafeIntrusiveEntityRef::from_raw(t_ref) });
            }
        }
        None
    }
}

/// Traversal
impl Operation {
    pub fn prewalk_all<F>(&self, callback: F)
    where
        F: FnMut(&Operation),
    {
        Walk::<Operation>::prewalk_all::<Forward, _>(self, callback);
    }

    pub fn prewalk<F, B>(&self, callback: F) -> WalkResult<B>
    where
        F: FnMut(&Operation) -> WalkResult<B>,
    {
        Walk::<Operation>::prewalk::<Forward, _, _>(self, callback)
    }

    pub fn postwalk_all<F>(&self, callback: F)
    where
        F: FnMut(&Operation),
    {
        Walk::<Operation>::postwalk_all::<Forward, _>(self, callback);
    }

    pub fn postwalk<F, B>(&self, callback: F) -> WalkResult<B>
    where
        F: FnMut(&Operation) -> WalkResult<B>,
    {
        Walk::<Operation>::postwalk::<Forward, _, _>(self, callback)
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
            dest: info.block,
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
            dest: info.block,
            arguments: self.operands.group_mut(info.operand_group as usize),
        }
    }

    /// Get a reference to the successor at `index`
    #[inline]
    #[track_caller]
    pub fn successor(&self, index: usize) -> OpSuccessor<'_> {
        let info = &self.successors[index];
        OpSuccessor {
            dest: info.block,
            arguments: self.operands.group(info.operand_group as usize),
        }
    }

    /// Get a mutable reference to the successor at `index`
    #[inline]
    #[track_caller]
    pub fn successor_mut(&mut self, index: usize) -> OpSuccessorMut<'_> {
        let info = self.successors[index];
        OpSuccessorMut {
            dest: info.block,
            arguments: self.operands.group_mut(info.operand_group as usize),
        }
    }

    /// Get an iterator over the successors of this operation
    pub fn successor_iter(&self) -> impl DoubleEndedIterator<Item = OpSuccessor<'_>> + '_ {
        self.successors.iter().map(|info| OpSuccessor {
            dest: info.block,
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
    pub fn set_operands(&mut self, operands: impl IntoIterator<Item = ValueRef>) {
        self.operands.clear();
        let context = self.context_rc();
        let owner = self.as_operation_ref();
        self.operands.extend(
            operands
                .into_iter()
                .enumerate()
                .map(|(index, value)| context.make_operand(value, owner, index as u8)),
        );
    }

    /// Replace any uses of `from` with `to` within this operation
    pub fn replaces_uses_of_with(&mut self, from: ValueRef, to: ValueRef) {
        if ValueRef::ptr_eq(&from, &to) {
            return;
        }

        for operand in self.operands.iter_mut() {
            debug_assert!(operand.is_linked());
            if ValueRef::ptr_eq(&from, &operand.borrow().value.unwrap()) {
                operand.borrow_mut().set(to);
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
            if (*result as ValueRef) == replacement {
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
            let mut result = *result as ValueRef;
            if result == replacement {
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
                    used_by = Some(used.owner);
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
        let mut effecting_ops = SmallVec::<[OperationRef; 4]>::from_iter([self.as_operation_ref()]);
        while let Some(op) = effecting_ops.pop() {
            let op = op.borrow();

            // If the operation has recursive effects, push all of the nested operations on to the
            // stack to consider
            let has_recursive_effects = op.implements::<dyn HasRecursiveMemoryEffects>();
            if has_recursive_effects {
                for region in op.regions() {
                    for block in region.body() {
                        for op in block.body() {
                            effecting_ops.push(op.as_operation_ref());
                        }
                    }
                }
            }

            // If the op has memory effects, try to characterize them to see if the op is trivially
            // dead here.
            if let Some(effect_interface) = op.as_trait::<dyn MemoryEffectOpInterface>() {
                let mut effects = effect_interface.effects();

                // Gather all results of this op that are allocated
                let mut alloc_results = SmallSet::<ValueRef, 4>::default();
                for effect in effects.as_slice() {
                    let allocates = matches!(effect.effect(), MemoryEffect::Allocate);
                    if let Some(value) = effect.value() {
                        let is_defined_by_op = value
                            .borrow()
                            .get_defining_op()
                            .is_some_and(|op| self.as_operation_ref() == op);
                        if allocates && is_defined_by_op {
                            alloc_results.insert(value);
                        }
                    }
                }

                if !effects.all(|effect| {
                    // We can drop effects if the value is an allocation and is a result of
                    // the operation
                    if effect.value().is_some_and(|v| alloc_results.contains(&v)) {
                        true
                    } else {
                        // Otherwise, the effect must be a read
                        matches!(effect.effect(), MemoryEffect::Read)
                    }
                }) {
                    return false;
                }
                continue;
            }

            // Otherwise, if the op has recursive side effects we can treat the operation itself
            // as having no effects
            if has_recursive_effects {
                continue;
            }

            // If there were no effect interfaces, we treat this op as conservatively having
            // effects
            return false;
        }

        // If we get here, none of the operations had effects that prevented marking this operation
        // as dead.
        true
    }

    /// Returns true if the given operation is free of memory effects.
    ///
    /// An operation is free of memory effects if its implementation of `MemoryEffectOpInterface`
    /// indicates that it has no memory effects. For example, it may implement `NoMemoryEffect`.
    /// Alternatively, if the operation has the `HasRecursiveMemoryEffects` trait, then it is free
    /// of memory effects if all of its nested operations are free of memory effects.
    ///
    /// If the operation has both, then it is free of memory effects if both conditions are
    /// satisfied.
    pub fn is_memory_effect_free(&self) -> bool {
        if let Some(mem_interface) = self.as_trait::<dyn MemoryEffectOpInterface>() {
            if !mem_interface.has_no_effect() {
                return false;
            }

            // If the op does not have recursive side effects, then it is memory effect free
            if !self.implements::<dyn HasRecursiveMemoryEffects>() {
                return true;
            }
        } else if !self.implements::<dyn HasRecursiveMemoryEffects>() {
            // Otherwise, if the op does not implement the memory effect interface and it does not
            // have recursive side effects, then it cannot be known that the op is moveable.
            return false;
        }

        // Recurse into the regions and ensure that all nested ops are memory effect free
        for region in self.regions() {
            let walk_result = region.prewalk(|op| {
                if !op.is_memory_effect_free() {
                    WalkResult::Break(())
                } else {
                    WalkResult::Continue(())
                }
            });
            if walk_result.was_interrupted() {
                return false;
            }
        }

        true
    }
}

/// Insertion
impl Operation {
    pub fn insert_before(&mut self, before: OperationRef) {
        assert!(
            self.parent().is_none(),
            "cannot insert operation that is already attached to another block"
        );
        let mut block = before.parent().expect("'before' block is not attached to a block");
        {
            let mut block = block.borrow_mut();
            let block_body = block.body_mut();
            let mut cursor = unsafe { block_body.cursor_mut_from_ptr(before) };
            cursor.insert_before(self.as_operation_ref());
        }
    }

    pub fn insert_after(&mut self, after: OperationRef) {
        assert!(
            self.parent().is_none(),
            "cannot insert operation that is already attached to another block"
        );
        let mut block = after.parent().expect("'after' block is not attached to a block");
        {
            let mut block = block.borrow_mut();
            let block_body = block.body_mut();
            let mut cursor = unsafe { block_body.cursor_mut_from_ptr(after) };
            cursor.insert_after(self.as_operation_ref());
        }
    }
}

/// Movement
impl Operation {
    /// Remove this operation (and its descendants) from its containing block, and delete them
    #[inline]
    pub fn erase(&mut self) {
        // We don't delete entities currently, so for now this is just an alias for `remove`
        self.remove();

        self.successors.clear();
        self.operands.clear();
    }

    /// Remove the operation from its parent block, but don't delete it.
    pub fn remove(&mut self) {
        if let Some(mut parent) = self.parent() {
            let mut block = parent.borrow_mut();
            let body = block.body_mut();
            let mut cursor = unsafe { body.cursor_mut_from_ptr(self.as_operation_ref()) };
            cursor.remove();
        }
    }

    /// Unlink this operation from its current block and insert it at `ip`, which may be in the same
    /// or another block in the same function.
    ///
    /// # Panics
    ///
    /// This function will panic if the given program point is unset, or refers to an orphaned op,
    /// i.e. an op that has no parent block.
    pub fn move_to(&mut self, mut ip: ProgramPoint) {
        let this = self.as_operation_ref();
        if let Some(op) = ip.operation() {
            if op == this {
                // The move is a no-op
                return;
            }

            assert!(ip.block().is_some(), "cannot insert an operation relative to an orphaned op");
        }

        // Detach `self`
        self.remove();

        {
            // Move `self` to `ip`
            let mut cursor = ip.cursor_mut().expect("insertion point is invalid/unset");
            // NOTE: We use `insert_after` here because the cursor we get is positioned such that
            // insert_after will always insert at the precise point specified.
            cursor.insert_after(self.as_operation_ref());
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
    pub fn is_ancestor_of(&self, other: &Self) -> bool {
        core::ptr::addr_eq(self, other) || Self::is_a_proper_ancestor_of_b(self, other)
    }

    /// Returns true if this operation is a proper ancestor of `other`
    pub fn is_proper_ancestor_of(&self, other: &Self) -> bool {
        Self::is_a_proper_ancestor_of_b(self, other)
    }

    /// Returns true if operation `a` is a proper ancestor of operation `b`
    fn is_a_proper_ancestor_of_b(a: &Self, b: &Self) -> bool {
        let a = a.as_operation_ref();
        let mut next = b.parent_op();
        while let Some(b) = next.take() {
            if OperationRef::ptr_eq(&a, &b) {
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

        let block = self.parent().expect("operations without parent blocks have no order");
        let other = other.borrow();
        assert!(
            other
                .parent()
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

        assert!(self.parent().is_some(), "expected valid parent");

        // If the order is valid for this operation there is nothing to do.
        let block = self.parent().unwrap();
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

    fn recompute_block_order(block: BlockRef) {
        use core::sync::atomic::Ordering;

        let block = block.borrow();
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
    pub(crate) fn order(&self) -> Option<u32> {
        use core::sync::atomic::Ordering;
        match self.order.load(Ordering::Acquire) {
            Self::INVALID_ORDER => None,
            order => Some(order),
        }
    }

    /// Returns `None` if this operation has invalid ordering
    #[inline]
    #[allow(unused)]
    pub(crate) fn get_or_compute_order(&self) -> u32 {
        use core::sync::atomic::Ordering;

        if let Some(order) = self.order() {
            return order;
        }

        Self::recompute_block_order(
            self.parent().expect("cannot compute block ordering for orphaned operation"),
        );

        self.order.load(Ordering::Acquire)
    }

    /// Returns true if this operation has a valid order
    #[inline(always)]
    pub(super) fn has_valid_order(&self) -> bool {
        self.order().is_some()
    }
}

/// Canonicalization
impl Operation {
    /// Populates `rewrites` with the set of canonicalization patterns registered for this operation
    #[inline]
    pub fn populate_canonicalization_patterns(
        &self,
        rewrites: &mut RewritePatternSet,
        context: Rc<Context>,
    ) {
        self.name.populate_canonicalization_patterns(rewrites, context);
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
