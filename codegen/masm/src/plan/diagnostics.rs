//! Shared diagnostics that retain source provenance across planning and verification.
use alloc::{string::String, vec::Vec};
use core::fmt;

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
        write!(f, "{:?}: {} at {:?}", self.category, self.message, self.origin.span)?;
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
        for span in &self.origin.inline_chain {
            write!(f, " inline={span:?}")?;
        }
        Ok(())
    }
}

impl core::error::Error for PlanError {}

impl midenc_session::diagnostics::Diagnostic for PlanError {}
