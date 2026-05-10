use alloc::{
    boxed::Box,
    collections::BTreeMap,
    format,
    rc::Rc,
    string::{String, ToString},
    vec,
    vec::Vec,
};
use core::{any::Any, fmt};

use midenc_dialect_arith as arith;
use midenc_hir::{
    CallOpInterface, Forward, Operation, OperationName, Report, SourceSpan, Spanned, Symbol,
    SymbolName, Type, Value,
    diagnostics::{Diagnostic, LabeledSpan, Severity},
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
    /// The operation span from which the unconstrained value originated.
    pub advice_span: SourceSpan,
    /// The origin represented by `advice_span`.
    pub origin: AdviceTaintOrigin,
    /// The nearest containing function, when available.
    pub function: Option<SymbolName>,
}

impl AdviceTaintFinding {
    pub fn diagnostic(&self) -> AdviceTaintDiagnostic {
        AdviceTaintDiagnostic::new(self)
    }

    pub fn into_report(&self) -> Report {
        self.diagnostic().into_report()
    }
}

/// The kind of unconstrained value origin tracked by advice taint.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub enum AdviceTaintOriginKind {
    /// A value produced by a MASM advice operation.
    Advice,
    /// A value returned by an external call whose body is unavailable to the analysis.
    ExternalCall,
}

/// Provenance for an unconstrained value tracked by advice taint.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct AdviceTaintOrigin {
    /// The operation span at which the unconstrained value originated.
    pub span: SourceSpan,
    /// The kind of origin represented by `span`.
    pub kind: AdviceTaintOriginKind,
}

impl AdviceTaintOrigin {
    pub fn advice(span: SourceSpan) -> Self {
        Self {
            span,
            kind: AdviceTaintOriginKind::Advice,
        }
    }

    pub fn external_call(span: SourceSpan) -> Self {
        Self {
            span,
            kind: AdviceTaintOriginKind::ExternalCall,
        }
    }
}

/// User-facing diagnostic for an unconstrained advice taint finding.
#[derive(Debug, Clone)]
pub struct AdviceTaintDiagnostic {
    message: String,
    help: String,
    labels: Vec<LabeledSpan>,
}

impl AdviceTaintDiagnostic {
    fn new(finding: &AdviceTaintFinding) -> Self {
        let function = finding
            .function
            .map(|name| format!(" in function '{}'", name.as_str()))
            .unwrap_or_default();
        let (subject, sink_label, origin_label, help) = match finding.origin.kind {
            AdviceTaintOriginKind::Advice => (
                "unconstrained advice value",
                format!("`{}` consumes unconstrained advice as a u32", finding.sink),
                "unconstrained advice originates here".to_string(),
                "add an explicit u32 range check, such as `u32assert` or `u32test` followed by \
                 `assert`, before this value is consumed by a u32-presuming operation"
                    .to_string(),
            ),
            AdviceTaintOriginKind::ExternalCall => (
                "unconstrained external call result",
                format!(
                    "`{}` consumes an unconstrained external call result as a u32",
                    finding.sink
                ),
                "external call result is modeled as unconstrained here".to_string(),
                "add an explicit u32 range check after the external call, or provide an \
                 analyzable callee body/summary proving the result is constrained before this \
                 u32-presuming operation"
                    .to_string(),
            ),
        };
        let message =
            format!("{subject} reaches u32-presuming operation `{}`{}", finding.sink, function);
        let labels = vec![
            LabeledSpan::new_primary_with_span(Some(sink_label), finding.sink_span),
            LabeledSpan::new_with_span(Some(origin_label), finding.advice_span),
        ];

        Self {
            message,
            help,
            labels,
        }
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    pub fn help_message(&self) -> &str {
        &self.help
    }

    pub fn label_messages(&self) -> impl Iterator<Item = &str> {
        self.labels.iter().filter_map(|label| label.label())
    }

    pub fn into_report(self) -> Report {
        Report::from(self)
    }
}

impl fmt::Display for AdviceTaintDiagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl core::error::Error for AdviceTaintDiagnostic {}

impl Diagnostic for AdviceTaintDiagnostic {
    fn severity(&self) -> Option<Severity> {
        Some(Severity::Warning)
    }

    fn help<'a>(&'a self) -> Option<Box<dyn fmt::Display + 'a>> {
        Some(Box::new(&self.help))
    }

    fn labels(&self) -> Option<Box<dyn Iterator<Item = LabeledSpan> + '_>> {
        Some(Box::new(self.labels.iter().cloned()))
    }
}

/// Sparse taint facts for an SSA value.
///
/// Each tracked origin is either still unreported on at least one path, or has already reached an
/// unsafe sink on all paths represented by the value. Keeping reported origins in the lattice lets
/// downstream u32 operations avoid duplicate diagnostics along the same path.
#[derive(Clone, Eq, PartialEq)]
pub struct AdviceTaintValue {
    origins: BTreeMap<AdviceTaintOrigin, OriginState>,
}

impl AdviceTaintValue {
    pub fn clean() -> Self {
        Self {
            origins: BTreeMap::new(),
        }
    }

    pub fn raw(span: SourceSpan) -> Self {
        Self {
            origins: BTreeMap::from([(AdviceTaintOrigin::advice(span), OriginState::Unreported)]),
        }
    }

    pub fn external_call(span: SourceSpan) -> Self {
        Self {
            origins: BTreeMap::from([(
                AdviceTaintOrigin::external_call(span),
                OriginState::Unreported,
            )]),
        }
    }

    pub fn is_clean(&self) -> bool {
        self.origins.is_empty()
    }

    pub fn has_unreported_origin(&self) -> bool {
        self.origins.values().any(|state| state.is_unreported())
    }

    pub fn unreported_origins(&self) -> impl Iterator<Item = AdviceTaintOrigin> + '_ {
        self.origins.iter().filter_map(|(origin, state)| {
            if state.is_unreported() {
                Some(*origin)
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
                .map(|origin| (origin, OriginState::Reported))
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
        for (&origin, &state) in other.origins.iter() {
            joined
                .entry(origin)
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
        let result_taint = if is_u32_presuming_sink(op) && operand_taint.has_unreported_origin() {
            operand_taint.mark_reported()
        } else {
            operand_taint
        };
        join_results(results, &result_taint)
    }

    fn visit_external_call(
        &self,
        call: &dyn CallOpInterface,
        _arguments: &[AnalysisStateGuard<'_, Self::Lattice>],
        results: &mut [AnalysisStateGuardMut<'_, Self::Lattice>],
        _solver: &mut DataFlowSolver,
    ) {
        let value = AdviceTaintValue::external_call(call.as_operation().span());
        for result in results {
            result.join(&value);
        }
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

    pub fn diagnostics(&self) -> Vec<AdviceTaintDiagnostic> {
        self.findings.iter().map(AdviceTaintFinding::diagnostic).collect()
    }

    pub fn reports(&self) -> Vec<Report> {
        self.findings.iter().map(AdviceTaintFinding::into_report).collect()
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
        if !is_u32_presuming_sink(operation) {
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
        for origin in operand_taint.unreported_origins() {
            let finding = AdviceTaintFinding {
                sink: sink.clone(),
                sink_span,
                advice_span: origin.span,
                origin,
                function,
            };
            if !findings.iter().any(|existing| same_finding(existing, &finding)) {
                findings.push(finding);
            }
        }
    });
    findings
}

fn same_finding(lhs: &AdviceTaintFinding, rhs: &AdviceTaintFinding) -> bool {
    lhs.sink == rhs.sink
        && lhs.sink_span == rhs.sink_span
        && lhs.origin == rhs.origin
        && lhs.function == rhs.function
}

fn is_u32_presuming_sink(op: &Operation) -> bool {
    is_u32_presuming_arith_op(op) || is_u32_to_u64_zext(op)
}

fn is_u32_presuming_arith_op(op: &Operation) -> bool {
    if !has_u32_operand(op) {
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
        || op.is::<arith::Bnot>()
        || op.is::<arith::Popcnt>()
        || op.is::<arith::Ctz>()
        || op.is::<arith::Clz>()
        || op.is::<arith::Clo>()
        || op.is::<arith::Cto>()
}

fn is_u32_to_u64_zext(op: &Operation) -> bool {
    // MASM widening/add3/madd u32 instructions lower by first refining operands to u32, then
    // zero-extending them to u64 for the widened arithmetic. The zext is the u32-consuming
    // boundary that remains visible after lifting.
    op.is::<arith::Zext>()
        && has_u32_operand(op)
        && op.results().all().iter().any(|result| result.borrow().ty() == &Type::U64)
}

fn has_u32_operand(op: &Operation) -> bool {
    op.operands().iter().any(|operand| {
        let value = operand.borrow().as_value_ref();
        value.borrow().ty() == &Type::U32
    })
}
