use alloc::rc::Rc;
use core::fmt;

use smallvec::SmallVec;

use crate::{
    dataflow::{
        sparse::{self, SparseDataFlowAnalysis},
        AnalysisStateGuard, BuildableDataFlowAnalysis, DataFlowSolver, Forward, Lattice,
        LatticeLike, SparseForwardDataFlowAnalysis, SparseLattice,
    },
    traits::Foldable,
    AttributeValue, Dialect, EntityRef, OpFoldResult, Operation, Report,
};

/// This lattice value represents a known constant value of a lattice.
#[derive(Default)]
pub struct ConstantValue {
    /// The constant value
    constant: Option<Box<dyn AttributeValue>>,
    /// The dialect that can be used to materialize this constant
    dialect: Option<Rc<dyn Dialect>>,
    /// A flag that indicates whether or not this value was explicitly initialized
    initialized: bool,
}
impl fmt::Debug for ConstantValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConstantValue")
            .field("value", &self.constant)
            .field_with("dialect", |f| {
                if let Some(dialect) = self.dialect.as_deref() {
                    write!(f, "Some({})", dialect.name())
                } else {
                    f.write_str("None")
                }
            })
            .field("initialized", &self.initialized)
            .finish()
    }
}

impl Clone for ConstantValue {
    fn clone(&self) -> Self {
        let constant = self.constant.as_deref().map(|c| c.clone_value());
        Self {
            constant,
            dialect: self.dialect.clone(),
            initialized: self.initialized,
        }
    }
}

#[allow(unused)]
impl ConstantValue {
    pub fn new(constant: Box<dyn AttributeValue>, dialect: Rc<dyn Dialect>) -> Self {
        Self {
            constant: Some(constant),
            dialect: Some(dialect),
            initialized: true,
        }
    }

    pub fn unknown() -> Self {
        Self {
            initialized: true,
            ..Default::default()
        }
    }

    #[inline]
    pub fn uninitialized() -> Self {
        Self::default()
    }

    #[inline]
    pub const fn is_uninitialized(&self) -> bool {
        !self.initialized
    }

    pub fn constant_value(&self) -> Option<Box<dyn AttributeValue>> {
        assert!(self.initialized, "expected constant value to be initialized");
        self.constant.as_deref().map(|c| c.clone_value())
    }

    pub fn constant_dialect(&self) -> Option<Rc<dyn Dialect>> {
        self.dialect.clone()
    }
}

impl Eq for ConstantValue {}
impl PartialEq for ConstantValue {
    fn eq(&self, other: &Self) -> bool {
        if !self.initialized && !other.initialized {
            return true;
        } else if self.initialized != other.initialized {
            return false;
        }

        self.constant == other.constant
    }
}

impl LatticeLike for ConstantValue {
    /// The join of two constant values is:
    ///
    /// * `unknown` if they represent different values
    /// * The identity function if they represent the same value
    /// * The more defined value if one of the two is uninitialized
    fn join(&self, rhs: &Self) -> Self {
        if self.is_uninitialized() {
            return rhs.clone();
        }

        if rhs.is_uninitialized() || self == rhs {
            return self.clone();
        }

        Self::unknown()
    }

    fn meet(&self, _other: &Self) -> Self {
        self.clone()
    }
}

/// This analysis implements sparse constant propagation, which attempts to determine constant-
/// valued results for operations using constant-valued operands, by speculatively folding
/// operations.
///
/// When combined with dead-code analysis, this becomes sparse conditional constant propagation,
/// commonly abbreviated as _SCCP_.
#[derive(Default)]
pub struct SparseConstantPropagation;

impl BuildableDataFlowAnalysis for SparseConstantPropagation {
    type Strategy = SparseDataFlowAnalysis<Self, Forward>;

    #[inline(always)]
    fn new(_solver: &mut DataFlowSolver) -> Self {
        Self
    }
}

impl SparseForwardDataFlowAnalysis for SparseConstantPropagation {
    type Lattice = Lattice<ConstantValue>;

    fn visit_operation(
        &self,
        op: &Operation,
        operands: &[EntityRef<'_, Self::Lattice>],
        results: &mut [AnalysisStateGuard<'_, Self::Lattice>],
        solver: &mut DataFlowSolver,
    ) -> Result<(), Report> {
        log::debug!("sparse-constant-propagation: visiting operation '{}'", op.name());

        // Don't try to simulate the results of a region operation as we can't guarantee that
        // folding will be out-of-place. We don't allow in-place folds as the desire here is for
        // simulated execution, and not general folding.
        if op.has_regions() {
            sparse::set_all_to_entry_states(self, results);
            return Ok(());
        }

        let mut constant_operands =
            SmallVec::<[Option<Box<dyn AttributeValue>>; 8]>::with_capacity(op.num_operands());
        for operand_lattice in operands.iter() {
            if operand_lattice.value().is_uninitialized() {
                return Ok(());
            }
            constant_operands.push(operand_lattice.value().constant_value());
        }

        // Save the original operands and attributes just in case the operation folds in-place.
        // The constant passed in may not correspond to the real runtime value, so in-place updates
        // are not allowed.
        //
        // Simulate the result of folding this operation to a constant. If folding fails or would be
        // an in-place fold, mark the results as overdefined.
        let mut fold_results = SmallVec::with_capacity(op.num_results());
        let fold_result = op.fold(&mut fold_results);
        if matches!(fold_result, crate::FoldResult::Failed | crate::FoldResult::InPlace) {
            sparse::set_all_to_entry_states(self, results);
            return Ok(());
        }

        // Merge the fold results into the lattice for this operation.
        assert_eq!(fold_results.len(), op.num_results());
        for (lattice, fold_result) in results.iter_mut().zip(fold_results.into_iter()) {
            // Merge in the result of the fold, either a constant or a value.
            match fold_result {
                OpFoldResult::Attribute(value) => {
                    log::trace!("folded to constant: {}", value.render());
                    lattice.join(&ConstantValue::new(value, op.dialect()));
                }
                OpFoldResult::Value(value) => {
                    log::trace!("folded to value: {value}");
                    lattice
                        .join(solver.get_or_create_mut::<Lattice<ConstantValue>, _>(value).value());
                }
            }
        }

        Ok(())
    }

    fn set_to_entry_state(&self, lattice: &mut AnalysisStateGuard<'_, Self::Lattice>) {
        lattice.join(&ConstantValue::unknown());
    }
}
