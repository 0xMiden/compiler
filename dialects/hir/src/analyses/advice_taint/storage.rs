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

use super::{
    lattice::{
        CallContextFrame, ContextualAdviceTaintValue, join_value_taint, required_value_taint,
    },
    layout::{ADVICE_PIPE_MEMORY_ADDRESS_OPERAND, ADVICE_PIPE_MEMORY_WRITE_WIDTH},
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
    locals: BTreeMap<LocalKey, ContextualAdviceTaintValue>,
    memory: MemoryState,
}

type LocalKey = (SymbolName, usize);

#[derive(Debug, Clone, Default, Eq, PartialEq)]
struct MemoryState {
    addresses: BTreeMap<u32, ContextualAdviceTaintValue>,
    dynamic: ContextualAdviceTaintValue,
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
        let taint = required_value_taint(store.value().as_value_ref(), dependent, solver);
        state.store_local(local_key(*store.get_local()), taint);
        return Ok(());
    }

    if let Some(load) = operation.downcast_ref::<LoadLocal>() {
        let taint = state.load_local(&local_key(*load.get_local()));
        join_value_taint(load.result().as_value_ref(), &taint, solver);
        return Ok(());
    }

    if let Some(store) = operation.downcast_ref::<Store>() {
        let taint = required_value_taint(store.value().as_value_ref(), dependent, solver);
        match memory_address(store.addr().as_value_ref()) {
            Some(address) => state.store_memory(address, taint),
            None => state.store_dynamic_memory(taint),
        }
        return Ok(());
    }

    if let Some(load) = operation.downcast_ref::<Load>() {
        let taint = match memory_address(load.addr().as_value_ref()) {
            Some(address) => state.load_memory(address),
            None => state.load_dynamic_memory(),
        };
        join_value_taint(load.result().as_value_ref(), &taint, solver);
        return Ok(());
    }

    if let Some(pipe) = operation.downcast_ref::<AdvicePipe>() {
        let taint = ContextualAdviceTaintValue::raw(operation.span());
        let address =
            pipe.stack().iter().nth(ADVICE_PIPE_MEMORY_ADDRESS_OPERAND).and_then(|addr| {
                let addr = addr.borrow().as_value_ref();
                memory_address(addr)
            });
        if let Some(address) = address {
            for offset in 0..ADVICE_PIPE_MEMORY_WRITE_WIDTH {
                state.store_memory(address + offset, taint.clone());
            }
        } else {
            state.store_dynamic_memory(taint);
        }
        return Ok(());
    }

    Ok(())
}

impl StorageState {
    fn store_local(&mut self, key: LocalKey, taint: ContextualAdviceTaintValue) {
        self.locals.insert(key, taint);
    }

    fn load_local(&self, key: &LocalKey) -> ContextualAdviceTaintValue {
        self.locals.get(key).cloned().unwrap_or_default()
    }

    fn store_dynamic_memory(&mut self, taint: ContextualAdviceTaintValue) {
        self.memory.store_dynamic(taint);
    }

    fn store_memory(&mut self, addr: u32, taint: ContextualAdviceTaintValue) {
        self.memory.store(addr, taint);
    }

    fn load_memory(&self, addr: u32) -> ContextualAdviceTaintValue {
        self.memory.load(addr)
    }

    fn load_dynamic_memory(&self) -> ContextualAdviceTaintValue {
        self.memory.load_dynamic()
    }

    fn memory_only(&self) -> Self {
        Self {
            locals: BTreeMap::new(),
            memory: self.memory.clone(),
        }
    }

    fn replace_memory_from(&mut self, other: &Self) {
        self.memory = other.memory.clone();
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
            locals: self.locals.iter().map(|(key, taint)| (*key, f(taint))).collect(),
            memory: self.memory.map_taint(f),
        }
    }

    fn join(&self, other: &Self) -> Self {
        let mut locals = self.locals.clone();
        for (key, taint) in other.locals.iter() {
            locals
                .entry(*key)
                .and_modify(|current| *current = LatticeLike::join(current, taint))
                .or_insert_with(|| taint.clone());
        }

        Self {
            locals,
            memory: self.memory.join(&other.memory),
        }
    }
}

impl MemoryState {
    fn store(&mut self, addr: u32, taint: ContextualAdviceTaintValue) {
        self.addresses.insert(addr, taint);
    }

    fn store_dynamic(&mut self, taint: ContextualAdviceTaintValue) {
        self.dynamic = LatticeLike::join(&self.dynamic, &taint);
    }

    fn load(&self, addr: u32) -> ContextualAdviceTaintValue {
        LatticeLike::join(&self.addresses.get(&addr).cloned().unwrap_or_default(), &self.dynamic)
    }

    fn load_dynamic(&self) -> ContextualAdviceTaintValue {
        self.addresses
            .values()
            .fold(self.dynamic.clone(), |acc, taint| LatticeLike::join(&acc, taint))
    }

    fn map_taint(
        &self,
        mut f: impl FnMut(&ContextualAdviceTaintValue) -> ContextualAdviceTaintValue,
    ) -> Self {
        Self {
            addresses: self.addresses.iter().map(|(addr, taint)| (*addr, f(taint))).collect(),
            dynamic: f(&self.dynamic),
        }
    }

    fn join(&self, other: &Self) -> Self {
        let mut addresses = self.addresses.clone();
        for (addr, taint) in other.addresses.iter() {
            addresses
                .entry(*addr)
                .and_modify(|current| *current = LatticeLike::join(current, taint))
                .or_insert_with(|| taint.clone());
        }

        Self {
            addresses,
            dynamic: LatticeLike::join(&self.dynamic, &other.dynamic),
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

fn local_key(local: LocalVariable) -> LocalKey {
    let function = local.function();
    let function = function.borrow();
    (Symbol::name(&*function), local.as_usize())
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
