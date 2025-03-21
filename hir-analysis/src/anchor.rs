use core::{any::Any, fmt, hash::Hash, ptr::NonNull};

use midenc_hir::{
    Block, BlockArgument, BlockArgumentRef, BlockRef, DynHash, DynPartialEq, FxHashMap, FxHasher,
    OpResult, OpResultRef, Operation, OperationRef, ProgramPoint, RawEntityRef, SourceSpan,
    Spanned, Value, ValueRef,
};

/// This represents a pointer to a type-erased [LatticeAnchor] value.
///
/// # Safety
///
/// Anchors are immutable, so dereferencing these are always safe while the [DataFlowSolver] which
/// allocated them is still live. However, you must ensure that a reference never outlives the
/// parent [DataFlowSolver]. In practice, this is basically enforced in terms of API - you can't
/// do anything useful with one of these without the solver, however it is still incumbent on users
/// of this type to uphold this guarantee.
#[derive(Copy, Clone)]
pub struct LatticeAnchorRef(NonNull<dyn LatticeAnchor>);

impl LatticeAnchorRef {
    /// Get a [LatticeAnchorRef] from a raw [LatticeAnchor] pointer.
    #[inline]
    fn new(raw: NonNull<dyn LatticeAnchor>) -> Self {
        Self(raw)
    }

    fn compute_hash<A>(anchor: &A) -> u64
    where
        A: ?Sized + LatticeAnchor,
    {
        use core::hash::Hasher;

        let mut hasher = FxHasher::default();
        anchor.dyn_hash(&mut hasher);
        hasher.finish()
    }

    pub fn intern<A>(
        anchor: &A,
        alloc: &blink_alloc::Blink,
        interned: &mut FxHashMap<u64, LatticeAnchorRef>,
    ) -> LatticeAnchorRef
    where
        A: LatticeAnchorExt,
    {
        let hash = anchor.anchor_id();
        *interned
            .entry(hash)
            .or_insert_with(|| <A as LatticeAnchorExt>::alloc(anchor, alloc))
    }
}

impl core::ops::Deref for LatticeAnchorRef {
    type Target = dyn LatticeAnchor;

    #[inline(always)]
    fn deref(&self) -> &Self::Target {
        unsafe { self.0.as_ref() }
    }
}

impl core::convert::AsRef<dyn LatticeAnchor> for LatticeAnchorRef {
    #[inline(always)]
    fn as_ref(&self) -> &dyn LatticeAnchor {
        unsafe { self.0.as_ref() }
    }
}

impl Eq for LatticeAnchorRef {}

impl PartialEq for LatticeAnchorRef {
    fn eq(&self, other: &Self) -> bool {
        unsafe { self.0.as_ref().dyn_eq(other.0.as_ref()) }
    }
}

impl Hash for LatticeAnchorRef {
    fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
        unsafe {
            self.0.as_ref().dyn_hash(state);
        }
    }
}

impl Spanned for LatticeAnchorRef {
    fn span(&self) -> SourceSpan {
        unsafe { self.0.as_ref().span() }
    }
}

impl fmt::Debug for LatticeAnchorRef {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        fmt::Debug::fmt(unsafe { self.0.as_ref() }, f)
    }
}

impl fmt::Display for LatticeAnchorRef {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        fmt::Display::fmt(unsafe { self.0.as_ref() }, f)
    }
}

/// An abstraction over lattice anchors.
///
/// In classical data-flow analysis, lattice anchors represent positions in a program to which
/// lattice elements are attached. In sparse data-flow analysis, these can be SSA values, and in
/// dense data-flow analysis, these are the program points before and after every operation.
///
/// [LatticeAnchor] provides the means to represent and work with any type of anchor.
pub trait LatticeAnchor:
    Any + Spanned + fmt::Debug + fmt::Display + DynPartialEq + DynHash
{
    fn is_value(&self) -> bool {
        false
    }

    fn as_value(&self) -> Option<ValueRef> {
        None
    }

    fn is_valid_program_point(&self) -> bool {
        false
    }

    fn as_program_point(&self) -> Option<ProgramPoint> {
        None
    }
}

impl LatticeAnchor for LatticeAnchorRef {
    #[inline]
    fn is_value(&self) -> bool {
        self.as_ref().is_value()
    }

    #[inline]
    fn as_value(&self) -> Option<ValueRef> {
        self.as_ref().as_value()
    }

    #[inline]
    fn is_valid_program_point(&self) -> bool {
        self.as_ref().is_valid_program_point()
    }

    #[inline]
    fn as_program_point(&self) -> Option<ProgramPoint> {
        self.as_ref().as_program_point()
    }
}

impl LatticeAnchor for ProgramPoint {
    fn is_valid_program_point(&self) -> bool {
        true
    }

    fn as_program_point(&self) -> Option<ProgramPoint> {
        Some(*self)
    }
}

impl LatticeAnchor for Operation {
    fn is_valid_program_point(&self) -> bool {
        true
    }

    fn as_program_point(&self) -> Option<ProgramPoint> {
        Some(ProgramPoint::before(self))
    }
}

impl LatticeAnchor for Block {
    fn is_valid_program_point(&self) -> bool {
        true
    }

    fn as_program_point(&self) -> Option<ProgramPoint> {
        Some(ProgramPoint::at_start_of(self))
    }
}

impl LatticeAnchor for BlockArgument {
    fn is_value(&self) -> bool {
        true
    }

    fn as_value(&self) -> Option<ValueRef> {
        Some(self.as_value_ref())
    }
}

impl LatticeAnchor for OpResult {
    fn is_value(&self) -> bool {
        true
    }

    fn as_value(&self) -> Option<ValueRef> {
        Some(self.as_value_ref())
    }
}

impl LatticeAnchor for dyn Value {
    fn is_value(&self) -> bool {
        true
    }

    fn as_value(&self) -> Option<ValueRef> {
        Some(unsafe { ValueRef::from_raw(self) })
    }
}

impl<A: ?Sized + LatticeAnchor, Metadata: 'static> LatticeAnchor for RawEntityRef<A, Metadata> {
    default fn is_value(&self) -> bool {
        false
    }

    default fn as_value(&self) -> Option<ValueRef> {
        None
    }

    default fn is_valid_program_point(&self) -> bool {
        false
    }

    default fn as_program_point(&self) -> Option<ProgramPoint> {
        None
    }
}

impl LatticeAnchor for ValueRef {
    fn is_value(&self) -> bool {
        true
    }

    fn as_value(&self) -> Option<ValueRef> {
        Some(*self)
    }
}

impl LatticeAnchor for BlockArgumentRef {
    fn is_value(&self) -> bool {
        true
    }

    fn as_value(&self) -> Option<ValueRef> {
        Some(*self)
    }
}

impl LatticeAnchor for OpResultRef {
    fn is_value(&self) -> bool {
        true
    }

    fn as_value(&self) -> Option<ValueRef> {
        Some(*self)
    }
}

impl LatticeAnchor for OperationRef {
    fn is_valid_program_point(&self) -> bool {
        true
    }

    fn as_program_point(&self) -> Option<ProgramPoint> {
        Some(ProgramPoint::before(*self))
    }
}

impl LatticeAnchor for BlockRef {
    fn is_valid_program_point(&self) -> bool {
        true
    }

    fn as_program_point(&self) -> Option<ProgramPoint> {
        Some(ProgramPoint::at_start_of(*self))
    }
}

#[doc(hidden)]
pub trait LatticeAnchorExt: sealed::IsLatticeAnchor {
    fn anchor_id(&self) -> u64;

    fn alloc(&self, alloc: &blink_alloc::Blink) -> LatticeAnchorRef;
}

mod sealed {
    use super::LatticeAnchor;

    pub trait IsLatticeAnchor: LatticeAnchor {}
    impl<A: LatticeAnchor> IsLatticeAnchor for A {}
}

impl<A: LatticeAnchor + Clone> LatticeAnchorExt for A {
    default fn anchor_id(&self) -> u64 {
        LatticeAnchorRef::compute_hash(self)
    }

    default fn alloc(&self, alloc: &blink_alloc::Blink) -> LatticeAnchorRef {
        let ptr = alloc.put(self.clone());
        LatticeAnchorRef::new(unsafe { NonNull::new_unchecked(ptr) })
    }
}

impl LatticeAnchorExt for LatticeAnchorRef {
    fn anchor_id(&self) -> u64 {
        LatticeAnchorRef::compute_hash(self.as_ref())
    }

    #[inline(always)]
    fn alloc(&self, _alloc: &blink_alloc::Blink) -> LatticeAnchorRef {
        *self
    }
}

impl LatticeAnchorExt for ValueRef {
    fn anchor_id(&self) -> u64 {
        LatticeAnchorRef::compute_hash(&*self.borrow())
    }

    fn alloc(&self, _alloc: &blink_alloc::Blink) -> LatticeAnchorRef {
        // We do not need to allocate for IR entity refs, as by definition their context outlives
        // the dataflow solver, so we only need to convert the reference to a &dyn LatticeAnchor.
        let value = self.borrow();
        let ptr = if let Some(result) = value.downcast_ref::<OpResult>() {
            result as &dyn LatticeAnchor as *const dyn LatticeAnchor
        } else {
            let arg = value.downcast_ref::<BlockArgument>().unwrap();
            arg as &dyn LatticeAnchor as *const dyn LatticeAnchor
        };
        LatticeAnchorRef::new(unsafe { NonNull::new_unchecked(ptr.cast_mut()) })
    }
}

impl LatticeAnchorExt for BlockArgumentRef {
    fn anchor_id(&self) -> u64 {
        LatticeAnchorRef::compute_hash(&*self.borrow())
    }

    fn alloc(&self, _alloc: &blink_alloc::Blink) -> LatticeAnchorRef {
        let ptr = &*self.borrow() as &dyn LatticeAnchor as *const dyn LatticeAnchor;
        LatticeAnchorRef::new(unsafe { NonNull::new_unchecked(ptr.cast_mut()) })
    }
}

impl LatticeAnchorExt for OpResultRef {
    fn anchor_id(&self) -> u64 {
        LatticeAnchorRef::compute_hash(&*self.borrow())
    }

    fn alloc(&self, _alloc: &blink_alloc::Blink) -> LatticeAnchorRef {
        let ptr = &*self.borrow() as &dyn LatticeAnchor as *const dyn LatticeAnchor;
        LatticeAnchorRef::new(unsafe { NonNull::new_unchecked(ptr.cast_mut()) })
    }
}

impl LatticeAnchorExt for BlockRef {
    fn anchor_id(&self) -> u64 {
        LatticeAnchorRef::compute_hash(&*self.borrow())
    }

    fn alloc(&self, _alloc: &blink_alloc::Blink) -> LatticeAnchorRef {
        let ptr = &*self.borrow() as &dyn LatticeAnchor as *const dyn LatticeAnchor;
        LatticeAnchorRef::new(unsafe { NonNull::new_unchecked(ptr.cast_mut()) })
    }
}

impl LatticeAnchorExt for OperationRef {
    fn anchor_id(&self) -> u64 {
        LatticeAnchorRef::compute_hash(&*self.borrow())
    }

    fn alloc(&self, _alloc: &blink_alloc::Blink) -> LatticeAnchorRef {
        let ptr = &*self.borrow() as &dyn LatticeAnchor as *const dyn LatticeAnchor;
        LatticeAnchorRef::new(unsafe { NonNull::new_unchecked(ptr.cast_mut()) })
    }
}

impl LatticeAnchorExt for ProgramPoint {
    fn anchor_id(&self) -> u64 {
        LatticeAnchorRef::compute_hash(self)
    }

    fn alloc(&self, alloc: &blink_alloc::Blink) -> LatticeAnchorRef {
        let ptr = alloc.put(*self);
        LatticeAnchorRef::new(unsafe { NonNull::new_unchecked(ptr) })
    }
}
