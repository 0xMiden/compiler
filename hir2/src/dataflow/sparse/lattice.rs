use core::fmt;

use crate::dataflow::{
    AnalysisState, BuildableAnalysisState, ChangeResult, LatticeAnchor, LatticeAnchorRef,
};

/// A [SparseLattice] represents some analysis state attached to a specific value.
///
/// It is propagated through the IR by sparse data-flow analysis.
#[allow(unused_variables)]
pub trait SparseLattice: AnalysisState {
    type Lattice;

    /// Get the underlying lattice value
    fn lattice(&self) -> &Self::Lattice;

    /// Join `rhs` with `self`, returning whether or not a change was made
    fn join(&mut self, rhs: &Self::Lattice) -> ChangeResult {
        ChangeResult::Unchanged
    }

    /// Meet `rhs` with `self`, returning whether or not a change was made
    fn meet(&mut self, rhs: &Self::Lattice) -> ChangeResult {
        ChangeResult::Unchanged
    }
}

pub trait LatticeValue: Default + Eq + fmt::Debug + 'static {
    fn join(&self, other: &Self) -> Self;
    fn meet(&self, other: &Self) -> Self;
}

#[derive(Debug)]
pub struct Lattice<T> {
    anchor: LatticeAnchorRef,
    value: T,
}

impl<T> Lattice<T> {
    pub fn new(anchor: LatticeAnchorRef, value: T) -> Self {
        Self { anchor, value }
    }

    pub fn value(&self) -> &T {
        &self.value
    }
}

impl<T: Default + 'static> BuildableAnalysisState for Lattice<T> {
    fn create(anchor: LatticeAnchorRef) -> Self {
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
        &self.anchor as &dyn LatticeAnchor
    }
}

impl<T: LatticeValue> SparseLattice for Lattice<T> {
    type Lattice = T;

    #[inline]
    fn lattice(&self) -> &Self::Lattice {
        &self.value
    }

    fn join(&mut self, rhs: &Self::Lattice) -> ChangeResult {
        let new_value = <T as LatticeValue>::join(&self.value, rhs);
        debug_assert_eq!(
            <T as LatticeValue>::join(&new_value, &self.value),
            new_value,
            "expected `join` to be monotonic"
        );
        debug_assert_eq!(
            <T as LatticeValue>::join(&new_value, rhs),
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
        let new_value = <T as LatticeValue>::meet(&self.value, rhs);
        debug_assert_eq!(
            <T as LatticeValue>::meet(&new_value, &self.value),
            new_value,
            "expected `meet` to be monotonic"
        );
        debug_assert_eq!(
            <T as LatticeValue>::meet(&new_value, rhs),
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
