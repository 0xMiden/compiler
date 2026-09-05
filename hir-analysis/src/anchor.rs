use core::{fmt, hash::Hash, ptr::NonNull};

use midenc_hir::{
    Block, BlockArgument, BlockArgumentRef, BlockRef, DynHash, DynPartialEq, FxHashMap, FxHasher,
    OpResult, OpResultRef, Operation, OperationRef, PartialEqable, ProgramPoint, RawEntityRef,
    SmallVec, SourceSpan, Spanned, Value, ValueRef, any::AsAny,
};

/// This represents a pointer to a type-erased [LatticeAnchor] value.
///
/// # Safety
///
/// Anchors are immutable, so dereferencing these are always safe while the [crate::DataFlowSolver]
/// which allocated them is still live. However, you must ensure that a reference never outlives the
/// parent [crate::DataFlowSolver]. In practice, this is basically enforced in terms of API - you
/// can't do anything useful with one of these without the solver, however it is still incumbent on
/// users of this type to uphold this guarantee.
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
        interned: &mut FxHashMap<u64, SmallVec<[LatticeAnchorRef; 1]>>,
    ) -> LatticeAnchorRef
    where
        A: LatticeAnchorExt,
    {
        let hash = anchor.anchor_id();
        let candidates = interned.entry(hash).or_default();
        if let Some(existing) =
            candidates.iter().find(|existing| anchor.equivalent_to(existing.as_ref()))
        {
            return *existing;
        }
        let interned = <A as LatticeAnchorExt>::alloc(anchor, alloc);
        candidates.push(interned);
        interned
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
    AsAny + Spanned + fmt::Debug + fmt::Display + PartialEqable + DynPartialEq + DynHash
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

    /// Compare the same canonical representation used by `anchor_id` and `alloc`.
    fn equivalent_to(&self, other: &dyn LatticeAnchor) -> bool;

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

    default fn equivalent_to(&self, other: &dyn LatticeAnchor) -> bool {
        other.dyn_eq(self)
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
    fn equivalent_to(&self, other: &dyn LatticeAnchor) -> bool {
        other.dyn_eq(self.as_ref())
    }

    fn alloc(&self, _alloc: &blink_alloc::Blink) -> LatticeAnchorRef {
        *self
    }
}

impl LatticeAnchorExt for ValueRef {
    fn anchor_id(&self) -> u64 {
        LatticeAnchorRef::compute_hash(&*self.borrow())
    }

    fn equivalent_to(&self, other: &dyn LatticeAnchor) -> bool {
        other.dyn_eq(&*self.borrow())
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

    fn equivalent_to(&self, other: &dyn LatticeAnchor) -> bool {
        other.dyn_eq(&*self.borrow())
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

    fn equivalent_to(&self, other: &dyn LatticeAnchor) -> bool {
        other.dyn_eq(&*self.borrow())
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

    fn equivalent_to(&self, other: &dyn LatticeAnchor) -> bool {
        other.dyn_eq(&*self.borrow())
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

    fn equivalent_to(&self, other: &dyn LatticeAnchor) -> bool {
        other.dyn_eq(&*self.borrow())
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

#[cfg(test)]
mod tests {
    use alloc::rc::Rc;
    use core::hash::Hasher;

    use midenc_hir::{Context, Type};

    use super::*;
    use crate::{DataFlowSolver, Lattice};

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct CollidingAnchor(u8);

    impl Hash for CollidingAnchor {
        fn hash<H: Hasher>(&self, state: &mut H) {
            0u8.hash(state);
        }
    }

    impl fmt::Display for CollidingAnchor {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "anchor({})", self.0)
        }
    }

    impl Spanned for CollidingAnchor {
        fn span(&self) -> SourceSpan {
            SourceSpan::UNKNOWN
        }
    }

    impl LatticeAnchor for CollidingAnchor {}

    #[test]
    fn colliding_anchors_keep_independent_analysis_states() {
        let mut solver = DataFlowSolver::default();
        for (anchor, value) in [(CollidingAnchor(1), 11), (CollidingAnchor(2), 22)] {
            let mut state = solver.get_or_create_mut::<Lattice<u32>, _>(anchor);
            *state.value_mut() = value;
        }
        assert_eq!(*solver.get::<Lattice<u32>, _>(&CollidingAnchor(1)).unwrap().value(), 11);
        assert_eq!(*solver.get::<Lattice<u32>, _>(&CollidingAnchor(2)).unwrap().value(), 22);

        let first = solver.create_lattice_anchor(CollidingAnchor(1));
        let repeated = solver.create_lattice_anchor(CollidingAnchor(1));
        let other = solver.create_lattice_anchor(CollidingAnchor(2));
        assert!(core::ptr::addr_eq(first.as_ref(), repeated.as_ref()));
        assert_ne!(first, other);
    }

    #[test]
    fn typed_and_erased_value_anchors_share_analysis_state() {
        let context = Rc::new(Context::default());
        let block = context.create_block_with_params([Type::U32]);
        let argument = block.borrow().arguments()[0];
        let value = argument as ValueRef;
        let mut solver = DataFlowSolver::default();
        {
            let mut state = solver.get_or_create_mut::<Lattice<u32>, _>(argument);
            *state.value_mut() = 42;
        }
        assert_eq!(*solver.get::<Lattice<u32>, _>(&value).unwrap().value(), 42);
        let typed = solver.create_lattice_anchor(argument);
        let erased = solver.create_lattice_anchor(value);
        let reinterned = solver.create_lattice_anchor(erased);
        assert!(core::ptr::addr_eq(typed.as_ref(), erased.as_ref()));
        assert!(core::ptr::addr_eq(typed.as_ref(), reinterned.as_ref()));
    }
}
