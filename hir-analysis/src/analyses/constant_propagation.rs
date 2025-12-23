use alloc::{boxed::Box, rc::Rc};
use core::fmt;

use midenc_hir::{
    AttributeValue, Dialect, FoldResult, Forward, OpFoldResult, Operation, Report, SmallVec,
    traits::Foldable,
};

use crate::{
    AnalysisState, AnalysisStateGuard, AnalysisStateGuardMut, BuildableDataFlowAnalysis,
    DataFlowSolver, Lattice, LatticeLike, SparseForwardDataFlowAnalysis, SparseLattice,
    sparse::{self, SparseDataFlowAnalysis},
};

/// This lattice value represents a known constant value of a lattice.
#[derive(Default)]
pub struct ConstantValue {
    /// The constant value
    constant: Option<Option<Box<dyn AttributeValue>>>,
    /// The dialect that can be used to materialize this constant
    dialect: Option<Rc<dyn Dialect>>,
}
impl fmt::Display for ConstantValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.constant.as_ref() {
            None => f.write_str("uninitialized"),
            Some(None) => f.write_str("unknown"),
            Some(Some(value)) => fmt::Debug::fmt(value, f),
        }
    }
}
impl fmt::Debug for ConstantValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

impl Clone for ConstantValue {
    fn clone(&self) -> Self {
        let constant = self.constant.as_ref().map(|c| c.as_deref().map(|c| c.clone_value()));
        Self {
            constant,
            dialect: self.dialect.clone(),
        }
    }
}

#[allow(unused)]
impl ConstantValue {
    pub fn new(constant: Box<dyn AttributeValue>, dialect: Rc<dyn Dialect>) -> Self {
        Self {
            constant: Some(Some(constant)),
            dialect: Some(dialect),
        }
    }

    pub fn unknown() -> Self {
        Self {
            constant: Some(None),
            ..Default::default()
        }
    }

    #[inline]
    pub fn uninitialized() -> Self {
        Self::default()
    }

    #[inline]
    pub const fn is_uninitialized(&self) -> bool {
        self.constant.is_none()
    }

    pub fn constant_value(&self) -> Option<Box<dyn AttributeValue>> {
        self.constant
            .as_ref()
            .expect("expected constant value to be initialized")
            .as_deref()
            .map(|c| c.clone_value())
    }

    pub fn constant_dialect(&self) -> Option<Rc<dyn Dialect>> {
        self.dialect.clone()
    }
}

impl Eq for ConstantValue {}
impl PartialEq for ConstantValue {
    fn eq(&self, other: &Self) -> bool {
        self.constant == other.constant
    }
}

impl LatticeLike for ConstantValue {
    fn join(&self, rhs: &Self) -> Self {
        // The join of two constant values is:
        //
        // * `unknown` if they represent different values
        // * The identity function if they represent the same value
        // * The more defined value if one of the two is uninitialized
        match (self.is_uninitialized(), rhs.is_uninitialized()) {
            (false, false) => {
                if self == rhs {
                    self.clone()
                } else {
                    Self::unknown()
                }
            }
            (true, true) | (false, true) => self.clone(),
            (true, false) => rhs.clone(),
        }
    }

    fn meet(&self, rhs: &Self) -> Self {
        if self.is_uninitialized() || rhs.is_uninitialized() {
            Self::uninitialized()
        } else if self == rhs {
            self.clone()
        } else {
            Self::unknown()
        }
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

    fn debug_name(&self) -> &'static str {
        "sparse-constant-propagation"
    }

    fn visit_operation(
        &self,
        op: &Operation,
        operands: &[AnalysisStateGuard<'_, Self::Lattice>],
        results: &mut [AnalysisStateGuardMut<'_, Self::Lattice>],
        solver: &mut DataFlowSolver,
    ) -> Result<(), Report> {
        log::debug!("visiting operation {op}");

        // Don't try to simulate the results of a region operation as we can't guarantee that
        // folding will be out-of-place. We don't allow in-place folds as the desire here is for
        // simulated execution, and not general folding.
        if op.has_regions() {
            log::trace!("op has regions so conservatively setting results to entry state");
            sparse::set_all_to_entry_states(self, results);
            return Ok(());
        }

        let mut constant_operands =
            SmallVec::<[Option<Box<dyn AttributeValue>>; 8]>::with_capacity(op.num_operands());
        for (index, operand_lattice) in operands.iter().enumerate() {
            log::trace!(
                "operand lattice for {} is {}",
                op.operands()[index].borrow().as_value_ref(),
                operand_lattice.value()
            );
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
        if matches!(fold_result, FoldResult::Failed | FoldResult::InPlace) {
            sparse::set_all_to_entry_states(self, results);
            return Ok(());
        }

        // Merge the fold results into the lattice for this operation.
        assert_eq!(fold_results.len(), op.num_results());
        for (lattice, fold_result) in results.iter_mut().zip(fold_results.into_iter()) {
            // Merge in the result of the fold, either a constant or a value.
            match fold_result {
                OpFoldResult::Attribute(value) => {
                    let new_lattice = ConstantValue::new(value, op.dialect());
                    log::trace!(
                        "setting lattice for {} to {new_lattice} from {}",
                        lattice.anchor(),
                        lattice.value()
                    );
                    let change_result = lattice.join(&new_lattice);
                    log::debug!(
                        "setting constant value for {} to {new_lattice}: {change_result} as {}",
                        lattice.anchor(),
                        lattice.value()
                    );
                }
                OpFoldResult::Value(value) => {
                    let new_lattice = solver.get_or_create_mut::<Lattice<ConstantValue>, _>(value);
                    log::trace!(
                        "setting lattice for {} to {} from {}",
                        lattice.anchor(),
                        new_lattice.value(),
                        lattice.value()
                    );
                    let change_result = lattice.join(new_lattice.value());
                    log::debug!(
                        "setting constant value for {} to {}: {change_result} as {}",
                        lattice.anchor(),
                        new_lattice.value(),
                        lattice.value()
                    );
                }
            }
        }

        Ok(())
    }

    fn set_to_entry_state(&self, lattice: &mut AnalysisStateGuardMut<'_, Self::Lattice>) {
        log::trace!("setting lattice to entry state from {}", lattice.value());
        let entry_state = ConstantValue::unknown();
        let change_result = lattice.join(&entry_state);
        log::debug!(
            "setting constant value for {} to {entry_state}: {change_result} as {}",
            lattice.anchor(),
            lattice.value()
        );
    }
}
