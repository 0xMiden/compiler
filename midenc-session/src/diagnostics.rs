use alloc::{
    boxed::Box,
    collections::BTreeMap,
    fmt::{self, Display},
    format,
    string::{String, ToString},
    sync::Arc,
    vec::Vec,
};
use core::sync::atomic::{AtomicUsize, Ordering};

pub use miden_assembly::diagnostics::{
    miette,
    miette::MietteDiagnostic as AdHocDiagnostic,
    reporting,
    reporting::{PrintDiagnostic, ReportHandlerOpts},
    Diagnostic, Label, LabeledSpan, RelatedError, RelatedLabel, Report, Severity, WrapErr,
};
pub use miden_core::debuginfo::*;
pub use midenc_hir_macros::Spanned;

#[cfg(feature = "std")]
pub use crate::emitter::CaptureEmitter;
pub use crate::emitter::{Buffer, DefaultEmitter, Emitter, NullEmitter};
use crate::{ColorChoice, Verbosity, Warnings};

#[derive(Default, Debug, Copy, Clone)]
pub struct DiagnosticsConfig {
    pub verbosity: Verbosity,
    pub warnings: Warnings,
}

pub struct DiagnosticsHandler {
    emitter: Arc<dyn Emitter>,
    source_manager: Arc<dyn SourceManager + Send + Sync>,
    err_count: AtomicUsize,
    verbosity: Verbosity,
    warnings: Warnings,
    silent: bool,
}

impl Default for DiagnosticsHandler {
    fn default() -> Self {
        let emitter = Arc::new(DefaultEmitter::new(ColorChoice::Auto));
        let source_manager =
            Arc::new(DefaultSourceManager::default()) as Arc<dyn SourceManager + Send + Sync>;
        Self::new(Default::default(), source_manager, emitter)
    }
}

// We can safely implement these traits for DiagnosticsHandler,
// as the only two non-atomic fields are read-only after creation
unsafe impl Send for DiagnosticsHandler {}
unsafe impl Sync for DiagnosticsHandler {}

impl DiagnosticsHandler {
    /// Create a new [DiagnosticsHandler] from the given [DiagnosticsConfig],
    /// [CodeMap], and [Emitter] implementation.
    pub fn new(
        config: DiagnosticsConfig,
        source_manager: Arc<dyn SourceManager + Send + Sync>,
        emitter: Arc<dyn Emitter>,
    ) -> Self {
        let warnings = match config.warnings {
            Warnings::Error => Warnings::Error,
            _ if config.verbosity > Verbosity::Warning => Warnings::None,
            warnings => warnings,
        };
        Self {
            emitter,
            source_manager,
            err_count: AtomicUsize::new(0),
            verbosity: config.verbosity,
            warnings,
            silent: config.verbosity == Verbosity::Silent,
        }
    }

    #[inline]
    pub fn source_manager(&self) -> Arc<dyn SourceManager + Send + Sync> {
        self.source_manager.clone()
    }

    #[inline]
    pub fn source_manager_ref(&self) -> &dyn SourceManager {
        self.source_manager.as_ref()
    }

    /// Returns true if the [DiagnosticsHandler] has emitted any error diagnostics
    pub fn has_errors(&self) -> bool {
        self.err_count.load(Ordering::Relaxed) > 0
    }

    /// Triggers a panic if the [DiagnosticsHandler] has emitted any error diagnostics
    #[track_caller]
    pub fn abort_if_errors(&self) {
        if self.has_errors() {
            panic!("Compiler has encountered unexpected errors. See diagnostics for details.")
        }
    }

    /// Emit a diagnostic [Report]
    pub fn report(&self, report: impl Into<Report>) {
        self.emit(report.into())
    }

    /// Report an error diagnostic
    pub fn error(&self, error: impl ToString) {
        self.emit(Report::msg(error.to_string()));
    }

    /// Report a warning diagnostic
    ///
    /// If `warnings_as_errors` is set, it produces an error diagnostic instead.
    pub fn warn(&self, warning: impl ToString) {
        if matches!(self.warnings, Warnings::Error) {
            return self.error(warning);
        }
        let diagnostic = AdHocDiagnostic::new(warning.to_string()).with_severity(Severity::Warning);
        self.emit(diagnostic);
    }

    /// Emits an informational diagnostic
    pub fn info(&self, message: impl ToString) {
        if self.verbosity > Verbosity::Info {
            return;
        }
        let diagnostic = AdHocDiagnostic::new(message.to_string()).with_severity(Severity::Advice);
        self.emit(diagnostic);
    }

    /// Starts building an [InFlightDiagnostic] for rich compiler diagnostics.
    ///
    /// The caller is responsible for dropping/emitting the diagnostic using the
    /// [InFlightDiagnostic] API.
    pub fn diagnostic(&self, severity: Severity) -> InFlightDiagnosticBuilder<'_> {
        InFlightDiagnosticBuilder::new(self, severity)
    }

    /// Emits the given diagnostic
    #[inline(never)]
    pub fn emit(&self, diagnostic: impl Into<Report>) {
        let diagnostic: Report = diagnostic.into();
        let diagnostic = match diagnostic.severity() {
            Some(Severity::Advice) if self.verbosity > Verbosity::Info => return,
            Some(Severity::Warning) => match self.warnings {
                Warnings::None => return,
                Warnings::All => diagnostic,
                Warnings::Error => {
                    self.err_count.fetch_add(1, Ordering::Relaxed);
                    Report::from(WarningAsError::from(diagnostic))
                }
            },
            Some(Severity::Error) => {
                self.err_count.fetch_add(1, Ordering::Relaxed);
                diagnostic
            }
            _ => diagnostic,
        };

        if self.silent {
            return;
        }

        self.write_report(diagnostic);
    }

    #[cfg(feature = "std")]
    fn write_report(&self, diagnostic: Report) {
        use std::io::Write;

        let mut buffer = self.emitter.buffer();
        let printer = PrintDiagnostic::new(diagnostic);
        write!(&mut buffer, "{printer}").expect("failed to write diagnostic to buffer");
        self.emitter.print(buffer).unwrap();
    }

    #[cfg(not(feature = "std"))]
    fn write_report(&self, diagnostic: Report) {
        use core::fmt::Write;

        let mut buffer = self.emitter.buffer();
        let printer = PrintDiagnostic::new(diagnostic);
        write!(&mut buffer, "{printer}").expect("failed to write diagnostic to buffer");
        self.emitter.print(buffer).unwrap();
    }
}

#[derive(thiserror::Error, Diagnostic, Debug)]
#[error("{}", .report)]
#[diagnostic(
    severity(Error),
    help("this warning was promoted to an error via --warnings-as-errors")
)]
struct WarningAsError {
    #[diagnostic_source]
    report: Report,
}
impl From<Report> for WarningAsError {
    fn from(report: Report) -> Self {
        Self { report }
    }
}

/// Constructs an in-flight diagnostic using the builder pattern
pub struct InFlightDiagnosticBuilder<'h> {
    handler: &'h DiagnosticsHandler,
    diagnostic: InFlightDiagnostic,
    /// The source id of the primary diagnostic being constructed, if known
    primary_source_id: Option<SourceId>,
    /// The set of secondary labels which reference code in other source files than the primary
    references: BTreeMap<SourceId, RelatedLabel>,
}
impl<'h> InFlightDiagnosticBuilder<'h> {
    pub(crate) fn new(handler: &'h DiagnosticsHandler, severity: Severity) -> Self {
        Self {
            handler,
            diagnostic: InFlightDiagnostic::new(severity),
            primary_source_id: None,
            references: BTreeMap::default(),
        }
    }

    /// Sets the primary diagnostic message to `message`
    pub fn with_message(mut self, message: impl ToString) -> Self {
        self.diagnostic.message = message.to_string();
        self
    }

    /// Sets the error code for this diagnostic
    pub fn with_code(mut self, code: impl ToString) -> Self {
        self.diagnostic.code = Some(code.to_string());
        self
    }

    /// Sets the error url for this diagnostic
    pub fn with_url(mut self, url: impl ToString) -> Self {
        self.diagnostic.url = Some(url.to_string());
        self
    }

    /// Adds a primary label for `span` to this diagnostic, with no label message.
    pub fn with_primary_span(mut self, span: SourceSpan) -> Self {
        use miden_assembly::diagnostics::LabeledSpan;

        assert!(self.diagnostic.labels.is_empty(), "cannot set the primary span more than once");
        let source_id = span.source_id();
        let source_file = self.handler.source_manager.get(source_id).ok();
        self.primary_source_id = Some(source_id);
        self.diagnostic.source_code = source_file;
        self.diagnostic.labels.push(LabeledSpan::new_primary_with_span(None, span));
        self
    }

    /// Adds a primary label for `span` to this diagnostic, with the given message
    ///
    /// A primary label is one which should be rendered as the relevant source code
    /// at which a diagnostic originates. Secondary labels are used for related items
    /// involved in the diagnostic.
    pub fn with_primary_label(mut self, span: SourceSpan, message: impl ToString) -> Self {
        use miden_assembly::diagnostics::LabeledSpan;

        assert!(self.diagnostic.labels.is_empty(), "cannot set the primary span more than once");
        let source_id = span.source_id();
        let source_file = self.handler.source_manager.get(source_id).ok();
        self.primary_source_id = Some(source_id);
        self.diagnostic.source_code = source_file;
        self.diagnostic
            .labels
            .push(LabeledSpan::new_primary_with_span(Some(message.to_string()), span));
        self
    }

    /// Adds a secondary label for `span` to this diagnostic, with the given message
    ///
    /// A secondary label is used to point out related items in the source code which
    /// are relevant to the diagnostic, but which are not themselves the point at which
    /// the diagnostic originates.
    pub fn with_secondary_label(mut self, span: SourceSpan, message: impl ToString) -> Self {
        use miden_assembly::diagnostics::LabeledSpan;

        assert!(
            !self.diagnostic.labels.is_empty(),
            "must set a primary label before any secondary labels"
        );
        let source_id = span.source_id();
        if source_id != self.primary_source_id.unwrap_or_default() {
            let related = self.references.entry(source_id).or_insert_with(|| {
                let source_file = self.handler.source_manager.get(source_id).ok();
                RelatedLabel::advice("see diagnostics for more information")
                    .with_source_file(source_file)
            });
            related.labels.push(Label::new(span, message.to_string()));
        } else {
            self.diagnostic
                .labels
                .push(LabeledSpan::new_with_span(Some(message.to_string()), span));
        }
        self
    }

    /// Adds a note to the diagnostic
    ///
    /// Notes are used for explaining general concepts or suggestions
    /// related to a diagnostic, and are not associated with any particular
    /// source location. They are always rendered after the other diagnostic
    /// content.
    pub fn with_help(mut self, note: impl ToString) -> Self {
        self.diagnostic.help = Some(note.to_string());
        self
    }

    /// Consume this [InFlightDiagnostic] and create a [Report]
    pub fn into_report(mut self) -> Report {
        if self.diagnostic.message.is_empty() {
            self.diagnostic.message = "reported".into();
        }
        self.diagnostic.related.extend(self.references.into_values());
        Report::from(self.diagnostic)
    }

    /// Emit the underlying [Diagnostic] via the [DiagnosticHandler]
    pub fn emit(self) {
        let handler = self.handler;
        handler.emit(self.into_report());
    }
}

#[derive(Default)]
struct InFlightDiagnostic {
    source_code: Option<Arc<SourceFile>>,
    severity: Option<Severity>,
    message: String,
    code: Option<String>,
    help: Option<String>,
    url: Option<String>,
    labels: Vec<LabeledSpan>,
    related: Vec<RelatedLabel>,
}

impl InFlightDiagnostic {
    fn new(severity: Severity) -> Self {
        Self {
            severity: Some(severity),
            ..Default::default()
        }
    }
}

impl fmt::Display for InFlightDiagnostic {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", &self.message)
    }
}

impl fmt::Debug for InFlightDiagnostic {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", &self.message)
    }
}

impl core::error::Error for InFlightDiagnostic {}

impl Diagnostic for InFlightDiagnostic {
    fn code<'a>(&'a self) -> Option<Box<dyn Display + 'a>> {
        self.code.as_ref().map(Box::new).map(|c| c as Box<dyn Display>)
    }

    fn severity(&self) -> Option<Severity> {
        self.severity
    }

    fn help<'a>(&'a self) -> Option<Box<dyn Display + 'a>> {
        self.help.as_ref().map(Box::new).map(|c| c as Box<dyn Display>)
    }

    fn url<'a>(&'a self) -> Option<Box<dyn Display + 'a>> {
        self.url.as_ref().map(Box::new).map(|c| c as Box<dyn Display>)
    }

    fn labels(&self) -> Option<Box<dyn Iterator<Item = LabeledSpan> + '_>> {
        if self.labels.is_empty() {
            return None;
        }
        let iter = self.labels.iter().cloned();
        Some(Box::new(iter) as Box<dyn Iterator<Item = LabeledSpan>>)
    }

    fn related(&self) -> Option<Box<dyn Iterator<Item = &dyn Diagnostic> + '_>> {
        if self.related.is_empty() {
            return None;
        }

        let iter = self.related.iter().map(|r| r as &dyn Diagnostic);
        Some(Box::new(iter) as Box<dyn Iterator<Item = &dyn Diagnostic>>)
    }

    fn diagnostic_source(&self) -> Option<&(dyn Diagnostic + '_)> {
        None
    }
}

pub use self::into_diagnostic::{DiagnosticError, IntoDiagnostic};

mod into_diagnostic {
    use alloc::boxed::Box;

    /// Convenience [`Diagnostic`] that can be used as an "anonymous" wrapper for
    /// Errors. This is intended to be paired with [`IntoDiagnostic`].
    #[derive(Debug)]
    pub struct DiagnosticError<E>(Box<E>);
    impl<E> DiagnosticError<E> {
        pub fn new(error: E) -> Self {
            Self(Box::new(error))
        }
    }
    impl<E: core::fmt::Debug + core::fmt::Display + 'static> miden_assembly::diagnostics::Diagnostic
        for DiagnosticError<E>
    {
    }
    impl<E: core::fmt::Display> core::fmt::Display for DiagnosticError<E> {
        fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
            core::fmt::Display::fmt(self.0.as_ref(), f)
        }
    }
    impl<E: core::fmt::Debug + core::fmt::Display + 'static> core::error::Error for DiagnosticError<E> {
        default fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
            None
        }

        default fn cause(&self) -> Option<&dyn core::error::Error> {
            self.source()
        }
    }
    impl<E: core::error::Error + 'static> core::error::Error for DiagnosticError<E> {
        fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
            self.0.source()
        }
    }
    unsafe impl<E: Send> Send for DiagnosticError<E> {}
    unsafe impl<E: Sync> Sync for DiagnosticError<E> {}

    /**
    Convenience trait that adds a [`.into_diagnostic()`](IntoDiagnostic::into_diagnostic) method that converts a type implementing
    [`std::error::Error`] to a [`Result<T, Report>`].

    ## Warning

    Calling this on a type implementing [`Diagnostic`] will reduce it to the common denominator of
    [`std::error::Error`]. Meaning all extra information provided by [`Diagnostic`] will be
    inaccessible. If you have a type implementing [`Diagnostic`] consider simply returning it or using
    [`Into`] or the [`Try`](std::ops::Try) operator (`?`).
    */
    pub trait IntoDiagnostic<T, E> {
        /// Converts [`Result`] types that return regular [`std::error::Error`]s
        /// into a [`Result`] that returns a [`Diagnostic`].
        fn into_diagnostic(self) -> Result<T, super::Report>;
    }

    impl<T, E: core::fmt::Debug + core::fmt::Display + Sync + Send + 'static> IntoDiagnostic<T, E>
        for Result<T, E>
    {
        fn into_diagnostic(self) -> Result<T, super::Report> {
            self.map_err(|e| DiagnosticError::new(e).into())
        }
    }
}
