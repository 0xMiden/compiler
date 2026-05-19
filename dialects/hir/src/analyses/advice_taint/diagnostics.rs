use alloc::{
    format,
    string::{String, ToString},
    sync::Arc,
    vec,
    vec::Vec,
};

use midenc_hir::{
    OperationName, Report, SourceSpan, SymbolName, Type,
    diagnostics::{Diagnostic, LabeledSpan, RelatedLabel, SourceFile, SourceManager, miette},
};

use super::lattice::{AdviceTaintOrigin, AdviceTaintOriginKind};

/// The first unsafe range-constrained use of raw advice data.
#[derive(Debug, Clone)]
pub struct AdviceTaintFinding {
    /// The operation that consumed raw advice as a range-constrained value.
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
    pub fn diagnostic(&self, source_manager: &dyn SourceManager) -> AdviceTaintDiagnostic {
        AdviceTaintDiagnostic::new(self, source_manager)
    }

    pub fn into_report(&self, source_manager: &dyn SourceManager) -> Report {
        self.diagnostic(source_manager).into_report()
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
    pub fn diagnostic(&self, source_manager: &dyn SourceManager) -> AdviceTaintDiagnostic {
        AdviceTaintDiagnostic::new_exit(self, source_manager)
    }

    pub fn into_report(&self, source_manager: &dyn SourceManager) -> Report {
        self.diagnostic(source_manager).into_report()
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
    pub fn diagnostic(&self, source_manager: &dyn SourceManager) -> AdviceTaintDiagnostic {
        AdviceTaintDiagnostic::new_external_call(self, source_manager)
    }

    pub fn into_report(&self, source_manager: &dyn SourceManager) -> Report {
        self.diagnostic(source_manager).into_report()
    }
}

/// Return the advice-taint findings that should be rendered as user-facing diagnostics.
///
/// Interprocedural Rust lowering can produce both a useful user-source finding and a lower-quality
/// internal finding for the same origin along the same call path. Keep the more actionable finding
/// for diagnostics, but leave the raw analysis findings unchanged for debugging and tests that need
/// full solver visibility.
pub(super) fn visible_advice_findings<'a>(
    findings: &'a [AdviceTaintFinding],
    source_manager: &dyn SourceManager,
) -> Vec<&'a AdviceTaintFinding> {
    let mut visible = Vec::<&AdviceTaintFinding>::new();
    for finding in findings {
        if let Some(existing_index) = visible
            .iter()
            .position(|existing| same_user_visible_path(existing, finding, source_manager))
        {
            if is_more_actionable_finding(finding, visible[existing_index], source_manager) {
                visible[existing_index] = finding;
            }
        } else {
            visible.push(finding);
        }
    }
    visible
}

fn same_user_visible_path(
    lhs: &AdviceTaintFinding,
    rhs: &AdviceTaintFinding,
    source_manager: &dyn SourceManager,
) -> bool {
    lhs.origin == rhs.origin
        && !lhs.contexts.is_empty()
        && same_contexts(&lhs.contexts, &rhs.contexts)
        && (is_low_quality_span(lhs.sink_span, source_manager)
            || is_low_quality_span(rhs.sink_span, source_manager))
}

fn same_contexts(lhs: &[AdviceTaintContext], rhs: &[AdviceTaintContext]) -> bool {
    lhs.len() == rhs.len() && lhs.iter().all(|context| rhs.contains(context))
}

fn is_more_actionable_finding(
    candidate: &AdviceTaintFinding,
    current: &AdviceTaintFinding,
    source_manager: &dyn SourceManager,
) -> bool {
    finding_quality(candidate, source_manager) > finding_quality(current, source_manager)
}

fn finding_quality(
    finding: &AdviceTaintFinding,
    source_manager: &dyn SourceManager,
) -> (bool, bool) {
    (
        !is_low_quality_span(finding.sink_span, source_manager),
        finding.function.is_some(),
    )
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
    #[source_code]
    sink_source: Option<Arc<SourceFile>>,
    #[label(collection)]
    labels: Vec<LabeledSpan>,
    #[related]
    related: Vec<RelatedLabel>,
}

impl AdviceTaintDiagnostic {
    fn new(finding: &AdviceTaintFinding, source_manager: &dyn SourceManager) -> Self {
        let function_suffix = finding
            .function
            .map(|name| format!(" in function '{}'", name.as_str()))
            .unwrap_or_default();

        let (subject, sink_label, origin_label, help) = match finding.origin.kind {
            AdviceTaintOriginKind::Advice => (
                "unconstrained advice value",
                "unconstrained advice data is consumed here as a constrained value",
                "advice data is obtained here which is later used unconstrained",
                "add an explicit range check or checked cast before this value is consumed by an \
                 operation that requires a constrained value"
                    .to_string(),
            ),
            AdviceTaintOriginKind::ExternalCall => (
                "unconstrained external call result",
                "unconstrained advice from an external call is consumed here as a constrained \
                 value",
                "the result of the external call here is tainted as unconstrained",
                "add an explicit range check after the call, or provide an analyzable callee \
                 body/summary proving the result is constrained before this operation"
                    .to_string(),
            ),
        };
        let primary_context =
            best_primary_context(finding.sink_span, &finding.contexts, source_manager);
        let function_suffix = if primary_context.is_some() {
            String::new()
        } else {
            function_suffix
        };
        let (primary_span, primary_label) = primary_context
            .map(|context| {
                (
                    context.span,
                    primary_context_label(context.kind, finding.origin.kind).to_string(),
                )
            })
            .unwrap_or_else(|| (finding.sink_span, sink_label.to_string()));
        let sink_source = source_manager.get(primary_span.source_id()).ok();
        let message =
            format!("{subject} reaches operation requiring a constrained value{function_suffix}");
        let mut labels =
            vec![LabeledSpan::new_primary_with_span(Some(primary_label), primary_span)];
        let mut related = vec![];
        if let Some(primary_context) = primary_context {
            promoted_primary_context_label(
                primary_span,
                primary_context,
                &finding.contexts,
                &mut labels,
                &mut related,
                source_manager,
            );
        } else {
            context_labels(
                primary_span,
                &finding.contexts,
                None,
                &mut labels,
                &mut related,
                source_manager,
            );
        }
        if should_show_secondary_span(finding.origin.span, primary_span, source_manager) {
            push_label_or_related(
                primary_span,
                finding.origin.span,
                origin_label,
                "unconstrained advice",
                &mut labels,
                &mut related,
                source_manager,
            );
        }
        Self {
            message,
            help,
            sink_source,
            labels,
            related,
        }
    }

    fn new_exit(finding: &AdviceTaintExitFinding, source_manager: &dyn SourceManager) -> Self {
        let (subject, return_label, origin_label, help) = match finding.origin.kind {
            AdviceTaintOriginKind::Advice => (
                "unconstrained advice value",
                format!(
                    "public function returns unconstrained advice via result #{}",
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
                    "public function returns an unconstrained external call result via result #{}",
                    finding.result_index
                ),
                "external call result is modeled as unconstrained here".to_string(),
                "add an explicit constraint before returning the external result, or provide an \
                 analyzable callee body/summary proving the result is constrained"
                    .to_string(),
            ),
        };
        let sink_source = source_manager.get(finding.function_span.source_id()).ok();
        let message = format!(
            "public function '{}' returns {subject} as result #{}",
            finding.function.as_str(),
            finding.result_index
        );
        let label = LabeledSpan::new_primary_with_span(Some(return_label), finding.return_span);
        let mut labels = vec![label];
        let mut related = vec![];
        if should_show_secondary_span(finding.origin.span, finding.function_span, source_manager) {
            push_label_or_related(
                finding.function_span,
                finding.origin.span,
                &origin_label,
                "unconstrained advice",
                &mut labels,
                &mut related,
                source_manager,
            );
        }
        context_labels(
            finding.function_span,
            &finding.contexts,
            None,
            &mut labels,
            &mut related,
            source_manager,
        );

        Self {
            message,
            help,
            sink_source,
            labels,
            related,
        }
    }

    fn new_external_call(
        finding: &AdviceTaintExternalCallFinding,
        source_manager: &dyn SourceManager,
    ) -> Self {
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
        let sink_source = source_manager.get(finding.call_span.source_id()).ok();
        let message = format!(
            "{subject} is passed to external parameter #{} of type `{}`{}",
            finding.argument_index, finding.parameter_type, function
        );
        let label = LabeledSpan::new_primary_with_span(
            Some(format!(
                "an unconstrained value is passed to external parameter #{} typed `{}`",
                finding.argument_index, finding.parameter_type
            )),
            finding.call_span,
        );
        let mut labels = vec![label];
        let mut related = vec![];
        if finding.call_span.source_id() == finding.origin.span.source_id() {
            labels.push(LabeledSpan::new_with_span(Some(origin_label), finding.origin.span));
        } else {
            let source_source = source_manager.get(finding.origin.span.source_id()).ok();
            related.push(
                RelatedLabel::advice("unconstrained advice")
                    .with_labeled_span(finding.origin.span, origin_label)
                    .with_source_file(source_source),
            );
        }
        let help = "add an explicit constraint before passing this value to the external callee, \
                    or provide an analyzable callee body/summary proving the parameter is handled \
                    safely"
            .to_string();

        Self {
            message,
            help,
            sink_source,
            labels,
            related,
        }
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    pub fn help_message(&self) -> &str {
        &self.help
    }

    pub fn label_messages(&self) -> impl Iterator<Item = &str> {
        self.labels
            .iter()
            .map(|label| label.label())
            .chain(
                self.related
                    .iter()
                    .flat_map(|related| related.labels.iter().map(|label| label.label())),
            )
            .flatten()
    }

    pub fn into_report(self) -> Report {
        Report::from(self)
    }
}

fn context_labels(
    sink_span: SourceSpan,
    contexts: &[AdviceTaintContext],
    skip_span: Option<SourceSpan>,
    labels: &mut Vec<LabeledSpan>,
    related: &mut Vec<RelatedLabel>,
    source_manager: &dyn SourceManager,
) {
    for context in contexts {
        if Some(context.span) == skip_span
            || !should_show_secondary_span(context.span, sink_span, source_manager)
        {
            continue;
        }
        push_context_label(sink_span, context, labels, related, source_manager);
    }
}

fn promoted_primary_context_label(
    primary_span: SourceSpan,
    primary_context: &AdviceTaintContext,
    contexts: &[AdviceTaintContext],
    labels: &mut Vec<LabeledSpan>,
    related: &mut Vec<RelatedLabel>,
    source_manager: &dyn SourceManager,
) {
    let wanted_kind = match primary_context.kind {
        AdviceTaintContextKind::CallArgument => AdviceTaintContextKind::CallResult,
        AdviceTaintContextKind::CallResult => AdviceTaintContextKind::CallArgument,
    };
    let selected = contexts
        .iter()
        .filter(|context| {
            context.kind == wanted_kind
                && context.span != primary_span
                && should_show_secondary_span(context.span, primary_span, source_manager)
        })
        .max_by_key(|context| {
            if context.span.source_id() == primary_span.source_id()
                && context.span.start().to_u32() <= primary_span.start().to_u32()
            {
                context.span.start().to_u32()
            } else {
                0
            }
        });

    if let Some(context) = selected {
        push_context_label(primary_span, context, labels, related, source_manager);
    }
}

fn push_context_label(
    sink_span: SourceSpan,
    context: &AdviceTaintContext,
    labels: &mut Vec<LabeledSpan>,
    related: &mut Vec<RelatedLabel>,
    source_manager: &dyn SourceManager,
) {
    let label = match context.kind {
        AdviceTaintContextKind::CallArgument => {
            "unconstrained value is passed as a call argument here"
        }
        AdviceTaintContextKind::CallResult => "unconstrained value returns from a call here",
    };
    push_label_or_related(
        sink_span,
        context.span,
        label,
        "relevant context for unconstrained advice",
        labels,
        related,
        source_manager,
    );
}

fn best_primary_context<'a>(
    sink_span: SourceSpan,
    contexts: &'a [AdviceTaintContext],
    source_manager: &dyn SourceManager,
) -> Option<&'a AdviceTaintContext> {
    if !is_low_quality_span(sink_span, source_manager) {
        return None;
    }

    contexts
        .iter()
        .rev()
        .find(|context| !is_low_quality_span(context.span, source_manager))
}

fn primary_context_label(
    kind: AdviceTaintContextKind,
    origin_kind: AdviceTaintOriginKind,
) -> &'static str {
    match (kind, origin_kind) {
        (AdviceTaintContextKind::CallArgument, AdviceTaintOriginKind::Advice) => {
            "unconstrained advice value is passed here before reaching a constrained operation"
        }
        (AdviceTaintContextKind::CallArgument, AdviceTaintOriginKind::ExternalCall) => {
            "unconstrained advice from an external call is passed here before reaching a \
             constrained operation"
        }
        (AdviceTaintContextKind::CallResult, AdviceTaintOriginKind::Advice) => {
            "unconstrained advice value returns from this call before reaching a constrained \
             operation"
        }
        (AdviceTaintContextKind::CallResult, AdviceTaintOriginKind::ExternalCall) => {
            "unconstrained advice from an external call returns here before reaching a constrained \
             operation"
        }
    }
}

fn should_show_secondary_span(
    span: SourceSpan,
    primary_span: SourceSpan,
    source_manager: &dyn SourceManager,
) -> bool {
    span.source_id() == primary_span.source_id() || !is_low_quality_span(span, source_manager)
}

fn push_label_or_related(
    primary_span: SourceSpan,
    span: SourceSpan,
    label: &str,
    related_title: &'static str,
    labels: &mut Vec<LabeledSpan>,
    related: &mut Vec<RelatedLabel>,
    source_manager: &dyn SourceManager,
) {
    if primary_span.source_id() == span.source_id() {
        labels.push(LabeledSpan::new_with_span(Some(label.to_string()), span));
    } else {
        related.push(
            RelatedLabel::advice(related_title)
                .with_labeled_span(span, label.to_string())
                .with_source_file(source_manager.get(span.source_id()).ok()),
        );
    }
}

fn is_low_quality_span(span: SourceSpan, source_manager: &dyn SourceManager) -> bool {
    if span.is_unknown() || span.is_synthetic() {
        return true;
    }

    let Ok(location) = source_manager.file_line_col(span) else {
        return true;
    };
    let path = location.uri().path();

    path.contains("/rust/library/")
        || path.contains("/.cargo/registry/")
        || path.contains("/registry/src/")
        || path.contains("/compiler/sdk/")
}
