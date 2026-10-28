use core::any::Any;

use midenc_hir::IntegerValueRange;

use crate::{
    AnalysisState, BuildableAnalysisState, ChangeResult, Lattice, LatticeAnchor, LatticeAnchorRef,
    LatticeLike, SparseLattice,
};

/// Integer range state with bounded widening for loop-carried values.
///
/// Widening belongs to the analysis state: the underlying value's lattice joins
/// remain associative, and acyclic joins retain their precision until the budget
/// of strict range expansions is reached.
#[derive(Debug)]
pub struct IntegerRangeLattice {
    inner: Lattice<IntegerValueRange>,
    expansions: u8,
}

impl IntegerRangeLattice {
    pub fn value(&self) -> &IntegerValueRange {
        self.inner.value()
    }
}

impl BuildableAnalysisState for IntegerRangeLattice {
    fn create(anchor: LatticeAnchorRef) -> Self {
        Self {
            inner: Lattice::new(anchor, IntegerValueRange::uninitialized()),
            expansions: 0,
        }
    }
}

impl AnalysisState for IntegerRangeLattice {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn anchor(&self) -> &dyn LatticeAnchor {
        self.inner.anchor()
    }
}

impl SparseLattice for IntegerRangeLattice {
    type Lattice = IntegerValueRange;

    fn lattice(&self) -> &Self::Lattice {
        self.value()
    }

    fn join(&mut self, rhs: &Self::Lattice) -> ChangeResult {
        const MAX_PRECISE_EXPANSIONS: u8 = 8;

        let mut joined = self.value().join(rhs);
        if let (Some(previous), Some(next)) = (self.value().range(), joined.range())
            && previous != next
        {
            if self.expansions < MAX_PRECISE_EXPANSIONS {
                self.expansions += 1;
            } else {
                joined = IntegerValueRange::with_confidence(
                    previous.widen(next),
                    joined.confidence().expect("initialized range has confidence"),
                );
            }
        }
        self.inner.join(&joined)
    }

    fn meet(&mut self, rhs: &Self::Lattice) -> ChangeResult {
        self.inner.meet(rhs)
    }
}
