use alloc::{
    format,
    string::{String, ToString},
    vec,
    vec::Vec,
};

use midenc_hir::{
    OperationName, Report, SourceSpan, SymbolName, Type,
    diagnostics::{Diagnostic, LabeledSpan, miette},
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
