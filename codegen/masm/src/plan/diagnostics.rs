//! Shared diagnostics that retain source provenance across planning and verification.
use alloc::{format, string::String, vec::Vec};
use core::fmt;

use midenc_session::diagnostics::{DiagnosticsHandler, Report, Severity};

use super::{Atom, SelectedStepId};
use crate::masm::SourceSpan;

/// Shared failure classes for planning and independent verification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DiagnosticCategory {
    /// Selected semantics, source evidence, or expanded unit structure disagree.
    InvalidSelectedLowering,
    /// A required semantic lane has no usable representation.
    UnavailableValue,
    /// A transfer fails to establish the destination's semantic or storage contract.
    InvalidEdgeTransfer,
    /// A target rule or stack operation violates raw access or protected-suffix limits.
    IllegalStackAccess,
    /// Compiler storage lacks required initialization, identity, or access permission.
    InvalidSpillSlotRead,
    /// Invocation, argument exposure, or result/return shape violates its ABI.
    AbiMismatch,
    /// A frame assignment or reference violates bounds, alignment, or target capacity.
    FrameCapacityExceeded,
}

/// Source span and inline ancestry retained through lowering for plan diagnostics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SourceLocation {
    pub span: SourceSpan,
    pub inline_chain: Vec<SourceSpan>,
}

impl Default for SourceLocation {
    fn default() -> Self {
        Self {
            span: SourceSpan::UNKNOWN,
            inline_chain: Vec::new(),
        }
    }
}

/// A failed planning or proof obligation, with optional raw-state context.
/// Producers enrich the fields they know; an absent field means no context was attached.
#[derive(Debug, Clone)]
pub(crate) struct PlanError {
    pub category: DiagnosticCategory,
    pub origin: SourceLocation,
    pub step: Option<SelectedStepId>,
    /// Zero-based raw stack position involved in the failure, when known.
    pub depth: Option<usize>,
    pub expected: Option<Atom>,
    pub actual: Option<Atom>,
    pub message: String,
}

impl PlanError {
    /// Resolve source provenance through the session's diagnostic renderer.
    ///
    /// An unavailable primary source renders text and a location-unavailable note;
    /// callers are not promoted to the failure origin. With a usable primary,
    /// unresolved inline frames produce a note while usable frames remain labeled.
    pub fn into_report(self, diagnostics: &DiagnosticsHandler) -> Report {
        let mut diagnostic = diagnostics.diagnostic(Severity::Error).with_message(&self);
        let sources = diagnostics.source_manager_ref();
        let resolvable = |span: SourceSpan| {
            let range = span.into_range();
            !span.is_unknown()
                && sources.get(span.source_id()).is_ok_and(|source| {
                    source.source_slice(range.start as usize..range.end as usize).is_some()
                })
        };
        if resolvable(self.origin.span) {
            diagnostic = diagnostic.with_primary_label(self.origin.span, &self.message);
            let mut missing_inline = false;
            for (index, span) in self.origin.inline_chain.iter().copied().enumerate() {
                if resolvable(span) {
                    diagnostic = diagnostic
                        .with_secondary_label(span, format!("inlined from (frame {})", index + 1));
                } else {
                    missing_inline = true;
                }
            }
            if missing_inline {
                diagnostic =
                    diagnostic.with_help("one or more inline source locations unavailable");
            }
        } else {
            diagnostic = diagnostic.with_help("source location unavailable");
        }
        diagnostic.into_report()
    }

    /// Attach a known selected boundary without replacing more precise nested context.
    pub fn at_step(mut self, step: SelectedStepId, location: Option<&SourceLocation>) -> Self {
        if self.step.is_none() || self.step == Some(step) {
            self.step = Some(step);
            if self.origin == SourceLocation::default()
                && let Some(location) = location
            {
                self.origin = location.clone();
            }
        }
        self
    }

    /// Attach the semantic lane whose availability or identity was required.
    pub fn expecting(mut self, atom: Atom) -> Self {
        self.expected.get_or_insert(atom);
        self
    }

    /// Attach an observed raw position and only identities established by replay.
    pub fn mismatch(mut self, depth: usize, expected: Atom, actual: Option<Atom>) -> Self {
        self.depth.get_or_insert(depth);
        self.expected.get_or_insert(expected);
        if self.actual.is_none() {
            self.actual = actual;
        }
        self
    }

    /// Start a diagnostic with unknown source location and no attached state context.
    pub fn new(category: DiagnosticCategory, message: impl Into<String>) -> Self {
        Self {
            category,
            origin: SourceLocation::default(),
            step: None,
            depth: None,
            expected: None,
            actual: None,
            message: message.into(),
        }
    }
}

impl fmt::Display for PlanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}: {}", self.category, self.message)?;
        if let Some(step) = self.step {
            write!(f, " step=s{}", step.0)?;
        }
        if let Some(depth) = self.depth {
            write!(f, " depth={depth}")?;
        }
        if let Some(atom) = self.expected {
            write!(f, " expected={atom}")?;
        }
        if let Some(atom) = self.actual {
            write!(f, " actual={atom}")?;
        }
        Ok(())
    }
}

impl core::error::Error for PlanError {}

impl midenc_session::diagnostics::Diagnostic for PlanError {}
