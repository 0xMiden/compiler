use core::any::Any;

use super::*;
use crate::{
    dataflow::{
        AnalysisState, AnalysisStateGuard, BuildableAnalysisState, BuildableDataFlowAnalysis,
        CallControlFlowAction, ChangeResult, LatticeAnchor, LatticeAnchorRef,
    },
    CallOpInterface, Operation, OperationRef, RegionBranchOpInterface, RegionRef, Report,
};

#[derive(Default)]
pub struct ExampleDenseForwardDataFlowAnalysisImpl {
    #[allow(dead_code)]
    assume_func_writes: bool,
}

pub struct LastModification {
    anchor: LatticeAnchorRef,
    ops: alloc::collections::BTreeSet<OperationRef>,
}
impl LastModification {
    pub fn reset(&mut self) -> ChangeResult {
        if self.ops.is_empty() {
            ChangeResult::Unchanged
        } else {
            self.ops.clear();
            ChangeResult::Changed
        }
    }
}
impl BuildableAnalysisState for LastModification {
    fn create(anchor: LatticeAnchorRef) -> Self {
        Self {
            anchor,
            ops: Default::default(),
        }
    }
}
impl AnalysisState for LastModification {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn anchor(&self) -> &dyn LatticeAnchor {
        &self.anchor
    }
}
impl DenseLattice for LastModification {
    type Lattice = Self;

    #[inline(always)]
    fn lattice(&self) -> &Self::Lattice {
        self
    }

    fn join(&mut self, rhs: &Self) -> ChangeResult {
        let len = self.ops.len();
        self.ops.extend(rhs.ops.iter().cloned());
        if self.ops.len() != len {
            ChangeResult::Changed
        } else {
            ChangeResult::Unchanged
        }
    }
}

impl BuildableDataFlowAnalysis for ExampleDenseForwardDataFlowAnalysisImpl {
    type Strategy = DenseDataFlowAnalysis<Self, Forward>;

    fn new(_solver: &mut DataFlowSolver) -> Self {
        Self {
            assume_func_writes: false,
        }
    }
}

#[allow(unused_variables)]
impl DenseForwardDataFlowAnalysis for ExampleDenseForwardDataFlowAnalysisImpl {
    type Lattice = LastModification;

    /// Visit an operation. If the operation has no memory effects, then the state
    /// is propagated with no change. If the operation allocates a resource, then
    /// its reaching definitions is set to empty. If the operation writes to a
    /// resource, then its reaching definition is set to the written value.
    fn visit_operation(
        &self,
        op: &Operation,
        before: &Self::Lattice,
        after: &mut AnalysisStateGuard<'_, Self::Lattice>,
    ) -> Result<(), Report> {
        /*
        auto memory = dyn_cast<MemoryEffectOpInterface>(op);
        // If we can't reason about the memory effects, then conservatively assume we
        // can't deduce anything about the last modifications.
        if (!memory) {
          setToEntryState(after);
          return success();
        }

        SmallVector<MemoryEffects::EffectInstance> effects;
        memory.getEffects(effects);

        // First, check if all underlying values are already known. Otherwise, avoid
        // propagating and stay in the "undefined" state to avoid incorrectly
        // propagating values that may be overwritten later on as that could be
        // problematic for convergence based on monotonicity of lattice updates.
        SmallVector<Value> underlyingValues;
        underlyingValues.reserve(effects.size());
        for (const auto &effect : effects) {
          Value value = effect.getValue();

          // If we see an effect on anything other than a value, assume we can't
          // deduce anything about the last modifications.
          if (!value) {
            setToEntryState(after);
            return success();
          }

          // If we cannot find the underlying value, we shouldn't just propagate the
          // effects through, return the pessimistic state.
          std::optional<Value> underlyingValue =
              UnderlyingValueAnalysis::getMostUnderlyingValue(
                  value, [&](Value value) {
                    return getOrCreateFor<UnderlyingValueLattice>(
                        getProgramPointAfter(op), value);
                  });

          // If the underlying value is not yet known, don't propagate yet.
          if (!underlyingValue)
            return success();

          underlyingValues.push_back(*underlyingValue);
        }

        // Update the state when all underlying values are known.
        ChangeResult result = after->join(before);
        for (const auto &[effect, value] : llvm::zip(effects, underlyingValues)) {
          // If the underlying value is known to be unknown, set to fixpoint state.
          if (!value) {
            setToEntryState(after);
            return success();
          }

          // Nothing to do for reads.
          if (isa<MemoryEffects::Read>(effect.getEffect()))
            continue;

          result |= after->set(value, op);
        }
        propagateIfChanged(after, result);
        return success();
         */
        todo!()
    }

    fn visit_call_control_flow_transfer(
        &self,
        call: &dyn CallOpInterface,
        action: CallControlFlowAction,
        before: &Self::Lattice,
        after: &mut AnalysisStateGuard<'_, Self::Lattice>,
    ) {
        /*
        if (action == CallControlFlowAction::ExternalCallee && assumeFuncWrites) {
          SmallVector<Value> underlyingValues;
          underlyingValues.reserve(call->getNumOperands());
          for (Value operand : call.getArgOperands()) {
            std::optional<Value> underlyingValue =
                UnderlyingValueAnalysis::getMostUnderlyingValue(
                    operand, [&](Value value) {
                      return getOrCreateFor<UnderlyingValueLattice>(
                          getProgramPointAfter(call.getOperation()), value);
                    });
            if (!underlyingValue)
              return;
            underlyingValues.push_back(*underlyingValue);
          }

          ChangeResult result = after->join(before);
          for (Value operand : underlyingValues)
            result |= after->set(operand, call);
          return propagateIfChanged(after, result);
        }
        auto testCallAndStore =
            dyn_cast<::test::TestCallAndStoreOp>(call.getOperation());
        if (testCallAndStore && ((action == CallControlFlowAction::EnterCallee &&
                                  testCallAndStore.getStoreBeforeCall()) ||
                                 (action == CallControlFlowAction::ExitCallee &&
                                  !testCallAndStore.getStoreBeforeCall()))) {
          (void)visitOperation(call, before, after);
          return;
        }
        AbstractDenseForwardDataFlowAnalysis::visitCallControlFlowTransfer(
            call, action, before, after);
             */
        todo!()
    }

    fn visit_region_branch_control_flow_transfer(
        &self,
        branch: &dyn RegionBranchOpInterface,
        region_from: Option<RegionRef>,
        region_to: Option<RegionRef>,
        before: &Self::Lattice,
        after: &mut AnalysisStateGuard<'_, Self::Lattice>,
    ) {
        /*
        auto defaultHandling = [&]() {
          AbstractDenseForwardDataFlowAnalysis::visitRegionBranchControlFlowTransfer(
              branch, regionFrom, regionTo, before, after);
        };
        TypeSwitch<Operation *>(branch.getOperation())
            .Case<::test::TestStoreWithARegion, ::test::TestStoreWithALoopRegion>(
                [=](auto storeWithRegion) {
                  if ((!regionTo && !storeWithRegion.getStoreBeforeRegion()) ||
                      (!regionFrom && storeWithRegion.getStoreBeforeRegion()))
                    (void)visitOperation(branch, before, after);
                  defaultHandling();
                })
            .Default([=](auto) { defaultHandling(); });
             */
        todo!()
    }

    fn set_to_entry_state(&self, lattice: &mut AnalysisStateGuard<'_, Self::Lattice>) {
        lattice.reset();
    }
}
