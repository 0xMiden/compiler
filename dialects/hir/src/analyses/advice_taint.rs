use alloc::{
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
    CallOpInterface, Forward, Operation, OperationName, OperationRef, ProgramPoint, Report,
    SmallVec, SourceSpan, Spanned, Symbol, SymbolName, Type, Value, ValueRef,
    diagnostics::{Diagnostic, LabeledSpan, miette},
    dialects::builtin::{self, attributes::LocalVariable},
    pass::{Analysis, AnalysisManager, PreservedAnalyses},
};
use midenc_hir_analysis::{
    AnalysisStateGuard, AnalysisStateGuardMut, BuildableDataFlowAnalysis, CallControlFlowAction,
    DataFlowConfig, DataFlowSolver, DenseForwardDataFlowAnalysis, Lattice, LatticeLike,
    SparseForwardDataFlowAnalysis, SparseLattice,
    analyses::{DeadCodeAnalysis, SparseConstantPropagation},
    dense::DenseDataFlowAnalysis,
    sparse::SparseDataFlowAnalysis,
};

use crate::{
    AdviceLoadWord, AdvicePipe, AdvicePop, AssertU32, IntToPtr, Load, LoadLocal, Store, StoreLocal,
};

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
    /// Relevant call-boundary context for interprocedural propagation.
    pub contexts: Vec<AdviceTaintContext>,
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

/// A public/exported function returns an unconstrained value.
#[derive(Debug, Clone)]
pub struct AdviceTaintExitFinding {
    /// The public/exported function that returns the unconstrained value.
    pub function: SymbolName,
    /// The span of the function operation.
    pub function_span: SourceSpan,
    /// The return operation span.
    pub return_span: SourceSpan,
    /// The zero-based result index containing an unconstrained value.
    pub result_index: usize,
    /// The operation span from which the unconstrained value originated.
    pub advice_span: SourceSpan,
    /// The origin represented by `advice_span`.
    pub origin: AdviceTaintOrigin,
    /// Relevant call-boundary context for interprocedural propagation.
    pub contexts: Vec<AdviceTaintContext>,
}

impl AdviceTaintExitFinding {
    pub fn diagnostic(&self) -> AdviceTaintDiagnostic {
        AdviceTaintDiagnostic::new_exit(self)
    }

    pub fn into_report(&self) -> Report {
        self.diagnostic().into_report()
    }
}

/// An unconstrained value is passed to an external function parameter with a constrained type.
#[derive(Debug, Clone)]
pub struct AdviceTaintExternalCallFinding {
    /// The external call operation that receives the unconstrained argument.
    pub call: OperationName,
    /// The call operation span.
    pub call_span: SourceSpan,
    /// The zero-based external argument index.
    pub argument_index: usize,
    /// The constrained parameter type expected by the external callee.
    pub parameter_type: Type,
    /// The operation span from which the unconstrained value originated.
    pub advice_span: SourceSpan,
    /// The origin represented by `advice_span`.
    pub origin: AdviceTaintOrigin,
    /// The nearest containing function, when available.
    pub function: Option<SymbolName>,
}

impl AdviceTaintExternalCallFinding {
    pub fn diagnostic(&self) -> AdviceTaintDiagnostic {
        AdviceTaintDiagnostic::new_external_call(self)
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

/// The kind of call-boundary context associated with a tainted value.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum AdviceTaintContextKind {
    /// The unconstrained value is passed into another function as a call argument.
    CallArgument,
    /// The unconstrained value is returned from another function through a call result.
    CallResult,
}

/// Diagnostic context for a call boundary crossed by an unconstrained value.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct AdviceTaintContext {
    /// The call operation span.
    pub span: SourceSpan,
    /// How the tainted value crossed this call boundary.
    pub kind: AdviceTaintContextKind,
}

/// User-facing diagnostic for an unconstrained advice taint finding.
#[derive(Debug, Clone, Diagnostic, thiserror::Error)]
#[error("{message}")]
#[diagnostic(severity(Warning))]
pub struct AdviceTaintDiagnostic {
    message: String,
    #[help]
    help: String,
    #[label(collection)]
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
        let labels = vec![LabeledSpan::new_primary_with_span(Some(sink_label), finding.sink_span)];
        let labels = labels
            .into_iter()
            .chain(context_labels(&finding.contexts))
            .chain(core::iter::once(LabeledSpan::new_with_span(
                Some(origin_label),
                finding.advice_span,
            )))
            .collect();

        Self {
            message,
            help,
            labels,
        }
    }

    fn new_exit(finding: &AdviceTaintExitFinding) -> Self {
        let (subject, return_label, origin_label, help) = match finding.origin.kind {
            AdviceTaintOriginKind::Advice => (
                "unconstrained advice value",
                format!(
                    "public function returns unconstrained advice as result #{}",
                    finding.result_index
                ),
                "unconstrained advice originates here".to_string(),
                "constrain this value before returning it from a public function, or require \
                 callers to validate it before any constrained use"
                    .to_string(),
            ),
            AdviceTaintOriginKind::ExternalCall => (
                "unconstrained external call result",
                format!(
                    "public function returns an unconstrained external call result as result #{}",
                    finding.result_index
                ),
                "external call result is modeled as unconstrained here".to_string(),
                "add an explicit constraint before returning the external result, or provide an \
                 analyzable callee body/summary proving the result is constrained"
                    .to_string(),
            ),
        };
        let message = format!(
            "public function '{}' returns {subject} as result #{}",
            finding.function.as_str(),
            finding.result_index
        );
        let labels =
            vec![LabeledSpan::new_primary_with_span(Some(return_label), finding.return_span)];
        let labels = labels
            .into_iter()
            .chain(context_labels(&finding.contexts))
            .chain(core::iter::once(LabeledSpan::new_with_span(
                Some(origin_label),
                finding.advice_span,
            )))
            .collect();

        Self {
            message,
            help,
            labels,
        }
    }

    fn new_external_call(finding: &AdviceTaintExternalCallFinding) -> Self {
        let function = finding
            .function
            .map(|name| format!(" in function '{}'", name.as_str()))
            .unwrap_or_default();
        let (subject, origin_label) = match finding.origin.kind {
            AdviceTaintOriginKind::Advice => {
                ("unconstrained advice value", "unconstrained advice originates here".to_string())
            }
            AdviceTaintOriginKind::ExternalCall => (
                "unconstrained external call result",
                "external call result is modeled as unconstrained here".to_string(),
            ),
        };
        let message = format!(
            "{subject} is passed to external parameter #{} of type `{}`{}",
            finding.argument_index, finding.parameter_type, function
        );
        let labels = vec![
            LabeledSpan::new_primary_with_span(
                Some(format!(
                    "`{}` passes an unconstrained value to external parameter #{} typed `{}`",
                    finding.call, finding.argument_index, finding.parameter_type
                )),
                finding.call_span,
            ),
            LabeledSpan::new_with_span(Some(origin_label), finding.advice_span),
        ];
        let help = "add an explicit constraint before passing this value to the external callee, \
                    or provide an analyzable callee body/summary proving the parameter is handled \
                    safely"
            .to_string();

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

fn context_labels(contexts: &[AdviceTaintContext]) -> impl Iterator<Item = LabeledSpan> + '_ {
    contexts.iter().map(|context| {
        let label = match context.kind {
            AdviceTaintContextKind::CallArgument => {
                "unconstrained value is passed as a call argument here"
            }
            AdviceTaintContextKind::CallResult => "unconstrained value returns from a call here",
        };
        LabeledSpan::new_with_span(Some(label.to_string()), context.span)
    })
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

    pub fn contains_origin(&self, origin: AdviceTaintOrigin) -> bool {
        self.origins.contains_key(&origin)
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

const MAX_CALL_CONTEXT_DEPTH: usize = 4;

type CallContext = SmallVec<[CallContextFrame; MAX_CALL_CONTEXT_DEPTH]>;
type AdviceTaintFacts = ContextualAdviceTaintValue;
type AdviceTaintSparseLattice = Lattice<ContextualAdviceTaintValue>;

/// A callsite frame in the bounded call string used by advice taint.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd)]
struct CallContextFrame {
    id: usize,
    span: SourceSpan,
}

impl CallContextFrame {
    fn new(call: &dyn CallOpInterface) -> Self {
        let op = call.as_operation_ref();
        Self {
            id: OperationRef::as_ptr(&op) as usize,
            span: op.span(),
        }
    }
}

/// Sparse taint facts for an SSA value, partitioned by bounded call context.
///
/// The empty context is the baseline context: facts that hold without assuming any particular
/// caller-provided argument taint. Non-empty contexts represent facts derived from a specific
/// call string. Joining contextual facts unions the context maps and joins the inner taint lattice
/// when the same context appears on both sides.
#[derive(Clone, Eq, PartialEq)]
pub struct ContextualAdviceTaintValue {
    contexts: BTreeMap<CallContext, AdviceTaintValue>,
}

impl ContextualAdviceTaintValue {
    pub fn clean() -> Self {
        Self::from_inner(AdviceTaintValue::clean())
    }

    pub fn raw(span: SourceSpan) -> Self {
        Self::from_inner(AdviceTaintValue::raw(span))
    }

    pub fn external_call(span: SourceSpan) -> Self {
        Self::from_inner(AdviceTaintValue::external_call(span))
    }

    pub fn is_clean(&self) -> bool {
        self.contexts.values().all(AdviceTaintValue::is_clean)
    }

    pub fn has_unreported_origin(&self) -> bool {
        self.contexts.values().any(AdviceTaintValue::has_unreported_origin)
    }

    pub fn contains_origin(&self, origin: AdviceTaintOrigin) -> bool {
        self.contexts.values().any(|taint| taint.contains_origin(origin))
    }

    pub fn unreported_origins(&self) -> impl Iterator<Item = AdviceTaintOrigin> {
        let mut origins = Vec::new();
        for taint in self.contexts.values() {
            for origin in taint.unreported_origins() {
                if !origins.contains(&origin) {
                    origins.push(origin);
                }
            }
        }
        origins.into_iter()
    }

    pub fn mark_reported(&self) -> Self {
        Self {
            contexts: self
                .contexts
                .iter()
                .map(|(context, taint)| (context.clone(), taint.mark_reported()))
                .collect(),
        }
    }

    pub fn effective_taint(&self) -> AdviceTaintValue {
        AdviceTaintValue::join_all(self.contexts.values())
    }

    fn from_inner(taint: AdviceTaintValue) -> Self {
        Self {
            contexts: BTreeMap::from([(CallContext::new(), taint)]),
        }
    }

    fn enter_call(&self, frame: CallContextFrame) -> Self {
        let mut contexts = BTreeMap::new();
        for (context, taint) in self.contexts.iter() {
            Self::join_context(&mut contexts, push_call_context(context, frame), taint.clone());
        }
        Self { contexts }
    }

    fn exit_call(&self, frame: CallContextFrame) -> Self {
        let mut contexts = BTreeMap::new();
        for (context, taint) in self.contexts.iter() {
            if context.is_empty() {
                Self::join_context(&mut contexts, CallContext::new(), taint.clone());
                continue;
            }

            if context.last() == Some(&frame) {
                let mut caller_context = context.clone();
                caller_context.pop();
                Self::join_context(&mut contexts, caller_context, taint.clone());
            }
        }

        if contexts.is_empty() {
            Self::clean()
        } else {
            Self { contexts }
        }
    }

    fn join_all<'a>(values: impl IntoIterator<Item = &'a Self>) -> Self {
        values
            .into_iter()
            .fold(Self::clean(), |acc, value| LatticeLike::join(&acc, value))
    }

    fn join_context(
        contexts: &mut BTreeMap<CallContext, AdviceTaintValue>,
        context: CallContext,
        taint: AdviceTaintValue,
    ) {
        contexts
            .entry(context)
            .and_modify(|current| *current = LatticeLike::join(current, &taint))
            .or_insert(taint);
    }
}

impl Default for ContextualAdviceTaintValue {
    fn default() -> Self {
        Self::clean()
    }
}

impl fmt::Debug for ContextualAdviceTaintValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_map().entries(self.contexts.iter()).finish()
    }
}

impl LatticeLike for ContextualAdviceTaintValue {
    fn join(&self, other: &Self) -> Self {
        let mut contexts = self.contexts.clone();
        for (context, taint) in other.contexts.iter() {
            Self::join_context(&mut contexts, context.clone(), taint.clone());
        }
        Self { contexts }
    }

    fn meet(&self, other: &Self) -> Self {
        self.join(other)
    }
}

fn push_call_context(context: &CallContext, frame: CallContextFrame) -> CallContext {
    let mut pushed = context.clone();
    if pushed.len() == MAX_CALL_CONTEXT_DEPTH {
        pushed.remove(0);
    }
    pushed.push(frame);
    pushed
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
    type Lattice = AdviceTaintSparseLattice;

    fn debug_name(&self) -> &'static str {
        "unconstrained-advice-taint"
    }

    fn allow_unknown_predecessors(&self) -> bool {
        true
    }

    fn visit_operation(
        &self,
        op: &Operation,
        operands: &[AnalysisStateGuard<'_, Self::Lattice>],
        results: &mut [AnalysisStateGuardMut<'_, Self::Lattice>],
        _solver: &mut DataFlowSolver,
    ) -> Result<(), Report> {
        if op.is::<AdvicePipe>() {
            return join_advice_pipe_results(op, operands, results);
        }

        let operand_taint =
            AdviceTaintFacts::join_all(operands.iter().map(|operand| operand.value()));
        let result_taint = transfer_taint(op, operand_taint);
        join_results(results, &result_taint)
    }

    fn visit_call_control_flow_transfer(
        &self,
        call: &dyn CallOpInterface,
        action: CallControlFlowAction,
        before: &[AnalysisStateGuard<'_, Self::Lattice>],
        after: &mut [AnalysisStateGuardMut<'_, Self::Lattice>],
        _solver: &mut DataFlowSolver,
    ) {
        let frame = CallContextFrame::new(call);
        match action {
            CallControlFlowAction::Enter => {
                for (argument, parameter) in before.iter().zip(after.iter_mut()) {
                    parameter.join(&argument.value().enter_call(frame));
                }
            }
            CallControlFlowAction::Exit => {
                for (returned, result) in before.iter().zip(after.iter_mut()) {
                    result.join(&returned.value().exit_call(frame));
                }
            }
            CallControlFlowAction::External => {
                let span = call.as_operation().span();
                for (result_value, result) in call.as_operation().results().all().iter().zip(after)
                {
                    let result_value = result_value.borrow();
                    let value = if is_unconstrained_external_result_type(result_value.ty()) {
                        AdviceTaintFacts::external_call(span)
                    } else {
                        AdviceTaintFacts::clean()
                    };
                    result.join(&value);
                }
            }
        }
    }

    fn set_to_entry_state(&self, lattice: &mut AnalysisStateGuardMut<'_, Self::Lattice>) {
        lattice.join(&AdviceTaintFacts::clean());
    }
}

fn join_results(
    results: &mut [AnalysisStateGuardMut<'_, AdviceTaintSparseLattice>],
    value: &AdviceTaintFacts,
) -> Result<(), Report> {
    for result in results {
        result.join(value);
    }
    Ok(())
}

fn join_advice_pipe_results(
    op: &Operation,
    operands: &[AnalysisStateGuard<'_, AdviceTaintSparseLattice>],
    results: &mut [AnalysisStateGuardMut<'_, AdviceTaintSparseLattice>],
) -> Result<(), Report> {
    const RAW_ADVICE_RESULTS: usize = 8;

    for (index, result) in results.iter_mut().enumerate() {
        let taint = if index < RAW_ADVICE_RESULTS {
            AdviceTaintFacts::raw(op.span())
        } else {
            operands.get(index).map(|operand| operand.value().clone()).unwrap_or_default()
        };
        result.join(&taint);
    }

    Ok(())
}

/// Analysis wrapper that runs the sparse taint propagation and materializes diagnostics.
#[derive(Default)]
pub struct AdviceTaintAnalysis {
    solver: DataFlowSolver,
    findings: Vec<AdviceTaintFinding>,
    exit_findings: Vec<AdviceTaintExitFinding>,
    external_call_findings: Vec<AdviceTaintExternalCallFinding>,
}

impl AdviceTaintAnalysis {
    pub fn findings(&self) -> &[AdviceTaintFinding] {
        &self.findings
    }

    pub fn exit_findings(&self) -> &[AdviceTaintExitFinding] {
        &self.exit_findings
    }

    pub fn external_call_findings(&self) -> &[AdviceTaintExternalCallFinding] {
        &self.external_call_findings
    }

    pub fn diagnostics(&self) -> Vec<AdviceTaintDiagnostic> {
        self.findings
            .iter()
            .map(AdviceTaintFinding::diagnostic)
            .chain(self.exit_findings.iter().map(AdviceTaintExitFinding::diagnostic))
            .chain(
                self.external_call_findings
                    .iter()
                    .map(AdviceTaintExternalCallFinding::diagnostic),
            )
            .collect()
    }

    pub fn reports(&self) -> Vec<Report> {
        self.findings
            .iter()
            .map(AdviceTaintFinding::into_report)
            .chain(self.exit_findings.iter().map(AdviceTaintExitFinding::into_report))
            .chain(
                self.external_call_findings
                    .iter()
                    .map(AdviceTaintExternalCallFinding::into_report),
            )
            .collect()
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
        self.solver.load::<AdviceTaintStoragePropagation>();
        self.solver.initialize_and_run(op, analysis_manager)?;
        self.findings = collect_findings(op, &self.solver);
        self.exit_findings = collect_exit_findings(op, &self.solver);
        self.external_call_findings = collect_external_call_findings(op, &self.solver);
        Ok(())
    }

    fn invalidate(&self, _preserved_analyses: &mut PreservedAnalyses) -> bool {
        true
    }
}

/// Dense propagation of storage taint through local slots and memory.
///
/// Storage itself is modeled as dense program-point state, while load results are written back into
/// the sparse advice-taint value lattice. That lets the sparse solver propagate storage-derived
/// values through ordinary SSA use-def edges.
#[derive(Default)]
struct AdviceTaintStoragePropagation;

impl BuildableDataFlowAnalysis for AdviceTaintStoragePropagation {
    type Strategy = DenseDataFlowAnalysis<Self, Forward>;

    fn new(solver: &mut DataFlowSolver) -> Self {
        solver.load::<DeadCodeAnalysis>();
        solver.load::<AdviceTaintPropagation>();
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
        let mut state = required_storage_before_operation(op, dependent, solver);
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
        let dependent = midenc_hir_analysis::AnalysisState::anchor(after)
            .as_program_point()
            .unwrap_or_else(|| ProgramPoint::after(call.as_operation()));
        let frame = CallContextFrame::new(call);
        match action {
            CallControlFlowAction::Enter => {
                let state =
                    required_storage_before_operation(call.as_operation(), dependent, solver);
                midenc_hir_analysis::DenseLattice::join(
                    after,
                    &state.memory_only().enter_call(frame),
                );
            }
            CallControlFlowAction::Exit => {
                let mut state =
                    required_storage_before_operation(call.as_operation(), dependent, solver);
                state.replace_memory_from(&before.value().exit_call(frame));
                midenc_hir_analysis::DenseLattice::join(after, &state);
            }
            // External memory effects need summaries before we can model them conservatively.
            CallControlFlowAction::External => {
                let state =
                    required_storage_before_operation(call.as_operation(), dependent, solver);
                midenc_hir_analysis::DenseLattice::join(after, &state);
            }
        }
    }
}

#[derive(Debug, Clone, Default, Eq, PartialEq)]
struct StorageState {
    storage: BTreeMap<StorageKey, AdviceTaintFacts>,
    dynamic_memory: AdviceTaintFacts,
}

#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
enum StorageKey {
    Local(SymbolName, usize),
    Memory(u32),
}

fn required_storage_before_operation(
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
        join_solver_value_taint(load.result().as_value_ref(), &taint, solver);
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
        join_solver_value_taint(load.result().as_value_ref(), &taint, solver);
        return Ok(());
    }

    if let Some(pipe) = operation.downcast_ref::<AdvicePipe>() {
        let taint = AdviceTaintFacts::raw(operation.span());
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

fn transfer_taint(op: &Operation, operand_taint: AdviceTaintFacts) -> AdviceTaintFacts {
    if op.is::<AdvicePop>() || op.is::<AdviceLoadWord>() {
        return AdviceTaintFacts::raw(op.span());
    }

    if op.is::<AssertU32>() {
        return AdviceTaintFacts::clean();
    }

    if is_u32_presuming_sink(op) && operand_taint.has_unreported_origin() {
        operand_taint.mark_reported()
    } else {
        operand_taint
    }
}

impl StorageState {
    fn store(&mut self, key: StorageKey, taint: AdviceTaintFacts) {
        self.storage.insert(key, taint);
    }

    fn load(&self, key: &StorageKey) -> AdviceTaintFacts {
        self.storage.get(key).cloned().unwrap_or_default()
    }

    fn store_dynamic_memory(&mut self, taint: AdviceTaintFacts) {
        self.dynamic_memory = LatticeLike::join(&self.dynamic_memory, &taint);
    }

    fn load_memory(&self, addr: u32) -> AdviceTaintFacts {
        LatticeLike::join(&self.load(&StorageKey::Memory(addr)), &self.dynamic_memory)
    }

    fn load_dynamic_memory(&self) -> AdviceTaintFacts {
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

    fn map_taint(&self, mut f: impl FnMut(&AdviceTaintFacts) -> AdviceTaintFacts) -> Self {
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

fn required_value_taint(
    value: ValueRef,
    dependent: ProgramPoint,
    solver: &mut DataFlowSolver,
) -> AdviceTaintFacts {
    solver.require::<AdviceTaintSparseLattice, _>(value, dependent).value().clone()
}

fn join_solver_value_taint(value: ValueRef, taint: &AdviceTaintFacts, solver: &mut DataFlowSolver) {
    let mut lattice = solver.get_or_create_mut::<AdviceTaintSparseLattice, _>(value);
    SparseLattice::join(&mut *lattice, taint);
}

fn value_taint(value: ValueRef, solver: &DataFlowSolver) -> AdviceTaintFacts {
    solver
        .get::<AdviceTaintSparseLattice, _>(&value)
        .map(|state| state.value().clone())
        .unwrap_or_default()
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

fn collect_findings(op: &Operation, solver: &DataFlowSolver) -> Vec<AdviceTaintFinding> {
    let mut findings = Vec::new();
    op.prewalk_all(|operation| {
        if !is_u32_presuming_sink(operation) {
            return;
        }

        let mut operand_taint = AdviceTaintFacts::clean();
        for operand in operation.operands().iter() {
            let value = operand.borrow().as_value_ref();
            operand_taint = LatticeLike::join(&operand_taint, &value_taint(value, solver));
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
                contexts: collect_call_contexts(op, solver, function, origin),
                function,
            };
            if !findings.iter().any(|existing| same_finding(existing, &finding)) {
                findings.push(finding);
            }
        }
    });
    findings
}

fn collect_exit_findings(op: &Operation, solver: &DataFlowSolver) -> Vec<AdviceTaintExitFinding> {
    let mut findings = Vec::new();
    op.prewalk_all(|operation| {
        let Some(ret) = operation.downcast_ref::<builtin::Ret>() else {
            return;
        };
        let Some(function_ref) = operation.nearest_parent_op::<builtin::Function>() else {
            return;
        };
        let function = function_ref.borrow();
        if !Symbol::is_public(&*function) {
            return;
        }

        let function_name = Symbol::name(&*function);
        let function_span = function.as_symbol_operation().span();
        let return_span = operation.span();
        for (result_index, operand) in ret.values().iter().enumerate() {
            let value = operand.borrow().as_value_ref();
            let taint = value_taint(value, solver);
            if taint.is_clean() || !taint.has_unreported_origin() {
                continue;
            }

            for origin in taint.unreported_origins() {
                let finding = AdviceTaintExitFinding {
                    function: function_name,
                    function_span,
                    return_span,
                    result_index,
                    advice_span: origin.span,
                    origin,
                    contexts: collect_call_contexts(op, solver, Some(function_name), origin),
                };
                if !findings.iter().any(|existing| same_exit_finding(existing, &finding)) {
                    findings.push(finding);
                }
            }
        }
    });
    findings
}

fn collect_external_call_findings(
    op: &Operation,
    solver: &DataFlowSolver,
) -> Vec<AdviceTaintExternalCallFinding> {
    let mut findings = Vec::new();
    op.prewalk_all(|operation| {
        let Some(call) = operation.as_trait::<dyn CallOpInterface>() else {
            return;
        };
        if !is_external_call(call) {
            return;
        }

        let Some(param_types) = external_call_param_types(call) else {
            return;
        };
        let function = operation.nearest_parent_op::<builtin::Function>().map(|function| {
            let function = function.borrow();
            Symbol::name(&*function)
        });
        for (argument_index, (argument, parameter_type)) in
            call.arguments().iter().zip(param_types).enumerate()
        {
            if !is_constrained_external_parameter_type(&parameter_type) {
                continue;
            }

            let taint = value_taint(argument.borrow().as_value_ref(), solver);
            if taint.is_clean() || !taint.has_unreported_origin() {
                continue;
            }

            for origin in taint.unreported_origins() {
                let finding = AdviceTaintExternalCallFinding {
                    call: operation.name(),
                    call_span: operation.span(),
                    argument_index,
                    parameter_type: parameter_type.clone(),
                    advice_span: origin.span,
                    origin,
                    function,
                };
                if !findings.iter().any(|existing| same_external_call_finding(existing, &finding)) {
                    findings.push(finding);
                }
            }
        }
    });
    findings
}

fn is_external_call(call: &dyn CallOpInterface) -> bool {
    let Some(callee) = call.resolve() else {
        return true;
    };
    let callee = callee.borrow();
    callee
        .as_symbol_operation()
        .downcast_ref::<builtin::Function>()
        .is_some_and(Symbol::is_declaration)
}

fn external_call_param_types(call: &dyn CallOpInterface) -> Option<Vec<Type>> {
    let callee = call.resolve()?;
    let callee = callee.borrow();
    let function = callee.as_symbol_operation().downcast_ref::<builtin::Function>()?;
    Some(function.get_signature().params().iter().map(|param| param.ty.clone()).collect())
}

fn is_constrained_external_parameter_type(ty: &Type) -> bool {
    matches!(ty, Type::U32 | Type::U16 | Type::U8 | Type::I1)
}

fn is_unconstrained_external_result_type(ty: &Type) -> bool {
    matches!(ty, Type::Felt | Type::Array(_))
}

fn collect_call_contexts(
    root: &Operation,
    solver: &DataFlowSolver,
    use_function: Option<SymbolName>,
    origin: AdviceTaintOrigin,
) -> Vec<AdviceTaintContext> {
    let Some(use_function) = use_function else {
        return Vec::new();
    };

    let mut contexts = Vec::new();
    root.prewalk_all(|operation| {
        if operation.span() == origin.span {
            return;
        }
        let Some(call) = operation.as_trait::<dyn CallOpInterface>() else {
            return;
        };

        let caller_function = operation.nearest_parent_op::<builtin::Function>().map(|function| {
            let function = function.borrow();
            Symbol::name(&*function)
        });
        let callee_function = resolved_callee_function_name(call);

        if caller_function == Some(use_function)
            && call_results_contain_origin(operation, solver, origin)
        {
            push_context(&mut contexts, operation.span(), AdviceTaintContextKind::CallResult);
        }

        if callee_function == Some(use_function)
            && call_arguments_contain_origin(call, solver, origin)
        {
            push_context(&mut contexts, operation.span(), AdviceTaintContextKind::CallArgument);
        }
    });
    contexts
}

fn resolved_callee_function_name(call: &dyn CallOpInterface) -> Option<SymbolName> {
    let callee = call.resolve()?;
    let callee = callee.borrow();
    callee
        .as_symbol_operation()
        .downcast_ref::<builtin::Function>()
        .map(Symbol::name)
}

fn call_results_contain_origin(
    call: &Operation,
    solver: &DataFlowSolver,
    origin: AdviceTaintOrigin,
) -> bool {
    call.results().all().iter().any(|result| {
        let value = result.borrow().as_value_ref();
        value_taint(value, solver).contains_origin(origin)
    })
}

fn call_arguments_contain_origin(
    call: &dyn CallOpInterface,
    solver: &DataFlowSolver,
    origin: AdviceTaintOrigin,
) -> bool {
    call.arguments().iter().any(|operand| {
        let value = operand.borrow().as_value_ref();
        value_taint(value, solver).contains_origin(origin)
    })
}

fn push_context(
    contexts: &mut Vec<AdviceTaintContext>,
    span: SourceSpan,
    kind: AdviceTaintContextKind,
) {
    let context = AdviceTaintContext { span, kind };
    if !contexts.contains(&context) {
        contexts.push(context);
    }
}

fn same_finding(lhs: &AdviceTaintFinding, rhs: &AdviceTaintFinding) -> bool {
    lhs.sink == rhs.sink
        && lhs.sink_span == rhs.sink_span
        && lhs.origin == rhs.origin
        && lhs.function == rhs.function
}

fn same_exit_finding(lhs: &AdviceTaintExitFinding, rhs: &AdviceTaintExitFinding) -> bool {
    lhs.function == rhs.function
        && lhs.return_span == rhs.return_span
        && lhs.result_index == rhs.result_index
        && lhs.origin == rhs.origin
}

fn same_external_call_finding(
    lhs: &AdviceTaintExternalCallFinding,
    rhs: &AdviceTaintExternalCallFinding,
) -> bool {
    lhs.call == rhs.call
        && lhs.call_span == rhs.call_span
        && lhs.argument_index == rhs.argument_index
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
