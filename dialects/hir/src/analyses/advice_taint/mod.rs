mod diagnostics;
mod lattice;
mod layout;
mod propagation;
mod sinks;
mod storage;

use alloc::{
    rc::Rc,
    string::{String, ToString},
    vec::Vec,
};
use core::any::Any;

use midenc_hir::{
    CallOpInterface, Operation, Report, Spanned, Symbol, SymbolName,
    diagnostics::SourceManager,
    dialects::builtin,
    pass::{Analysis, AnalysisManager, PreservedAnalyses},
};
use midenc_hir_analysis::{DataFlowConfig, DataFlowSolver, LatticeLike};

pub use self::{
    diagnostics::{
        AdviceTaintContext, AdviceTaintContextKind, AdviceTaintDiagnostic, AdviceTaintExitFinding,
        AdviceTaintExternalCallFinding, AdviceTaintFinding,
    },
    lattice::{
        AdviceTaintOrigin, AdviceTaintOriginKind, AdviceTaintValue, ContextualAdviceTaintValue,
    },
    propagation::AdviceTaintPropagation,
};
use self::{
    lattice::value_taint,
    sinks::{
        external_call_param_types, external_parameter_range_constraint, is_external_call,
        is_range_constrained_sink, range_constrained_operand_indices,
    },
    storage::AdviceTaintStoragePropagation,
};

/// Analysis wrapper that runs the sparse taint propagation and materializes diagnostics.
#[derive(Default)]
pub struct AdviceTaintAnalysis {
    solver: DataFlowSolver,
    findings: Vec<AdviceTaintFinding>,
    exit_findings: Vec<AdviceTaintExitFinding>,
    external_call_findings: Vec<AdviceTaintExternalCallFinding>,
}

pub struct AdviceTaintAnalysisResult {
    pub analysis: AdviceTaintAnalysis,
    pub incomplete_reason: Option<String>,
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

    pub fn diagnostics(&self, source_manager: &dyn SourceManager) -> Vec<AdviceTaintDiagnostic> {
        diagnostics::visible_advice_findings(&self.findings, source_manager)
            .into_iter()
            .map(|finding| finding.diagnostic(source_manager))
            .chain(self.exit_findings.iter().map(|finding| finding.diagnostic(source_manager)))
            .chain(
                self.external_call_findings
                    .iter()
                    .map(|finding| finding.diagnostic(source_manager)),
            )
            .collect()
    }

    pub fn reports(&self, source_manager: &dyn SourceManager) -> Vec<Report> {
        diagnostics::visible_advice_findings(&self.findings, source_manager)
            .into_iter()
            .map(|finding| finding.into_report(source_manager))
            .chain(self.exit_findings.iter().map(|finding| finding.into_report(source_manager)))
            .chain(
                self.external_call_findings
                    .iter()
                    .map(|finding| finding.into_report(source_manager)),
            )
            .collect()
    }

    pub fn solver(&self) -> &DataFlowSolver {
        &self.solver
    }

    pub fn analyze_with_config(
        &mut self,
        op: &Operation,
        analysis_manager: AnalysisManager,
        config: DataFlowConfig,
    ) -> Result<(), Report> {
        self.run_solver_with_config(op, analysis_manager, config)?;
        self.collect_analysis_results(op);
        Ok(())
    }

    fn run_solver_with_config(
        &mut self,
        op: &Operation,
        analysis_manager: AnalysisManager,
        config: DataFlowConfig,
    ) -> Result<(), Report> {
        self.solver = DataFlowSolver::new(config);
        self.solver.load::<AdviceTaintPropagation>();
        self.solver.load::<AdviceTaintStoragePropagation>();
        self.solver.initialize_and_run(op, analysis_manager)
    }

    fn collect_analysis_results(&mut self, op: &Operation) {
        self.findings = collect_findings(op, &self.solver);
        self.exit_findings = collect_exit_findings(op, &self.solver);
        self.external_call_findings = collect_external_call_findings(op, &self.solver);
    }

    pub fn run_with_config(
        op: &Operation,
        analysis_manager: AnalysisManager,
        config: DataFlowConfig,
    ) -> Result<Self, Report> {
        let mut analysis = Self::default();
        analysis.analyze_with_config(op, analysis_manager, config)?;
        Ok(analysis)
    }

    pub fn run_with_config_allow_partial(
        op: &Operation,
        analysis_manager: AnalysisManager,
        config: DataFlowConfig,
    ) -> Result<AdviceTaintAnalysisResult, Report> {
        let mut analysis = Self::default();
        let incomplete_reason = match analysis.run_solver_with_config(op, analysis_manager, config)
        {
            Ok(()) => None,
            Err(err) if is_dataflow_budget_exhaustion(&err) => Some(err.to_string()),
            Err(err) => return Err(err),
        };
        analysis.collect_analysis_results(op);
        Ok(AdviceTaintAnalysisResult {
            analysis,
            incomplete_reason,
        })
    }
}

fn is_dataflow_budget_exhaustion(err: &Report) -> bool {
    err.to_string().contains("dataflow solver exceeded worklist iteration budget")
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
        self.analyze_with_config(op, analysis_manager, config)
    }

    fn invalidate(&self, _preserved_analyses: &mut PreservedAnalyses) -> bool {
        true
    }
}

fn collect_findings(op: &Operation, solver: &DataFlowSolver) -> Vec<AdviceTaintFinding> {
    let mut findings = Vec::new();
    op.prewalk_all(|operation| {
        if !is_range_constrained_sink(operation) {
            return;
        }

        let mut operand_taint = ContextualAdviceTaintValue::clean();
        for operand_index in range_constrained_operand_indices(operation) {
            let operand = &operation.operands()[operand_index];
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
            let mut contexts = collect_call_contexts(op, solver, function, origin);
            for span in operand_taint.call_context_spans_containing_origin(origin) {
                push_context(&mut contexts, span, AdviceTaintContextKind::CallArgument);
            }
            let finding = AdviceTaintFinding {
                sink: sink.clone(),
                sink_span,
                advice_span: origin.span,
                origin,
                contexts,
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
            if external_parameter_range_constraint(&parameter_type).is_none() {
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

        let callee_function = resolved_callee_function_name(call);

        if callee_function != Some(use_function)
            && call_results_contain_origin(operation, solver, origin)
        {
            push_context(&mut contexts, operation.span(), AdviceTaintContextKind::CallResult);
        }

        if callee_function == Some(use_function)
            && call_arguments_contain_origin(call, solver, origin)
        {
            push_context(&mut contexts, operation.span(), AdviceTaintContextKind::CallArgument);
        }

        if call_arguments_contain_origin(call, solver, origin) {
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
    span: midenc_hir::SourceSpan,
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
        && lhs.origin.kind == rhs.origin.kind
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
