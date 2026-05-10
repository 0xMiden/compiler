use alloc::{collections::BTreeMap, rc::Rc, vec::Vec};
use core::{any::Any, fmt};

use midenc_dialect_arith as arith;
use midenc_hir::{
    Forward, Operation, OperationName, Report, SourceSpan, Spanned, Symbol, SymbolName, Type,
    dialects::builtin,
    pass::{Analysis, AnalysisManager, PreservedAnalyses},
};
use midenc_hir_analysis::{
    AnalysisStateGuard, AnalysisStateGuardMut, BuildableDataFlowAnalysis, DataFlowConfig,
    DataFlowSolver, Lattice, LatticeLike, SparseForwardDataFlowAnalysis, SparseLattice,
    analyses::{DeadCodeAnalysis, SparseConstantPropagation},
    sparse::SparseDataFlowAnalysis,
};

use crate::{AdviceLoadWord, AdvicePipe, AdvicePop, AssertU32};

/// The first unsafe u32-presuming use of raw advice data.
#[derive(Debug, Clone)]
pub struct AdviceTaintFinding {
    /// The operation that consumed raw advice as a u32.
    pub sink: OperationName,
    /// The span of the unsafe sink operation.
    pub sink_span: SourceSpan,
    /// The advice-producing operation span from which the raw value originated.
    pub advice_span: SourceSpan,
    /// The nearest containing function, when available.
    pub function: Option<SymbolName>,
}

/// Sparse taint facts for an SSA value.
///
/// Each tracked origin is either still unreported on at least one path, or has already reached an
/// unsafe sink on all paths represented by the value. Keeping reported origins in the lattice lets
/// downstream u32 operations avoid duplicate diagnostics along the same path.
#[derive(Clone, Eq, PartialEq)]
pub struct AdviceTaintValue {
    origins: BTreeMap<SourceSpan, OriginState>,
}

impl AdviceTaintValue {
    pub fn clean() -> Self {
        Self {
            origins: BTreeMap::new(),
        }
    }

    pub fn raw(span: SourceSpan) -> Self {
        Self {
            origins: BTreeMap::from([(span, OriginState::Unreported)]),
        }
    }

    pub fn is_clean(&self) -> bool {
        self.origins.is_empty()
    }

    pub fn has_unreported_origin(&self) -> bool {
        self.origins.values().any(|state| state.is_unreported())
    }

    pub fn unreported_origins(&self) -> impl Iterator<Item = SourceSpan> + '_ {
        self.origins.iter().filter_map(|(span, state)| {
            if state.is_unreported() {
                Some(*span)
            } else {
                None
            }
        })
    }

    pub fn mark_reported(&self) -> Self {
        Self {
            origins: self
                .origins
                .keys()
                .copied()
                .map(|span| (span, OriginState::Reported))
                .collect(),
        }
    }

    fn join_all<'a>(values: impl IntoIterator<Item = &'a Self>) -> Self {
        values
            .into_iter()
            .fold(Self::clean(), |acc, value| LatticeLike::join(&acc, value))
    }
}

impl Default for AdviceTaintValue {
    fn default() -> Self {
        Self::clean()
    }
}

impl fmt::Debug for AdviceTaintValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_map().entries(self.origins.iter()).finish()
    }
}

impl LatticeLike for AdviceTaintValue {
    fn join(&self, other: &Self) -> Self {
        let mut joined = self.origins.clone();
        for (&span, &state) in other.origins.iter() {
            joined
                .entry(span)
                .and_modify(|current| *current = current.join(state))
                .or_insert(state);
        }
        Self { origins: joined }
    }

    fn meet(&self, other: &Self) -> Self {
        self.join(other)
    }
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
enum OriginState {
    Unreported,
    Reported,
}

impl OriginState {
    fn is_unreported(self) -> bool {
        matches!(self, Self::Unreported)
    }

    fn join(self, other: Self) -> Self {
        if self.is_unreported() || other.is_unreported() {
            Self::Unreported
        } else {
            Self::Reported
        }
    }
}

/// Sparse propagation of unconstrained advice taint through SSA values.
#[derive(Default)]
pub struct AdviceTaintPropagation;

impl BuildableDataFlowAnalysis for AdviceTaintPropagation {
    type Strategy = SparseDataFlowAnalysis<Self, Forward>;

    fn new(solver: &mut DataFlowSolver) -> Self {
        solver.load::<DeadCodeAnalysis>();
        solver.load::<SparseConstantPropagation>();
        Self
    }
}

impl SparseForwardDataFlowAnalysis for AdviceTaintPropagation {
    type Lattice = Lattice<AdviceTaintValue>;

    fn debug_name(&self) -> &'static str {
        "unconstrained-advice-taint"
    }

    fn visit_operation(
        &self,
        op: &Operation,
        operands: &[AnalysisStateGuard<'_, Self::Lattice>],
        results: &mut [AnalysisStateGuardMut<'_, Self::Lattice>],
        _solver: &mut DataFlowSolver,
    ) -> Result<(), Report> {
        if op.is::<AdvicePop>() || op.is::<AdviceLoadWord>() || op.is::<AdvicePipe>() {
            return join_results(results, &AdviceTaintValue::raw(op.span()));
        }

        if op.is::<AssertU32>() {
            return join_results(results, &AdviceTaintValue::clean());
        }

        let operand_taint =
            AdviceTaintValue::join_all(operands.iter().map(|operand| operand.value()));
        let result_taint = if is_u32_presuming_arith_op(op) && operand_taint.has_unreported_origin()
        {
            operand_taint.mark_reported()
        } else {
            operand_taint
        };
        join_results(results, &result_taint)
    }

    fn set_to_entry_state(&self, lattice: &mut AnalysisStateGuardMut<'_, Self::Lattice>) {
        lattice.join(&AdviceTaintValue::clean());
    }
}

fn join_results(
    results: &mut [AnalysisStateGuardMut<'_, Lattice<AdviceTaintValue>>],
    value: &AdviceTaintValue,
) -> Result<(), Report> {
    for result in results {
        result.join(value);
    }
    Ok(())
}

/// Analysis wrapper that runs the sparse taint propagation and materializes diagnostics.
#[derive(Default)]
pub struct AdviceTaintAnalysis {
    solver: DataFlowSolver,
    findings: Vec<AdviceTaintFinding>,
}

impl AdviceTaintAnalysis {
    pub fn findings(&self) -> &[AdviceTaintFinding] {
        &self.findings
    }

    pub fn solver(&self) -> &DataFlowSolver {
        &self.solver
    }
}

impl Analysis for AdviceTaintAnalysis {
    type Target = Operation;

    fn name(&self) -> &'static str {
        "unconstrained-advice-taint"
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_rc(self: Rc<Self>) -> Rc<dyn Any> {
        self
    }

    fn analyze(
        &mut self,
        op: &Self::Target,
        analysis_manager: AnalysisManager,
    ) -> Result<(), Report> {
        let mut config = DataFlowConfig::new();
        config.set_interprocedural(true);
        self.solver = DataFlowSolver::new(config);
        self.solver.load::<AdviceTaintPropagation>();
        self.solver.initialize_and_run(op, analysis_manager)?;
        self.findings = collect_findings(op, &self.solver);
        Ok(())
    }

    fn invalidate(&self, _preserved_analyses: &mut PreservedAnalyses) -> bool {
        true
    }
}

fn collect_findings(op: &Operation, solver: &DataFlowSolver) -> Vec<AdviceTaintFinding> {
    let mut findings = Vec::new();
    op.prewalk_all(|operation| {
        if !is_u32_presuming_arith_op(operation) {
            return;
        }

        let mut operand_taint = AdviceTaintValue::clean();
        for operand in operation.operands().iter() {
            let value = operand.borrow().as_value_ref();
            if let Some(state) = solver.get::<Lattice<AdviceTaintValue>, _>(&value) {
                operand_taint = LatticeLike::join(&operand_taint, state.value());
            }
        }
        if operand_taint.is_clean() || !operand_taint.has_unreported_origin() {
            return;
        }

        let function = operation.nearest_parent_op::<builtin::Function>().map(|function| {
            let function = function.borrow();
            Symbol::name(&*function)
        });
        let sink = operation.name();
        let sink_span = operation.span();
        findings.extend(operand_taint.unreported_origins().map(|advice_span| AdviceTaintFinding {
            sink: sink.clone(),
            sink_span,
            advice_span,
            function,
        }));
    });
    findings
}

fn is_u32_presuming_arith_op(op: &Operation) -> bool {
    if !op.operands().iter().any(|operand| {
        let value = operand.borrow().as_value_ref();
        value.borrow().ty() == &Type::U32
    }) {
        return false;
    }

    op.is::<arith::Add>()
        || op.is::<arith::AddOverflowing>()
        || op.is::<arith::Sub>()
        || op.is::<arith::SubOverflowing>()
        || op.is::<arith::Mul>()
        || op.is::<arith::MulOverflowing>()
        || op.is::<arith::Div>()
        || op.is::<arith::Mod>()
        || op.is::<arith::Divmod>()
        || op.is::<arith::Band>()
        || op.is::<arith::Bor>()
        || op.is::<arith::Bxor>()
        || op.is::<arith::Shl>()
        || op.is::<arith::Shr>()
        || op.is::<arith::Rotl>()
        || op.is::<arith::Rotr>()
        || op.is::<arith::Eq>()
        || op.is::<arith::Neq>()
        || op.is::<arith::Gt>()
        || op.is::<arith::Gte>()
        || op.is::<arith::Lt>()
        || op.is::<arith::Lte>()
        || op.is::<arith::Min>()
        || op.is::<arith::Max>()
}
