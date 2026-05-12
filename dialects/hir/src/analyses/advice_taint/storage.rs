use alloc::collections::BTreeMap;

use midenc_dialect_arith as arith;
use midenc_hir::{
    CallOpInterface, Forward, Operation, ProgramPoint, Report, Spanned, Symbol, SymbolName,
    ValueRef, dialects::builtin::attributes::LocalVariable,
};
use midenc_hir_analysis::{
    AnalysisState, AnalysisStateGuardMut, BuildableDataFlowAnalysis, CallControlFlowAction,
    DataFlowSolver, DenseForwardDataFlowAnalysis, Lattice, LatticeLike, analyses::DeadCodeAnalysis,
    dense::DenseDataFlowAnalysis,
};

use super::lattice::{
    CallContextFrame, ContextualAdviceTaintValue, join_value_taint, required_value_taint,
};
use crate::{AdvicePipe, IntToPtr, Load, LoadLocal, Store, StoreLocal};

/// Dense propagation of storage taint through local slots and memory.
///
/// Storage itself is modeled as dense program-point state, while load results are written back into
/// the sparse advice-taint value lattice. That lets the sparse solver propagate storage-derived
/// values through ordinary SSA use-def edges.
#[derive(Default)]
pub(super) struct AdviceTaintStoragePropagation;

impl BuildableDataFlowAnalysis for AdviceTaintStoragePropagation {
    type Strategy = DenseDataFlowAnalysis<Self, Forward>;

    fn new(solver: &mut DataFlowSolver) -> Self {
        solver.load::<DeadCodeAnalysis>();
        solver.load::<super::AdviceTaintPropagation>();
        Self
    }
}

impl DenseForwardDataFlowAnalysis for AdviceTaintStoragePropagation {
    type Lattice = Lattice<StorageState>;

    fn debug_name(&self) -> &'static str {
        "unconstrained-advice-storage-taint"
    }

    fn allow_unknown_predecessors(&self) -> bool {
        true
    }

    fn visit_operation(
        &self,
        op: &Operation,
        _before: &Self::Lattice,
        after: &mut AnalysisStateGuardMut<'_, Self::Lattice>,
        solver: &mut DataFlowSolver,
    ) -> Result<(), Report> {
        let dependent = ProgramPoint::after(op.as_operation_ref());
        let mut state = storage_state_before_operation(op, dependent, solver);
        transfer_storage_operation(op, &mut state, solver)?;
        midenc_hir_analysis::DenseLattice::join(after, &state);
        Ok(())
    }

    fn set_to_entry_state(
        &self,
        lattice: &mut AnalysisStateGuardMut<'_, Self::Lattice>,
        _solver: &mut DataFlowSolver,
    ) {
        midenc_hir_analysis::DenseLattice::join(lattice, &StorageState::default());
    }

    fn visit_call_control_flow_transfer(
        &self,
        call: &dyn CallOpInterface,
        action: CallControlFlowAction,
        before: &Self::Lattice,
        after: &mut AnalysisStateGuardMut<'_, Self::Lattice>,
        solver: &mut DataFlowSolver,
    ) {
        let dependent = AnalysisState::anchor(after)
            .as_program_point()
            .unwrap_or_else(|| ProgramPoint::after(call.as_operation()));
        let frame = CallContextFrame::new(call);
        match action {
            CallControlFlowAction::Enter => {
                let state = storage_state_before_operation(call.as_operation(), dependent, solver);
                midenc_hir_analysis::DenseLattice::join(
                    after,
                    &state.memory_only().enter_call(frame),
                );
            }
            CallControlFlowAction::Exit => {
                let mut state =
                    storage_state_before_operation(call.as_operation(), dependent, solver);
                state.replace_memory_from(&before.value().exit_call(frame));
                midenc_hir_analysis::DenseLattice::join(after, &state);
            }
            // External memory effects need summaries before we can model them conservatively.
            CallControlFlowAction::External => {
                let state = storage_state_before_operation(call.as_operation(), dependent, solver);
                midenc_hir_analysis::DenseLattice::join(after, &state);
            }
        }
    }
}

#[derive(Debug, Clone, Default, Eq, PartialEq)]
pub(super) struct StorageState {
    storage: BTreeMap<StorageKey, ContextualAdviceTaintValue>,
    dynamic_memory: ContextualAdviceTaintValue,
}

#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
enum StorageKey {
    Local(SymbolName, usize),
    Memory(u32),
}

/// Returns the storage state immediately before `operation`.
///
/// Dense forward analysis passes `ProgramPoint::before(operation)` as the `before` lattice for an
/// operation transfer, but the solver stores straight-line dense state at concrete CFG anchors:
/// either after the previous operation or at the start of the block. Storage transfer needs that
/// concrete state because it both reads storage and writes load-result taint back into sparse value
/// state during the same solver iteration.
fn storage_state_before_operation(
    operation: &Operation,
    dependent: ProgramPoint,
    solver: &mut DataFlowSolver,
) -> StorageState {
    let operation = operation.as_operation_ref();
    if let Some(previous) = operation.prev() {
        return solver
            .require::<Lattice<StorageState>, _>(ProgramPoint::after(previous), dependent)
            .value()
            .clone();
    }

    if let Some(block) = operation.parent() {
        return solver
            .require::<Lattice<StorageState>, _>(ProgramPoint::at_start_of(block), dependent)
            .value()
            .clone();
    }

    StorageState::default()
}

fn transfer_storage_operation(
    operation: &Operation,
    state: &mut StorageState,
    solver: &mut DataFlowSolver,
) -> Result<(), Report> {
    let dependent = ProgramPoint::after(operation.as_operation_ref());
    if let Some(store) = operation.downcast_ref::<StoreLocal>() {
        let key = StorageKey::from(*store.get_local());
        let taint = required_value_taint(store.value().as_value_ref(), dependent, solver);
        state.store(key, taint);
        return Ok(());
    }

    if let Some(load) = operation.downcast_ref::<LoadLocal>() {
        let key = StorageKey::from(*load.get_local());
        let taint = state.load(&key);
        join_value_taint(load.result().as_value_ref(), &taint, solver);
        return Ok(());
    }

    if let Some(store) = operation.downcast_ref::<Store>() {
        let taint = required_value_taint(store.value().as_value_ref(), dependent, solver);
        match memory_storage_key(store.addr().as_value_ref()) {
            Some(key) => state.store(key, taint),
            None => state.store_dynamic_memory(taint),
        }
        return Ok(());
    }

    if let Some(load) = operation.downcast_ref::<Load>() {
        let taint = match memory_storage_key(load.addr().as_value_ref()) {
            Some(StorageKey::Memory(addr)) => state.load_memory(addr),
            Some(key) => state.load(&key),
            None => state.load_dynamic_memory(),
        };
        join_value_taint(load.result().as_value_ref(), &taint, solver);
        return Ok(());
    }

    if let Some(pipe) = operation.downcast_ref::<AdvicePipe>() {
        let taint = ContextualAdviceTaintValue::raw(operation.span());
        let address = pipe.stack().iter().nth(12).and_then(|addr| {
            let addr = addr.borrow().as_value_ref();
            memory_address(addr)
        });
        if let Some(address) = address {
            for offset in 0..8 {
                state.store(StorageKey::Memory(address + offset), taint.clone());
            }
        } else {
            state.store_dynamic_memory(taint);
        }
        return Ok(());
    }

    Ok(())
}

impl StorageState {
    fn store(&mut self, key: StorageKey, taint: ContextualAdviceTaintValue) {
        self.storage.insert(key, taint);
    }

    fn load(&self, key: &StorageKey) -> ContextualAdviceTaintValue {
        self.storage.get(key).cloned().unwrap_or_default()
    }

    fn store_dynamic_memory(&mut self, taint: ContextualAdviceTaintValue) {
        self.dynamic_memory = LatticeLike::join(&self.dynamic_memory, &taint);
    }

    fn load_memory(&self, addr: u32) -> ContextualAdviceTaintValue {
        LatticeLike::join(&self.load(&StorageKey::Memory(addr)), &self.dynamic_memory)
    }

    fn load_dynamic_memory(&self) -> ContextualAdviceTaintValue {
        self.storage
            .iter()
            .filter_map(|(key, taint)| {
                if matches!(key, StorageKey::Memory(_)) {
                    Some(taint)
                } else {
                    None
                }
            })
            .fold(self.dynamic_memory.clone(), |acc, taint| LatticeLike::join(&acc, taint))
    }

    fn memory_only(&self) -> Self {
        Self {
            storage: self
                .storage
                .iter()
                .filter_map(|(key, taint)| {
                    if matches!(key, StorageKey::Memory(_)) {
                        Some((key.clone(), taint.clone()))
                    } else {
                        None
                    }
                })
                .collect(),
            dynamic_memory: self.dynamic_memory.clone(),
        }
    }

    fn replace_memory_from(&mut self, other: &Self) {
        self.storage.retain(|key, _| !matches!(key, StorageKey::Memory(_)));
        self.storage.extend(other.storage.iter().filter_map(|(key, taint)| {
            if matches!(key, StorageKey::Memory(_)) {
                Some((key.clone(), taint.clone()))
            } else {
                None
            }
        }));
        self.dynamic_memory = other.dynamic_memory.clone();
    }

    fn enter_call(&self, frame: CallContextFrame) -> Self {
        self.map_taint(|taint| taint.enter_call(frame))
    }

    fn exit_call(&self, frame: CallContextFrame) -> Self {
        self.map_taint(|taint| taint.exit_call(frame))
    }

    fn map_taint(
        &self,
        mut f: impl FnMut(&ContextualAdviceTaintValue) -> ContextualAdviceTaintValue,
    ) -> Self {
        Self {
            storage: self.storage.iter().map(|(key, taint)| (key.clone(), f(taint))).collect(),
            dynamic_memory: f(&self.dynamic_memory),
        }
    }

    fn join(&self, other: &Self) -> Self {
        let mut storage = self.storage.clone();
        for (key, taint) in other.storage.iter() {
            storage
                .entry(key.clone())
                .and_modify(|current| *current = LatticeLike::join(current, taint))
                .or_insert_with(|| taint.clone());
        }

        Self {
            storage,
            dynamic_memory: LatticeLike::join(&self.dynamic_memory, &other.dynamic_memory),
        }
    }
}

impl LatticeLike for StorageState {
    fn join(&self, other: &Self) -> Self {
        StorageState::join(self, other)
    }

    fn meet(&self, other: &Self) -> Self {
        self.join(other)
    }
}

impl From<LocalVariable> for StorageKey {
    fn from(local: LocalVariable) -> Self {
        let function = local.function();
        let function = function.borrow();
        Self::Local(Symbol::name(&*function), local.as_usize())
    }
}

fn memory_storage_key(ptr: ValueRef) -> Option<StorageKey> {
    memory_address(ptr).map(StorageKey::Memory)
}

fn memory_address(value: ValueRef) -> Option<u32> {
    let defining_op = value.borrow().get_defining_op()?;
    let defining_op = defining_op.borrow();

    if let Some(inttoptr) = defining_op.downcast_ref::<IntToPtr>() {
        return memory_address(inttoptr.operand().as_value_ref());
    }

    if let Some(constant) = defining_op.downcast_ref::<arith::Constant>() {
        return constant.get_value().as_u32();
    }

    if let Some(add) = defining_op.downcast_ref::<arith::Add>() {
        let lhs = memory_address(add.lhs().as_value_ref())?;
        let rhs = memory_address(add.rhs().as_value_ref())?;
        return lhs.checked_add(rhs);
    }

    None
}
