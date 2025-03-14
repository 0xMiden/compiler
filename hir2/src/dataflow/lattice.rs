use core::{convert::AsRef, fmt};

use super::{
    AnalysisState, BuildableAnalysisState, ChangeResult, DenseLattice, LatticeAnchor,
    LatticeAnchorRef, SparseLattice,
};

/// This trait must be implemented for any value that exhibits the properties of a
/// [Lattice](https://en.wikipedia.org/wiki/Lattice_(order)#Definition).
///
/// Lattices can either be bounded or unbounded, however in the specific case of data-flow analysis,
/// virtually all lattices are going to be bounded, in order to represent two important states:
///
/// * Undefined (i.e. not yet known, _bottom_). This state represents the initial state
///   of the lattice, as well as its minimum value.
/// * Overdefined (i.e. cannot be known, _top_). This state represents the "failure" state of
///   the lattice, as well as its maximum value. This is almost always used to signal that an
///   analysis has reached conflicting conclusions, or has discovered information that makes it
///   impossible to draw a conclusion at all.
///
/// These "bound" the values of the lattice, and all valid values of the lattice are partially (or
/// totally) ordered with respect to these bounds. For example, integer range analysis will start
/// in the _undefined_ state, as it does not yet know anything about integer values in the program.
/// It will then start to visit integer values in the program, and will either identify a valid
/// range for the value, refine the range for a value (i.e. new information makes the range
/// narrower or wider), or determine that the range cannot be known (i.e. the _overdefined_ state,
/// but also conveniently, the "maximum" range of an finite integral value is equivalent to saying
/// that the range covers all valid values of that type).
///
/// It is permitted to implement this trait for semi-lattices (i.e. values for which only `join`
/// or `meet` are well-defined), however the implementation of whichever method is _not_ well-
/// defined must assert/panic, to ensure the value is not improperly used in an analysis that relies
/// on the properties of a join (or meet) semi-lattice for correctness.
pub trait LatticeLike: Default + Clone + Eq + fmt::Debug + 'static {
    /// Joins `self` with `other`, producing a new value that represents the least upper bound of
    /// the two values in the join semi-lattice of the type.
    ///
    /// Formally, the join of two values is represented by the binary operation $\lor$, i.e. logical
    /// disjunction, or more commonly, logical-OR.
    ///
    /// The following are some examples of what joins of non-boolean values look like in practice:
    ///
    /// * The least upper bound of a non-empty set of integers $I$, is the maximum value in $I$.
    /// * The disjunction of two partially-ordered sets is the union of those sets
    /// * The disjunction of two integral ranges is a range that includes the elements of both
    fn join(&self, other: &Self) -> Self;

    /// Meets `self` with `other`, producing a new value that represents the greatest lower bound of
    /// the two values in the meet semi-lattice of the type.
    ///
    /// Formally, the meet of two values is represented by the binary operation $\and$, i.e. logical
    /// conjunction, or more commonly, logical-AND.
    ///
    /// The following are some examples of what meets of non-boolean values look like in practice:
    ///
    /// * The greatest lower bound of a non-empty set of integers $I$, is the minimum value in $I$.
    /// * The conjunction of two partially-ordered sets is the intersection of those sets
    /// * The conjunction of two non-overlapping integral ranges is an empty range
    ///   (i.e. _overdefined_).
    /// * The conjunction of two overlapping integral ranges is the overlapping range.
    fn meet(&self, other: &Self) -> Self;
}

/// This type adapts a [LatticeLike] value for use as an [AnalysisState] by a [DataFlowAnalysis].
pub struct Lattice<T> {
    anchor: LatticeAnchorRef,
    value: T,
}

impl<T> Lattice<T> {
    /// Construct a new [Lattice] from the given anchor and [LatticeLike] value.
    pub fn new(anchor: LatticeAnchorRef, value: T) -> Self {
        Self { anchor, value }
    }

    /// Get a reference to the underlying lattice value
    pub fn value(&self) -> &T {
        &self.value
    }

    /// Get a mutable reference to the underlying lattice value
    pub fn value_mut(&mut self) -> &mut T {
        &mut self.value
    }
}

impl<T: core::fmt::Debug> core::fmt::Debug for Lattice<T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        core::fmt::Debug::fmt(&self.value, f)
    }
}

impl<T: core::fmt::Display> core::fmt::Display for Lattice<T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        core::fmt::Display::fmt(&self.value, f)
    }
}

/// Any type that has a default value is buildable
///
/// NOTE: The default value is expected to correspond to the _minimum_ state of the lattice,
/// typically a value that represents "unknown" or most conservative state. Otherwise, the
/// implementation of any analyses that depend on the type are likely to reach the overdefined
/// state when they otherwise would not (due to a non-minimal value "conflicting" with the value
/// concluded by the analysis).
impl<T: Default + 'static> BuildableAnalysisState for Lattice<T> {
    default fn create(anchor: LatticeAnchorRef) -> Self {
        Self {
            anchor,
            value: Default::default(),
        }
    }
}

impl<T: 'static> AnalysisState for Lattice<T> {
    #[inline(always)]
    fn as_any(&self) -> &dyn core::any::Any {
        self as &dyn core::any::Any
    }

    #[inline(always)]
    fn anchor(&self) -> &dyn LatticeAnchor {
        self.anchor.as_ref()
    }
}

impl<T: LatticeLike> SparseLattice for Lattice<T> {
    type Lattice = T;

    #[inline]
    fn lattice(&self) -> &Self::Lattice {
        &self.value
    }

    fn join(&mut self, rhs: &Self::Lattice) -> ChangeResult {
        let new_value = <T as LatticeLike>::join(&self.value, rhs);
        debug_assert_eq!(
            <T as LatticeLike>::join(&new_value, &self.value),
            new_value,
            "expected `join` to be monotonic"
        );
        debug_assert_eq!(
            <T as LatticeLike>::join(&new_value, rhs),
            new_value,
            "expected `join` to be monotonic"
        );

        // Update the current optimistic value if something changed
        if new_value == self.value {
            ChangeResult::Unchanged
        } else {
            self.value = new_value;
            ChangeResult::Changed
        }
    }

    fn meet(&mut self, rhs: &Self::Lattice) -> ChangeResult {
        let new_value = <T as LatticeLike>::meet(&self.value, rhs);
        debug_assert_eq!(
            <T as LatticeLike>::meet(&new_value, &self.value),
            new_value,
            "expected `meet` to be monotonic"
        );
        debug_assert_eq!(
            <T as LatticeLike>::meet(&new_value, rhs),
            new_value,
            "expected `meet` to be monotonic"
        );

        // Update the current optimistic value if something changed
        if new_value == self.value {
            ChangeResult::Unchanged
        } else {
            self.value = new_value;
            ChangeResult::Changed
        }
    }
}

impl<T: LatticeLike> DenseLattice for Lattice<T> {
    type Lattice = T;

    #[inline]
    fn lattice(&self) -> &Self::Lattice {
        &self.value
    }

    #[inline]
    fn lattice_mut(&mut self) -> &mut Self::Lattice {
        &mut self.value
    }

    fn join(&mut self, rhs: &Self::Lattice) -> ChangeResult {
        let new_value = <T as LatticeLike>::join(&self.value, rhs);
        debug_assert_eq!(
            <T as LatticeLike>::join(&new_value, &self.value),
            new_value,
            "expected `join` to be monotonic"
        );
        debug_assert_eq!(
            <T as LatticeLike>::join(&new_value, rhs),
            new_value,
            "expected `join` to be monotonic"
        );

        // Update the current optimistic value if something changed
        if new_value == self.value {
            ChangeResult::Unchanged
        } else {
            self.value = new_value;
            ChangeResult::Changed
        }
    }

    fn meet(&mut self, rhs: &Self::Lattice) -> ChangeResult {
        let new_value = <T as LatticeLike>::meet(&self.value, rhs);
        debug_assert_eq!(
            <T as LatticeLike>::meet(&new_value, &self.value),
            new_value,
            "expected `meet` to be monotonic"
        );
        debug_assert_eq!(
            <T as LatticeLike>::meet(&new_value, rhs),
            new_value,
            "expected `meet` to be monotonic"
        );

        // Update the current optimistic value if something changed
        if new_value == self.value {
            ChangeResult::Unchanged
        } else {
            self.value = new_value;
            ChangeResult::Changed
        }
    }
}
