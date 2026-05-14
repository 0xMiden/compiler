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
        let function = finding
            .function
            .map(|name| format!(" in function '{}'", name.as_str()))
            .unwrap_or_default();

        let (subject, sink_label, origin_label, help) = match finding.origin.kind {
            AdviceTaintOriginKind::Advice => (
                "unconstrained advice value",
                "unconstrained advice data is consumed here as a u32",
                "advice data is obtained here which is later used unconstrained",
                "add an explicit u32 range check, such as MASM's `u32assert`, before this value \
                 is consumed by a u32-presuming operation"
                    .to_string(),
            ),
            AdviceTaintOriginKind::ExternalCall => (
                "unconstrained external call result",
                "unconstrained advice from an external call is consumed here as a u32",
                "the result of the external call here is tainted as unconstrained",
                "add an explicit u32 range check after the call, or provide an analyzable callee \
                 body/summary proving the result is constrained before this u32-presuming \
                 operation"
                    .to_string(),
            ),
        };
        let sink_source = source_manager.get(finding.sink_span.source_id()).ok();
        let message = format!("{subject} reaches u32-presuming operation {}", function);
        let label =
            LabeledSpan::new_primary_with_span(Some(sink_label.to_string()), finding.sink_span);
        let mut labels = vec![label];
        let mut related = vec![];
        context_labels(
            finding.sink_span,
            &finding.contexts,
            &mut labels,
            &mut related,
            source_manager,
        );
        let source_source = source_manager.get(finding.origin.span.source_id()).ok();
        if finding.origin.span.source_id() == finding.advice_span.source_id() {
            labels.push(LabeledSpan::new_with_span(
                Some(origin_label.to_string()),
                finding.origin.span,
            ));
        } else {
            related.push(
                RelatedLabel::advice("unconstrained advice")
                    .with_labeled_span(finding.origin.span, origin_label)
                    .with_source_file(source_source),
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
        context_labels(
            finding.function_span,
            &finding.contexts,
            &mut labels,
            &mut related,
            source_manager,
        );
        if finding.origin.span.source_id() == finding.advice_span.source_id() {
            labels.push(LabeledSpan::new_with_span(
                Some(origin_label.to_string()),
                finding.origin.span,
            ));
        } else {
            let source_source = source_manager.get(finding.origin.span.source_id()).ok();
            related.push(
                RelatedLabel::advice("unconstrained advice")
                    .with_labeled_span(finding.origin.span, origin_label)
                    .with_source_file(source_source),
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
    labels: &mut Vec<LabeledSpan>,
    related: &mut Vec<RelatedLabel>,
    source_manager: &dyn SourceManager,
) {
    for context in contexts {
        let label = match context.kind {
            AdviceTaintContextKind::CallArgument => {
                "unconstrained value is passed as a call argument here"
            }
            AdviceTaintContextKind::CallResult => "unconstrained value returns from a call here",
        };
        if sink_span.source_id() == context.span.source_id() {
            labels.push(LabeledSpan::new_with_span(Some(label.to_string()), context.span));
        } else {
            related.push(
                RelatedLabel::advice("relevant context for unconstrained advice")
                    .with_labeled_span(context.span, label)
                    .with_source_file(source_manager.get(context.span.source_id()).ok()),
            );
        }
    }
}
