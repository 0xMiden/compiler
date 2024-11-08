use core::{any::Any, fmt, hash::Hash, ptr::NonNull};

use super::ProgramPoint;
use crate::{BlockRef, DynHash, DynPartialEq, Insert, OperationRef, Spanned, ValueRef};

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
    pub(super) fn new(raw: NonNull<dyn LatticeAnchor>) -> Self {
        Self(raw)
    }

    pub fn compute_hash<A>(anchor: &A) -> u64
    where
        A: LatticeAnchor + Hash,
    {
        use core::hash::Hasher;

        let mut hasher = rustc_hash::FxHasher::default();
        anchor.hash(&mut hasher);
        hasher.finish()
    }
}

impl core::ops::Deref for LatticeAnchorRef {
    type Target = dyn LatticeAnchor;

    #[inline(always)]
    fn deref(&self) -> &Self::Target {
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
    fn span(&self) -> miden_assembly::SourceSpan {
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

impl LatticeAnchor for LatticeAnchorRef {
    fn as_any(&self) -> &dyn Any {
        LatticeAnchor::as_any(unsafe { self.0.as_ref() })
    }

    fn is_value(&self) -> bool {
        unsafe { self.0.as_ref().is_value() }
    }

    fn as_value(&self) -> Option<ValueRef> {
        unsafe { self.0.as_ref().as_value() }
    }

    fn is_valid_program_point(&self) -> bool {
        unsafe { self.0.as_ref().is_valid_program_point() }
    }

    fn as_program_point(&self) -> Option<ProgramPoint> {
        unsafe { self.0.as_ref().as_program_point() }
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
    fn as_any(&self) -> &dyn Any;
    fn is_value(&self) -> bool {
        LatticeAnchor::as_any(self).is::<ValueRef>()
    }
    fn as_value(&self) -> Option<ValueRef> {
        LatticeAnchor::as_any(self).downcast_ref::<ValueRef>().copied()
    }
    fn is_valid_program_point(&self) -> bool {
        let any = LatticeAnchor::as_any(self);
        any.is::<ProgramPoint>() || any.is::<OperationRef>() || any.is::<BlockRef>()
    }
    fn as_program_point(&self) -> Option<ProgramPoint> {
        let any = LatticeAnchor::as_any(self);
        if let Some(pp) = any.downcast_ref::<ProgramPoint>() {
            Some(*pp)
        } else if let Some(op) = any.downcast_ref::<OperationRef>().cloned() {
            let block = op.borrow().parent();
            Some(ProgramPoint::Op {
                block,
                op,
                point: Insert::Before,
            })
        } else {
            any.downcast_ref::<BlockRef>().copied().map(|block| ProgramPoint::Block {
                block,
                point: Insert::Before,
            })
        }
    }
}

impl dyn LatticeAnchor {
    #[inline]
    pub fn is<T: 'static>(&self) -> bool {
        self.as_any().is::<T>()
    }

    #[inline]
    pub fn downcast_ref<T: 'static>(&self) -> Option<&T> {
        self.as_any().downcast_ref()
    }
}

pub(super) trait LatticeAnchorExt: LatticeAnchor {
    fn intern(
        self,
        alloc: &blink_alloc::Blink,
        interned: &mut crate::FxHashMap<u64, LatticeAnchorRef>,
    ) -> LatticeAnchorRef;
}

impl<A: LatticeAnchor + Hash> LatticeAnchorExt for A {
    default fn intern(
        self,
        alloc: &blink_alloc::Blink,
        interned: &mut crate::FxHashMap<u64, LatticeAnchorRef>,
    ) -> LatticeAnchorRef {
        let hash = LatticeAnchorRef::compute_hash(&self);
        *interned.entry(hash).or_insert_with(|| {
            let anchor = alloc.put(self);
            LatticeAnchorRef::new(unsafe { NonNull::new_unchecked(anchor) })
        })
    }
}
impl LatticeAnchorExt for LatticeAnchorRef {
    #[inline(always)]
    fn intern(
        self,
        _alloc: &blink_alloc::Blink,
        _interned: &mut crate::FxHashMap<u64, LatticeAnchorRef>,
    ) -> LatticeAnchorRef {
        self
    }
}

impl LatticeAnchor for ValueRef {
    fn as_any(&self) -> &dyn Any {
        self
    }

    #[inline(always)]
    fn is_value(&self) -> bool {
        true
    }

    #[inline(always)]
    fn is_valid_program_point(&self) -> bool {
        false
    }

    #[inline]
    fn as_value(&self) -> Option<ValueRef> {
        Some(*self)
    }

    #[inline(always)]
    fn as_program_point(&self) -> Option<ProgramPoint> {
        None
    }
}
impl LatticeAnchor for ProgramPoint {
    fn as_any(&self) -> &dyn Any {
        self
    }

    #[inline(always)]
    fn is_value(&self) -> bool {
        false
    }

    #[inline(always)]
    fn is_valid_program_point(&self) -> bool {
        true
    }

    #[inline(always)]
    fn as_value(&self) -> Option<ValueRef> {
        None
    }

    #[inline]
    fn as_program_point(&self) -> Option<ProgramPoint> {
        Some(*self)
    }
}
impl LatticeAnchor for OperationRef {
    fn as_any(&self) -> &dyn Any {
        self
    }

    #[inline(always)]
    fn is_value(&self) -> bool {
        false
    }

    #[inline(always)]
    fn is_valid_program_point(&self) -> bool {
        true
    }

    #[inline(always)]
    fn as_value(&self) -> Option<ValueRef> {
        None
    }

    #[inline]
    fn as_program_point(&self) -> Option<ProgramPoint> {
        Some(ProgramPoint::before(*self))
    }
}
impl LatticeAnchor for BlockRef {
    fn as_any(&self) -> &dyn Any {
        self
    }

    #[inline(always)]
    fn is_value(&self) -> bool {
        false
    }

    #[inline(always)]
    fn is_valid_program_point(&self) -> bool {
        true
    }

    #[inline(always)]
    fn as_value(&self) -> Option<ValueRef> {
        None
    }

    #[inline]
    fn as_program_point(&self) -> Option<ProgramPoint> {
        Some(ProgramPoint::before(*self))
    }
}
