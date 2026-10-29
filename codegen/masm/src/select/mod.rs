//! Translate final HIR into the semantic program consumed by allocation.
//!
//! Selection records values, lowering units, source coverage, and structured control flow;
//! it owns no physical locations and never mutates HIR. Registered source rules also construct
//! the executable definitions and evidence used to check an allocation candidate. Source checking
//! reruns this trusted selector against current HIR; physical replay is independent of allocation.
//! A selected graph describes the requested computation, but does not certify its own semantics.
pub(crate) mod generated;
pub(crate) mod graph;

use alloc::vec::Vec;

use midenc_hir::{Immediate, OperationRef, Report, dialects::builtin};

use crate::{lower::units::HirUnit, plan::*, verify::DefinitionEvidence};

/// Immutable source information, shared with source checking rather than supplied by a candidate.
/// Keep source lookup here so initial selection and source checking see the same linked facts.
#[derive(Default)]
pub(crate) struct SelectEnv<'a> {
    /// Linked symbol and function-table layouts needed by source rules which address them.
    pub link_info: Option<&'a crate::linker::LinkInfo>,
    /// Closed final emission identities, fixed before source reconstruction.
    pub owners: Option<&'a crate::lower::EmissionOwners>,
    /// Address domain derived from the entire compilation unit before selection.
    pub byte_addressing: crate::emit::addressing::ByteAddressing,
}

/// The finite vocabulary returned by [`crate::HirSelection`] implementations.
///
/// Each rule has a source-specific validator and construction in the selector. Advertising a
/// region interface alone does not authorize an operation or its nested control flow.
#[derive(Clone, Copy)]
pub(crate) enum SelectionRule {
    Arithmetic(crate::lower::units::arithmetic::Kind),
    Memory(crate::lower::units::memory::Kind),
    Intrinsic(crate::lower::units::intrinsics::Kind),
    Constant(Immediate),
    /// Preserve raw lanes through a source-checked representation change.
    Alias,
    /// Expose the lanes of one value as independently named results.
    Split,
    /// Name a whole value assembled from compatible source lanes.
    Join,
    If,
    Switch,
    While,
    Yield,
    Condition,
    Return,
    ReturnImmediate(Immediate),
    Trap,
    TerminalBranch,
    Debug,
    /// Stage a procedure root in declared scratch before context-changing dispatch.
    Dyncall,
    /// Resolve a direct callable and validate its concrete invocation family.
    DirectCall,
    /// Check a linked function-table slot before same-context dispatch.
    ExecIndirect,
    /// Protocol executor with semantic stored prefix and payload operands.
    Fpi,
    /// Source-owned reference to a validated callable digest.
    ProcedureRoot,
}

/// Source reconstruction paired with the executable definitions it authorizes.
///
/// Keep both when lowering a procedure: allocation consumes the selected program, while
/// expansion registers `definitions` and source checking compares against `source`.
pub(crate) struct HirEvidence {
    /// Selected semantics and per-unit facts derived from HIR rather than a physical plan.
    pub source: DefinitionEvidence,
    /// Instruction constructions indexed by the selected units' lowering kinds.
    pub definitions: Vec<HirUnit>,
    /// Source-ordered observations, separate from semantic uses and equality.
    pub observations: Vec<crate::lower::debug::Observation>,
}

impl HirEvidence {
    /// Extract the selected graph, discarding the definitions and source-checking evidence.
    #[cfg(test)]
    pub fn program(self) -> SelectedProgram {
        self.source.program
    }
}

/// Select a function when only its semantic graph is needed.
///
/// Use [`evidence`] when constructing executable lowerings. Invalid or unsupported final HIR,
/// declarations, inconsistent types, and unrepresentable target layouts are reported as errors.
#[cfg(test)]
pub(crate) fn select_function(
    function: &builtin::Function,
    env: &SelectEnv<'_>,
) -> Result<SelectedProgram, Report> {
    Ok(evidence(function, env)?.program())
}

/// Reconstruct a function's selected graph, executable definitions, and source evidence together.
/// The function must be a supported final-HIR definition; selection never repairs its types.
pub(crate) fn evidence(
    function: &builtin::Function,
    env: &SelectEnv<'_>,
) -> Result<HirEvidence, Report> {
    graph::Selector::select(function, env)
}

/// Select a registered generated definition using only its semantic source parameters.
/// Executable constructions must come from that same definition and are registered separately;
/// successful selection remains subject to independent source and physical verification.
#[cfg(test)]
pub(crate) fn select_generated(
    definition: &dyn crate::verify::GeneratedDefinition,
    parameters: &[midenc_hir::Felt],
) -> Result<SelectedProgram, Report> {
    Ok(definition.instantiate(parameters)?.program)
}

pub(crate) fn invalid(message: impl Into<alloc::string::String>) -> Report {
    PlanError::new(DiagnosticCategory::InvalidSelectedLowering, message).into()
}

/// Checked source metadata for a table lookup; never synthesized from an allocation candidate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TableLookup {
    /// Indirect call whose checked table access this describes.
    pub operation: OperationRef,
    /// Linked table source supplying the slot bounds and layout.
    pub table: OperationRef,
    /// Selected bounds/signature check which produces `address`.
    pub lookup: SelectedStepId,
    /// Exact checked slot address consumed by the later dispatch phase.
    pub address: Atom,
    /// Table base in field elements.
    pub base: u32,
    /// Number of addressable function slots.
    pub slots: u32,
    /// Required linked signature tag; a mismatch or null entry traps at lookup.
    pub tag: u32,
    /// Source call signature used to validate the dispatch ABI.
    pub signature: midenc_hir::dialects::builtin::attributes::Signature,
}

#[cfg(test)]
pub(crate) mod tests;
