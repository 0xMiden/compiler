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
use midenc_dialect_scf as scf;
use midenc_hir::{
    CallOpInterface, Forward, Op, Operation, OperationName, RegionRef, Report, SourceSpan, Spanned,
    Symbol, SymbolName, Type, Value, ValueRef,
    diagnostics::{Diagnostic, LabeledSpan, Severity},
    dialects::builtin::{self, attributes::LocalVariable},
    pass::{Analysis, AnalysisManager, PreservedAnalyses},
};
use midenc_hir_analysis::{
    AnalysisStateGuard, AnalysisStateGuardMut, BuildableDataFlowAnalysis, DataFlowConfig,
    DataFlowSolver, Lattice, LatticeLike, SparseForwardDataFlowAnalysis, SparseLattice,
    analyses::{DeadCodeAnalysis, SparseConstantPropagation},
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
        if op.is::<AdvicePipe>() {
            return join_advice_pipe_results(op, operands, results);
        }

        let operand_taint =
            AdviceTaintValue::join_all(operands.iter().map(|operand| operand.value()));
        let result_taint = transfer_taint(op, operand_taint);
        join_results(results, &result_taint)
    }

    fn visit_external_call(
        &self,
        call: &dyn CallOpInterface,
        _arguments: &[AnalysisStateGuard<'_, Self::Lattice>],
        results: &mut [AnalysisStateGuardMut<'_, Self::Lattice>],
        _solver: &mut DataFlowSolver,
    ) {
        let span = call.as_operation().span();
        for (result_value, result) in call.as_operation().results().all().iter().zip(results) {
            let result_value = result_value.borrow();
            let value = if is_unconstrained_external_result_type(result_value.ty()) {
                AdviceTaintValue::external_call(span)
            } else {
                AdviceTaintValue::clean()
            };
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

fn join_advice_pipe_results(
    op: &Operation,
    operands: &[AnalysisStateGuard<'_, Lattice<AdviceTaintValue>>],
    results: &mut [AnalysisStateGuardMut<'_, Lattice<AdviceTaintValue>>],
) -> Result<(), Report> {
    const RAW_ADVICE_RESULTS: usize = 8;

    for (index, result) in results.iter_mut().enumerate() {
        let taint = if index < RAW_ADVICE_RESULTS {
            AdviceTaintValue::raw(op.span())
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
    storage_overlay: StorageTaintOverlay,
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
        self.solver.initialize_and_run(op, analysis_manager)?;
        self.storage_overlay = collect_storage_taint_overlay(op, &self.solver);
        self.findings = collect_findings(op, &self.solver, &self.storage_overlay);
        self.exit_findings = collect_exit_findings(op, &self.solver, &self.storage_overlay);
        self.external_call_findings =
            collect_external_call_findings(op, &self.solver, &self.storage_overlay);
        Ok(())
    }

    fn invalidate(&self, _preserved_analyses: &mut PreservedAnalyses) -> bool {
        true
    }
}

#[derive(Default)]
struct StorageTaintOverlay {
    values: BTreeMap<String, AdviceTaintValue>,
}

#[derive(Clone, Default, Eq, PartialEq)]
struct StorageState {
    storage: BTreeMap<StorageKey, AdviceTaintValue>,
    dynamic_memory: AdviceTaintValue,
}

#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
enum StorageKey {
    Local(SymbolName, usize),
    Memory(u32),
}

fn collect_storage_taint_overlay(op: &Operation, solver: &DataFlowSolver) -> StorageTaintOverlay {
    let mut overlay = StorageTaintOverlay::default();
    collect_storage_taint_for_operation_tree(op, solver, &mut overlay);
    overlay
}

fn collect_storage_taint_for_operation_tree(
    op: &Operation,
    solver: &DataFlowSolver,
    overlay: &mut StorageTaintOverlay,
) {
    if let Some(function) = op.downcast_ref::<builtin::Function>() {
        if !function.body().is_empty() {
            let mut state = StorageState::default();
            let mut call_stack = vec![Symbol::name(function)];
            transfer_region(
                function.body().as_region_ref(),
                &mut state,
                solver,
                overlay,
                &mut call_stack,
            );
        }
        return;
    }

    for region in op.regions() {
        let mut state = StorageState::default();
        let mut call_stack = Vec::new();
        transfer_region(region.as_region_ref(), &mut state, solver, overlay, &mut call_stack);
    }
}

fn transfer_region(
    region: RegionRef,
    state: &mut StorageState,
    solver: &DataFlowSolver,
    overlay: &mut StorageTaintOverlay,
    call_stack: &mut Vec<SymbolName>,
) {
    let region = region.borrow();
    for block in region.body() {
        for operation in block.body() {
            transfer_storage_operation(&operation, state, solver, overlay, call_stack);
        }
    }
}

fn transfer_storage_operation(
    operation: &Operation,
    state: &mut StorageState,
    solver: &DataFlowSolver,
    overlay: &mut StorageTaintOverlay,
    call_stack: &mut Vec<SymbolName>,
) {
    if let Some(function) = operation.downcast_ref::<builtin::Function>() {
        if !function.body().is_empty() {
            let mut function_state = StorageState::default();
            let mut function_call_stack = vec![Symbol::name(function)];
            transfer_region(
                function.body().as_region_ref(),
                &mut function_state,
                solver,
                overlay,
                &mut function_call_stack,
            );
        }
        return;
    }

    if let Some(if_op) = operation.downcast_ref::<scf::If>() {
        transfer_storage_if(if_op, state, solver, overlay, call_stack);
        return;
    }

    if let Some(while_op) = operation.downcast_ref::<scf::While>() {
        transfer_storage_while(while_op, state, solver, overlay, call_stack);
        return;
    }

    if let Some(store) = operation.downcast_ref::<StoreLocal>() {
        let key = StorageKey::from(*store.get_local());
        let taint = value_taint(store.value().as_value_ref(), solver, overlay);
        state.store(key, taint);
        return;
    }

    if let Some(load) = operation.downcast_ref::<LoadLocal>() {
        let key = StorageKey::from(*load.get_local());
        overlay.set(load.result().as_value_ref(), state.load(&key));
        return;
    }

    if let Some(store) = operation.downcast_ref::<Store>() {
        let taint = value_taint(store.value().as_value_ref(), solver, overlay);
        match memory_storage_key(store.addr().as_value_ref()) {
            Some(key) => state.store(key, taint),
            None => state.store_dynamic_memory(taint),
        }
        return;
    }

    if let Some(load) = operation.downcast_ref::<Load>() {
        let taint = match memory_storage_key(load.addr().as_value_ref()) {
            Some(StorageKey::Memory(addr)) => state.load_memory(addr),
            Some(key) => state.load(&key),
            None => state.load_dynamic_memory(),
        };
        overlay.set(load.result().as_value_ref(), taint);
        return;
    }

    if let Some(pipe) = operation.downcast_ref::<AdvicePipe>() {
        let taint = AdviceTaintValue::raw(operation.span());
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
        transfer_advice_pipe_storage_results(pipe, solver, overlay);
        return;
    }

    if let Some(call) = operation.as_trait::<dyn CallOpInterface>() {
        transfer_storage_call(call, state, solver, overlay, call_stack);
        return;
    }

    transfer_storage_results(operation, solver, overlay);
}

fn transfer_storage_if(
    if_op: &scf::If,
    state: &mut StorageState,
    solver: &DataFlowSolver,
    overlay: &mut StorageTaintOverlay,
    call_stack: &mut Vec<SymbolName>,
) {
    let entry_state = state.clone();
    let mut then_state = entry_state.clone();
    transfer_region(
        if_op.then_body().as_region_ref(),
        &mut then_state,
        solver,
        overlay,
        call_stack,
    );

    let mut else_state = entry_state;
    if !if_op.else_body().is_empty() {
        transfer_region(
            if_op.else_body().as_region_ref(),
            &mut else_state,
            solver,
            overlay,
            call_stack,
        );
    }

    overlay_if_results(if_op, solver, overlay);
    *state = then_state.join(&else_state);
}

fn transfer_storage_while(
    while_op: &scf::While,
    state: &mut StorageState,
    solver: &DataFlowSolver,
    overlay: &mut StorageTaintOverlay,
    call_stack: &mut Vec<SymbolName>,
) {
    let entry_state = state.clone();
    let mut loop_state = state.clone();

    // Iterate to a fixed point. Storage taint is finite for a fixed function because each store
    // key can only accumulate origins, and this avoids missing loop-carried taint when a value is
    // stored late in one iteration and loaded early in the next.
    loop {
        let previous = loop_state.clone();
        let mut iteration = loop_state.clone();
        transfer_region(
            while_op.before().as_region_ref(),
            &mut iteration,
            solver,
            overlay,
            call_stack,
        );
        transfer_region(
            while_op.after().as_region_ref(),
            &mut iteration,
            solver,
            overlay,
            call_stack,
        );
        loop_state = loop_state.join(&iteration);
        if loop_state == previous {
            break;
        }
    }

    overlay_while_results(while_op, solver, overlay);
    *state = entry_state.join(&loop_state);
}

fn transfer_storage_call(
    call: &dyn CallOpInterface,
    state: &mut StorageState,
    solver: &DataFlowSolver,
    overlay: &mut StorageTaintOverlay,
    call_stack: &mut Vec<SymbolName>,
) {
    let Some((callee_name, callee_region)) = resolved_defined_callee(call) else {
        return;
    };
    if call_stack.contains(&callee_name) {
        return;
    }

    let mut callee_state = state.memory_only();
    call_stack.push(callee_name);
    transfer_region(callee_region, &mut callee_state, solver, overlay, call_stack);
    call_stack.pop();

    overlay_call_results_from_callee_returns(call.as_operation(), callee_region, solver, overlay);
    state.replace_memory_from(&callee_state);
}

fn overlay_call_results_from_callee_returns(
    call: &Operation,
    callee_region: RegionRef,
    solver: &DataFlowSolver,
    overlay: &mut StorageTaintOverlay,
) {
    if !call.has_results() {
        return;
    }

    let mut result_taints = vec![AdviceTaintValue::clean(); call.results().all().len()];
    callee_region.borrow().prewalk_all(|operation| {
        let Some(ret) = operation.downcast_ref::<builtin::Ret>() else {
            return;
        };

        for (index, operand) in ret.values().iter().enumerate() {
            let Some(result_taint) = result_taints.get_mut(index) else {
                continue;
            };
            let taint = value_taint(operand.borrow().as_value_ref(), solver, overlay);
            *result_taint = LatticeLike::join(result_taint, &taint);
        }
    });

    for (result, taint) in call.results().all().iter().zip(result_taints) {
        overlay.set(result.borrow().as_value_ref(), taint);
    }
}

fn transfer_storage_results(
    operation: &Operation,
    solver: &DataFlowSolver,
    overlay: &mut StorageTaintOverlay,
) {
    if !operation.has_results() {
        return;
    }

    let operand_taint =
        operation.operands().iter().fold(AdviceTaintValue::clean(), |acc, operand| {
            let taint = value_taint(operand.borrow().as_value_ref(), solver, overlay);
            LatticeLike::join(&acc, &taint)
        });
    let result_taint = transfer_taint(operation, operand_taint);
    if result_taint.is_clean() {
        return;
    }

    for result in operation.results().all() {
        overlay.set(result.borrow().as_value_ref(), result_taint.clone());
    }
}

fn transfer_advice_pipe_storage_results(
    pipe: &AdvicePipe,
    solver: &DataFlowSolver,
    overlay: &mut StorageTaintOverlay,
) {
    const RAW_ADVICE_RESULTS: usize = 8;

    let operation = pipe.as_operation();
    for (index, result) in operation.results().all().iter().enumerate() {
        let taint = if index < RAW_ADVICE_RESULTS {
            AdviceTaintValue::raw(operation.span())
        } else {
            pipe.stack()
                .iter()
                .nth(index)
                .map(|operand| value_taint(operand.borrow().as_value_ref(), solver, overlay))
                .unwrap_or_default()
        };
        overlay.set(result.borrow().as_value_ref(), taint);
    }
}

fn overlay_if_results(if_op: &scf::If, solver: &DataFlowSolver, overlay: &mut StorageTaintOverlay) {
    if !if_op.as_operation().has_results() {
        return;
    }

    let then_yield = if_op.then_yield();
    let else_yield = if_op.else_yield();
    let then_yield = then_yield.borrow();
    let else_yield = else_yield.borrow();
    for ((result, then_value), else_value) in if_op
        .as_operation()
        .results()
        .all()
        .iter()
        .zip(then_yield.yielded().iter())
        .zip(else_yield.yielded().iter())
    {
        let taint = LatticeLike::join(
            &value_taint(then_value.borrow().as_value_ref(), solver, overlay),
            &value_taint(else_value.borrow().as_value_ref(), solver, overlay),
        );
        overlay.set(result.borrow().as_value_ref(), taint);
    }
}

fn overlay_while_results(
    while_op: &scf::While,
    solver: &DataFlowSolver,
    overlay: &mut StorageTaintOverlay,
) {
    if !while_op.as_operation().has_results() {
        return;
    }

    let yield_op = while_op.yield_op();
    let yield_op = yield_op.borrow();
    for ((result, init), yielded) in while_op
        .as_operation()
        .results()
        .all()
        .iter()
        .zip(while_op.inits().iter())
        .zip(yield_op.yielded().iter())
    {
        let taint = LatticeLike::join(
            &value_taint(init.borrow().as_value_ref(), solver, overlay),
            &value_taint(yielded.borrow().as_value_ref(), solver, overlay),
        );
        overlay.set(result.borrow().as_value_ref(), taint);
    }
}

fn transfer_taint(op: &Operation, operand_taint: AdviceTaintValue) -> AdviceTaintValue {
    if op.is::<AdvicePop>() || op.is::<AdviceLoadWord>() {
        return AdviceTaintValue::raw(op.span());
    }

    if op.is::<AssertU32>() {
        return AdviceTaintValue::clean();
    }

    if is_u32_presuming_sink(op) && operand_taint.has_unreported_origin() {
        operand_taint.mark_reported()
    } else {
        operand_taint
    }
}

impl StorageTaintOverlay {
    fn get(&self, value: ValueRef) -> Option<&AdviceTaintValue> {
        self.values.get(&value_key(value))
    }

    fn set(&mut self, value: ValueRef, taint: AdviceTaintValue) {
        self.values
            .entry(value_key(value))
            .and_modify(|current| *current = LatticeLike::join(current, &taint))
            .or_insert(taint);
    }
}

impl StorageState {
    fn store(&mut self, key: StorageKey, taint: AdviceTaintValue) {
        self.storage.insert(key, taint);
    }

    fn load(&self, key: &StorageKey) -> AdviceTaintValue {
        self.storage.get(key).cloned().unwrap_or_default()
    }

    fn store_dynamic_memory(&mut self, taint: AdviceTaintValue) {
        self.dynamic_memory = LatticeLike::join(&self.dynamic_memory, &taint);
    }

    fn load_memory(&self, addr: u32) -> AdviceTaintValue {
        LatticeLike::join(&self.load(&StorageKey::Memory(addr)), &self.dynamic_memory)
    }

    fn load_dynamic_memory(&self) -> AdviceTaintValue {
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

fn resolved_defined_callee(call: &dyn CallOpInterface) -> Option<(SymbolName, RegionRef)> {
    let callee = call.resolve()?;
    let callee = callee.borrow();
    let function = callee.as_symbol_operation().downcast_ref::<builtin::Function>()?;
    if Symbol::is_declaration(function) {
        return None;
    }
    Some((Symbol::name(function), function.body().as_region_ref()))
}

impl From<LocalVariable> for StorageKey {
    fn from(local: LocalVariable) -> Self {
        let function = local.function();
        let function = function.borrow();
        Self::Local(Symbol::name(&*function), local.as_usize())
    }
}

fn value_taint(
    value: ValueRef,
    solver: &DataFlowSolver,
    overlay: &StorageTaintOverlay,
) -> AdviceTaintValue {
    let solver_taint = solver
        .get::<Lattice<AdviceTaintValue>, _>(&value)
        .map(|state| state.value().clone())
        .unwrap_or_default();
    match overlay.get(value) {
        Some(overlay_taint) => LatticeLike::join(&solver_taint, overlay_taint),
        None => solver_taint,
    }
}

fn value_key(value: ValueRef) -> String {
    format!("{}", value.borrow())
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

fn collect_findings(
    op: &Operation,
    solver: &DataFlowSolver,
    overlay: &StorageTaintOverlay,
) -> Vec<AdviceTaintFinding> {
    let mut findings = Vec::new();
    op.prewalk_all(|operation| {
        if !is_u32_presuming_sink(operation) {
            return;
        }

        let mut operand_taint = AdviceTaintValue::clean();
        for operand in operation.operands().iter() {
            let value = operand.borrow().as_value_ref();
            operand_taint = LatticeLike::join(&operand_taint, &value_taint(value, solver, overlay));
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
                contexts: collect_call_contexts(op, solver, overlay, function, origin),
                function,
            };
            if !findings.iter().any(|existing| same_finding(existing, &finding)) {
                findings.push(finding);
            }
        }
    });
    findings
}

fn collect_exit_findings(
    op: &Operation,
    solver: &DataFlowSolver,
    overlay: &StorageTaintOverlay,
) -> Vec<AdviceTaintExitFinding> {
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
            let taint = value_taint(value, solver, overlay);
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
                    contexts: collect_call_contexts(
                        op,
                        solver,
                        overlay,
                        Some(function_name),
                        origin,
                    ),
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
    overlay: &StorageTaintOverlay,
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
            call.arguments().iter().zip(param_types.into_iter()).enumerate()
        {
            if !is_constrained_external_parameter_type(&parameter_type) {
                continue;
            }

            let taint = value_taint(argument.borrow().as_value_ref(), solver, overlay);
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
    overlay: &StorageTaintOverlay,
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
            && call_results_contain_origin(operation, solver, overlay, origin)
        {
            push_context(&mut contexts, operation.span(), AdviceTaintContextKind::CallResult);
        }

        if callee_function == Some(use_function)
            && call_arguments_contain_origin(call, solver, overlay, origin)
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
    overlay: &StorageTaintOverlay,
    origin: AdviceTaintOrigin,
) -> bool {
    call.results().all().iter().any(|result| {
        let value = result.borrow().as_value_ref();
        value_taint(value, solver, overlay).contains_origin(origin)
    })
}

fn call_arguments_contain_origin(
    call: &dyn CallOpInterface,
    solver: &DataFlowSolver,
    overlay: &StorageTaintOverlay,
    origin: AdviceTaintOrigin,
) -> bool {
    call.arguments().iter().any(|operand| {
        let value = operand.borrow().as_value_ref();
        value_taint(value, solver, overlay).contains_origin(origin)
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
