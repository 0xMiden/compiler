mod diagnostics;
mod lattice;
mod layout;
mod propagation;
mod sinks;
mod storage;
#[cfg(test)]
mod test_support;

use alloc::{
    rc::Rc,
    string::{String, ToString},
    vec::Vec,
};
use core::any::Any;

use midenc_hir::{
    CallOpInterface, FxHashMap, Op, Operation, OperationRef, Report, Span, Spanned, Symbol,
    SymbolName, SymbolPath,
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
        range_constrained_operand_indices,
        unsatisfied_range_constrained_operand_indices_after_analysis,
        value_satisfies_range_constraint_after_analysis,
    },
    storage::AdviceTaintStoragePropagation,
};

/// Analysis wrapper that runs the sparse taint propagation and materializes diagnostics.
///
/// # Analysis root
///
/// Run this over a root that contains every callee reachable from the code under analysis — a
/// world or a component, as the compiler does. Rooted lower (a single function, say), a call to
/// a callee outside the root is classified by whether it has a body, not by whether this
/// analysis can see it: `hir_analysis`'s forward analyses then propagate into a callee they
/// never visit, seeding clean state with no known return to join, and findings inside that
/// callee are silently missed. Making that classification scope-aware is tracked separately.
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

    /// Return sink findings selected for user-facing diagnostics.
    ///
    /// Structured renderers share the same selection as `diagnostics` and `reports`.
    /// The `findings` accessor retains all raw solver findings.
    pub fn diagnostic_findings(
        &self,
        source_manager: &dyn SourceManager,
    ) -> Vec<&AdviceTaintFinding> {
        diagnostics::visible_advice_findings(&self.findings, source_manager)
    }

    pub fn diagnostics(&self, source_manager: &dyn SourceManager) -> Vec<AdviceTaintDiagnostic> {
        self.diagnostic_findings(source_manager)
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
        self.diagnostic_findings(source_manager)
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
        self.collect_analysis_results(op, true);
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

    fn collect_analysis_results(&mut self, op: &Operation, ranges_complete: bool) {
        self.findings = collect_findings(op, &self.solver, ranges_complete);
        self.exit_findings = collect_exit_findings(op, &self.solver);
        self.external_call_findings =
            collect_external_call_findings(op, &self.solver, ranges_complete);
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
        analysis.collect_analysis_results(op, incomplete_reason.is_none());
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

fn collect_findings(
    op: &Operation,
    solver: &DataFlowSolver,
    ranges_complete: bool,
) -> Vec<AdviceTaintFinding> {
    let mut findings = Vec::new();
    op.prewalk_all(|operation| {
        // Before convergence, a proven range may still omit predecessors. Only
        // complete range facts can discharge a sink's runtime requirements.
        let operand_indices = if ranges_complete {
            unsatisfied_range_constrained_operand_indices_after_analysis(operation, solver)
        } else {
            range_constrained_operand_indices(operation)
        };
        if operand_indices.is_empty() {
            return;
        }

        let mut operand_taint = ContextualAdviceTaintValue::clean();
        for operand_index in operand_indices {
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

fn collect_public_exposure(op: &Operation) -> FxHashMap<OperationRef, Vec<Span<SymbolPath>>> {
    let mut exposed = FxHashMap::default();
    op.prewalk_all(|operation| {
        if let Some(function) = operation.downcast_ref::<builtin::Function>() {
            if Symbol::is_public(function) {
                exposed.entry(operation.as_operation_ref()).or_insert_with(Vec::new);
            }
        } else if let Some(alias) = operation.downcast_ref::<builtin::FunctionAlias>()
            && Symbol::is_public(alias)
            && let Some(target) =
                operation.as_symbol_ref().and_then(|symbol| symbol.resolve_function().ok())
        {
            let target = target.borrow();
            if !Symbol::is_public(&*target) {
                exposed
                    .entry(target.as_operation_ref())
                    .or_insert_with(Vec::new)
                    .push(Span::new(alias.span(), alias.path()));
            }
        }
    });
    exposed
}

fn collect_exit_findings(op: &Operation, solver: &DataFlowSolver) -> Vec<AdviceTaintExitFinding> {
    let exposed = collect_public_exposure(op);
    let mut findings = Vec::new();
    op.prewalk_all(|operation| {
        let Some(ret) = operation.downcast_ref::<builtin::Ret>() else {
            return;
        };
        let Some(function_ref) = operation.nearest_parent_op::<builtin::Function>() else {
            return;
        };
        let function = function_ref.borrow();
        let Some(public_aliases) = exposed.get(&function.as_operation_ref()) else {
            return;
        };

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
                    public_aliases: public_aliases.clone(),
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
    ranges_complete: bool,
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
            let Some(constraint) = external_parameter_range_constraint(&parameter_type) else {
                continue;
            };

            let argument = argument.borrow().as_value_ref();
            if ranges_complete
                && value_satisfies_range_constraint_after_analysis(argument, &constraint, solver)
            {
                continue;
            }

            let taint = value_taint(argument, solver);
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
    let function = call.resolve()?.as_function()?;
    Some(Symbol::name(&*function.borrow()))
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

#[cfg(test)]
mod tests {
    use alloc::{format, string::ToString, vec, vec::Vec};

    use midenc_hir::{
        Context, OperationName, Report, SourceSpan,
        diagnostics::{SourceLanguage, Uri},
        dialects::builtin,
        interner::Symbol,
        parse::{ParserConfig, parse},
        pass::AnalysisManager,
        testing::Test,
    };

    use super::{
        AdviceTaintAnalysis, AdviceTaintContext, AdviceTaintContextKind, AdviceTaintDiagnostic,
        AdviceTaintExitFinding, AdviceTaintFinding, AdviceTaintOrigin,
    };

    type ExitResults = (Vec<AdviceTaintExitFinding>, Vec<AdviceTaintDiagnostic>);

    #[test]
    fn regression_partial_range_facts_do_not_suppress_findings() -> Result<(), Report> {
        use midenc_dialect_arith::ArithOpBuilder;
        use midenc_hir::{
            Immediate, IntegerValueRange, Type,
            dialects::builtin::{BuiltinOpBuilder, FunctionBuilder, attributes::Signature},
        };
        use midenc_hir_analysis::{SparseLattice, analyses::integer_range::IntegerRangeLattice};

        use super::lattice::AdviceTaintSparseLattice;
        use crate::HirOpBuilder;

        let span = SourceSpan::UNKNOWN;
        let mut test = Test::named("partial_range_findings").in_module("m");
        let external = test.define_function("external", &[Type::U32], &[]);
        let entry = test.define_function("entry", &[], &[Type::U32]);
        let unchecked = {
            let signature = Signature::new(&test.context_rc(), [Type::U32], []);
            let mut builder = FunctionBuilder::new(entry, test.builder_mut());
            let raw = builder.advice_pop(span)?;
            let unchecked = builder.unrealized_conversion_cast(raw, Type::U32, span)?;
            builder.exec(external, signature, [unchecked], span)?;
            let one = builder.u32(1, span);
            let sum = builder.add(unchecked, one, span)?;
            builder.ret([sum], span)?;
            unchecked
        };
        let mut analysis = AdviceTaintAnalysis::default();
        // Model a partial join: known predecessors fit u32, but an unchecked advice
        // origin has arrived and other predecessors may still widen the range.
        analysis
            .solver
            .get_or_create_mut::<IntegerRangeLattice, _>(unchecked)
            .join(&IntegerValueRange::constant(Immediate::U32(7)));
        analysis
            .solver
            .get_or_create_mut::<AdviceTaintSparseLattice, _>(unchecked)
            .join(&super::ContextualAdviceTaintValue::raw(span));
        let root = test.module().as_operation_ref();
        analysis.collect_analysis_results(&root.borrow(), false);
        assert_eq!(analysis.findings().len(), 1);
        assert_eq!(analysis.external_call_findings().len(), 1);
        analysis.collect_analysis_results(&root.borrow(), true);
        assert!(analysis.findings().is_empty());
        assert!(analysis.external_call_findings().is_empty());
        Ok(())
    }

    fn analyze_source(source: &str) -> Result<ExitResults, Report> {
        let test = Test::default();
        test.context().get_or_register_dialect::<crate::HirDialect>();
        let module = parse::<builtin::Module>(
            ParserConfig::new(test.context_rc()),
            Uri::new("advice_alias_exit.hir"),
            source,
        )?;
        let manager = AnalysisManager::new(module.as_operation_ref(), None);
        let analysis = manager.get_analysis::<AdviceTaintAnalysis>()?;
        let findings = analysis.exit_findings().to_vec();
        let diagnostics = findings
            .iter()
            .map(|finding| finding.diagnostic(test.context().session().source_manager.as_ref()))
            .collect();
        Ok((findings, diagnostics))
    }

    fn analyze_aliases(visibility: &str, aliases: &str) -> Result<ExitResults, Report> {
        analyze_source(&format!(
            r#"
    builtin.module public @test {{
        builtin.function {visibility} extern("C") @body() -> felt {{
            %value = hir.advice_pop;
            builtin.ret %value : (felt);
        }};
        {aliases}
    }};
    "#
        ))
    }

    fn assert_alias_exit(aliases: &str) -> Result<(), Report> {
        let (findings, diagnostics) = analyze_aliases("internal", aliases)?;
        assert_eq!(findings.len(), 1, "{findings:#?}");
        let finding = &findings[0];
        assert_eq!(finding.function.as_str(), "body");
        assert_eq!(finding.result_index, 0);
        assert_eq!(finding.public_aliases.len(), 1);
        assert_eq!(finding.public_aliases[0].inner().to_string(), "::test::api");
        assert!(!finding.public_aliases[0].span().is_unknown());
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(
            diagnostics[0].message(),
            "function 'body', exposed through public alias '::test::api', returns unconstrained \
             advice value as result #0"
        );
        assert_eq!(
            diagnostics[0].label_messages().collect::<Vec<_>>(),
            [
                "publicly exposed function returns unconstrained advice via result #0",
                "unconstrained advice originates here",
                "public alias '::test::api' exposes 'body' to callers",
            ]
        );
        Ok(())
    }

    #[test]
    fn public_alias_reports_internal_function_exit() -> Result<(), Report> {
        assert_alias_exit("builtin.function_alias public @api -> @body;")
    }

    #[test]
    fn public_alias_chain_reports_internal_function_exit() -> Result<(), Report> {
        assert_alias_exit(
            r#"
        builtin.function_alias public @api -> @forward;
        builtin.function_alias private @forward -> @body;
    "#,
        )
    }

    #[test]
    fn multiple_public_aliases_share_one_exit_finding() -> Result<(), Report> {
        let (findings, diagnostics) = analyze_aliases(
            "internal",
            r#"
        builtin.module public @a {
            builtin.function_alias public @api -> ::@test::@body;
        };
        builtin.module public @b {
            builtin.function_alias public @api -> ::@test::@body;
        };
    "#,
        )?;
        assert_eq!(findings.len(), 1, "{findings:#?}");
        assert_eq!(findings[0].public_aliases.len(), 2);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(
            diagnostics[0].message(),
            "function 'body', exposed through public aliases '::test::a::api', '::test::b::api', \
             returns unconstrained advice value as result #0"
        );
        let labels = diagnostics[0].label_messages().collect::<Vec<_>>();
        assert!(labels.contains(&"public alias '::test::a::api' exposes 'body' to callers"));
        assert!(labels.contains(&"public alias '::test::b::api' exposes 'body' to callers"));
        Ok(())
    }

    #[test]
    fn internal_function_without_public_exposure_has_no_exit_finding() -> Result<(), Report> {
        for aliases in [
            "",
            "builtin.function_alias private @api -> @body;",
            "builtin.function_alias internal @api -> @body;",
        ] {
            let (findings, diagnostics) = analyze_aliases("internal", aliases)?;
            assert!(findings.is_empty(), "{aliases}: {findings:#?}");
            assert!(diagnostics.is_empty());
        }
        Ok(())
    }

    #[test]
    fn directly_public_function_keeps_existing_exit_diagnostic() -> Result<(), Report> {
        for aliases in [
            "",
            "builtin.function_alias public @api -> @body;",
            "builtin.function_alias public @api -> @forward;
             builtin.function_alias private @forward -> @body;",
        ] {
            let (findings, diagnostics) = analyze_aliases("public", aliases)?;
            assert_eq!(findings.len(), 1, "{findings:#?}");
            assert!(findings[0].public_aliases.is_empty());
            assert_eq!(diagnostics.len(), 1);
            assert_eq!(
                diagnostics[0].message(),
                "public function 'body' returns unconstrained advice value as result #0"
            );
            assert_eq!(
                diagnostics[0].label_messages().collect::<Vec<_>>(),
                [
                    "public function returns unconstrained advice via result #0",
                    "unconstrained advice originates here",
                ]
            );
        }
        Ok(())
    }

    #[test]
    fn public_alias_reports_unconstrained_external_result_exit() -> Result<(), Report> {
        let (findings, diagnostics) = analyze_source(
            r#"
    builtin.module public @test {
        builtin.function internal extern("C") @body(%r0: felt, %r1: felt, %r2: felt, %r3: felt) -> felt {
            %value = hir.dyncall [%r0, %r1, %r2, %r3]() : extern("component-model") () -> felt;
            builtin.ret %value : (felt);
        };
        builtin.function_alias public @api -> @body;
    };
    "#,
        )?;
        assert_eq!(findings.len(), 1, "{findings:#?}");
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(
            diagnostics[0].message(),
            "function 'body', exposed through public alias '::test::api', returns unconstrained \
             external call result as result #0"
        );
        let labels = diagnostics[0].label_messages().collect::<Vec<_>>();
        assert!(labels.contains(
            &"publicly exposed function returns an unconstrained external call result via result \
              #0"
        ));
        assert!(labels.contains(&"public alias '::test::api' exposes 'body' to callers"));
        Ok(())
    }

    #[test]
    fn diagnostic_selection_prefers_user_source_without_changing_raw_findings() {
        let context = Context::default();
        let sources = context.source_manager();
        let user = sources.load(
            SourceLanguage::Rust,
            "/project/main.rs".into(),
            "let value = 0;\n".into(),
        );
        let internal = sources.load(
            SourceLanguage::Rust,
            "/compiler/sdk/internal.rs".into(),
            "let value = 0;\n".into(),
        );
        let user_span = SourceSpan::new(user.id(), 0..1);
        let internal_span = SourceSpan::new(internal.id(), 0..1);
        let contexts = vec![AdviceTaintContext {
            span: SourceSpan::new(user.id(), 4..5),
            kind: AdviceTaintContextKind::CallArgument,
        }];
        let user_finding = AdviceTaintFinding {
            sink: OperationName::new::<midenc_dialect_arith::Add>(Symbol::intern("arith"), vec![]),
            sink_span: user_span,
            advice_span: user_span,
            origin: AdviceTaintOrigin::advice(user_span),
            contexts: contexts.clone(),
            function: None,
        };
        let internal_finding = AdviceTaintFinding {
            sink: OperationName::new::<crate::ops::IntToPtr>(Symbol::intern("hir"), vec![]),
            sink_span: internal_span,
            ..user_finding.clone()
        };
        for findings in [
            vec![internal_finding.clone(), user_finding.clone()],
            vec![user_finding, internal_finding],
        ] {
            let analysis = AdviceTaintAnalysis {
                findings,
                ..Default::default()
            };
            let selected = analysis.diagnostic_findings(sources.as_ref());
            assert_eq!(selected.len(), 1);
            assert_eq!(selected[0].sink.to_string(), "arith.add");
            assert_eq!(selected[0].sink_span, user_span);
            assert_eq!(selected[0].contexts, contexts);
            assert_eq!(analysis.findings().len(), 2);
            assert_eq!(analysis.diagnostics(sources.as_ref()).len(), 1);
            assert_eq!(analysis.reports(sources.as_ref()).len(), 1);
        }
    }
}
