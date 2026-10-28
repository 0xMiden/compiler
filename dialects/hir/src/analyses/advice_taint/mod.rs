mod diagnostics;
mod lattice;
mod layout;
mod propagation;
mod ranges;
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
    ranges::RangeAnalysisStatus,
};
use self::{
    lattice::value_taint,
    ranges::RangeFacts,
    sinks::{external_call_param_types, external_parameter_range_constraint, is_external_call},
    storage::AdviceTaintStoragePropagation,
};

/// Analysis wrapper that stabilizes integer ranges before propagating taint and diagnostics.
///
/// The worklist budget applies separately to ranges and taint. Exhausting the range budget
/// retains the static operand requirements; taint still receives its full budget.
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
    ranges: RangeFacts,
    findings: Vec<AdviceTaintFinding>,
    exit_findings: Vec<AdviceTaintExitFinding>,
    external_call_findings: Vec<AdviceTaintExternalCallFinding>,
}

pub struct AdviceTaintAnalysisResult {
    pub analysis: AdviceTaintAnalysis,
    pub incomplete_reason: Option<String>,
}

impl AdviceTaintAnalysis {
    /// Status of optional range inference, independent of taint coverage.
    pub fn range_analysis_status(&self) -> &RangeAnalysisStatus {
        self.ranges.status()
    }

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
        self.collect_analysis_results(op);
        Ok(())
    }

    fn run_solver_with_config(
        &mut self,
        op: &Operation,
        analysis_manager: AnalysisManager,
        config: DataFlowConfig,
    ) -> Result<(), Report> {
        self.ranges = RangeFacts::analyze(op, analysis_manager.clone(), config.clone())?;
        self.solver = DataFlowSolver::new(config);
        let propagation =
            AdviceTaintPropagation::with_ranges(&mut self.solver, self.ranges.clone());
        self.solver.load_with_strategy(propagation);
        self.solver.load::<AdviceTaintStoragePropagation>();
        self.solver.initialize_and_run(op, analysis_manager)
    }

    fn collect_analysis_results(&mut self, op: &Operation) {
        self.findings = collect_findings(op, &self.solver, &self.ranges);
        self.exit_findings = collect_exit_findings(op, &self.solver);
        self.external_call_findings =
            collect_external_call_findings(op, &self.solver, &self.ranges);
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

fn collect_findings(
    op: &Operation,
    solver: &DataFlowSolver,
    ranges: &RangeFacts,
) -> Vec<AdviceTaintFinding> {
    let mut findings = Vec::new();
    op.prewalk_all(|operation| {
        let mut operand_taint = ContextualAdviceTaintValue::clean();
        for operand_index in ranges.unsatisfied_operands(operation) {
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
    ranges: &RangeFacts,
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
            if ranges.satisfies(argument, &constraint) {
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
    fn exhausted_range_phase_retains_static_sink_and_external_call_checks() -> Result<(), Report> {
        use midenc_dialect_arith::ArithOpBuilder;
        use midenc_hir::{
            Type,
            dialects::builtin::{BuiltinOpBuilder, FunctionBuilder, attributes::Signature},
        };
        use midenc_hir_analysis::{DataFlowConfig, DataFlowSolver};

        use super::{AdviceTaintPropagation, ranges::RangeFacts};
        use crate::HirOpBuilder;

        let span = SourceSpan::UNKNOWN;
        let mut test = Test::named("range_budget_fallback").in_module("m");
        let external = test.define_function("external", &[Type::U32], &[]);
        let mut helper = test.define_function("helper", &[Type::U32], &[Type::U32]);
        midenc_hir::Symbol::set_visibility(
            &mut *helper.borrow_mut(),
            midenc_hir::Visibility::Private,
        );
        {
            let mut builder = FunctionBuilder::new(helper, test.builder_mut());
            let arg = builder.entry_block().borrow().arguments()[0].borrow().as_value_ref();
            let two = builder.u32(2, span);
            let product = builder.mul(arg, two, span)?;
            builder.ret([product], span)?;
        }
        let entry = test.define_function("entry", &[], &[Type::U32]);
        {
            let signature = Signature::new(&test.context_rc(), [Type::U32], []);
            let helper_signature = Signature::new(&test.context_rc(), [Type::U32], [Type::U32]);
            let mut builder = FunctionBuilder::new(entry, test.builder_mut());
            let raw = builder.advice_pop(span)?;
            let zero = builder.felt(midenc_hir::Felt::ZERO, span);
            let boolean = builder.eq(raw, zero, span)?;
            let value = builder.unrealized_conversion_cast(boolean, Type::U32, span)?;
            builder.exec(external, signature, [value], span)?;
            builder.exec(helper, helper_signature, [value], span)?;
            let one = builder.u32(1, span);
            let sum = builder.add(value, one, span)?;
            builder.ret([sum], span)?;
        }
        let root = test.module().as_operation_ref();
        let manager = AnalysisManager::new(root, None);
        let mut config = DataFlowConfig::new();
        config.set_interprocedural(true);
        config.set_max_worklist_iterations(Some(0));
        let ranges = RangeFacts::analyze(&root.borrow(), manager.clone(), config.clone())?;
        assert!(matches!(ranges.status(), super::RangeAnalysisStatus::BudgetExhausted { .. }));
        // Run the actual taint phase with a full budget after range inference has exhausted
        // its own. Even initialization-time boolean facts must not suppress static checks.
        config.set_max_worklist_iterations(None);
        let mut solver = DataFlowSolver::new(config.clone());
        let propagation = AdviceTaintPropagation::with_ranges(&mut solver, ranges.clone());
        solver.load_with_strategy(propagation);
        solver.load::<super::storage::AdviceTaintStoragePropagation>();
        solver.initialize_and_run(&root.borrow(), manager)?;
        let mut analysis = AdviceTaintAnalysis {
            solver,
            ranges,
            ..Default::default()
        };
        analysis.collect_analysis_results(&root.borrow());
        assert!(matches!(
            analysis.range_analysis_status(),
            super::RangeAnalysisStatus::BudgetExhausted { .. }
        ));
        assert_eq!(analysis.findings().len(), 2);
        assert_eq!(analysis.external_call_findings().len(), 1);
        let complete = AdviceTaintAnalysis::run_with_config(
            &root.borrow(),
            AnalysisManager::new(root, None),
            config,
        )?;
        assert_eq!(complete.range_analysis_status(), &super::RangeAnalysisStatus::Complete);
        assert!(complete.findings().is_empty());
        assert!(complete.external_call_findings().is_empty());
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
