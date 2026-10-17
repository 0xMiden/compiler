//! Source correspondence and independent replay of physical plans, before assembly.
//!
//! Selection checking seals semantic evidence reconstructed from a registered source. Physical
//! verification then replays expanded instructions with independent tokens, liveness, and scratch
//! demand, discharging entry hypotheses on actual transfers. Only the resulting sealed procedure
//! can be converted to MASM without further executable edits.
mod abi;
mod control;
mod graph;
mod liveness;
mod memory;
mod reconstruction;
mod replay;
mod results;
mod selection;
mod stack;
mod storage;
mod tokens;

pub(crate) use selection::*;

use crate::{
    lower::expand::AnnotatedMasmBody,
    plan::{actions::ProcedurePlan, *},
};
/// Planning diagnostic enriched by the proof obligation that failed.
pub(crate) type VerificationError = PlanError;

/// Build a verification failure in the relevant planning diagnostic category.
pub(super) fn fail(category: DiagnosticCategory, message: &'static str) -> VerificationError {
    PlanError::new(category, message)
}
/// Report malformed selected or expanded structure without inventing physical-state context.
pub(super) fn error(message: &'static str) -> VerificationError {
    fail(DiagnosticCategory::InvalidSelectedLowering, message)
}

/// Immutable verified instructions. Conversion consumes this value; callers cannot append code.
#[derive(Debug)]
pub(crate) struct VerifiedProcedure {
    body: AnnotatedMasmBody,
    frame: FinalFrameLayout,
}

/// Consume an expanded body and prove its control, ABI, storage, and value behavior against
/// the source-checked plan and assigned frame. Failure returns no serializable proof wrapper.
pub(crate) fn verify(
    plan: &ProcedurePlan,
    frame: &FinalFrameLayout,
    body: AnnotatedMasmBody,
) -> Result<VerifiedProcedure, VerificationError> {
    replay::Verifier::new(plan, frame)?.verify(&body)?;
    Ok(VerifiedProcedure {
        body,
        frame: frame.clone(),
    })
}

#[cfg(test)]
pub(crate) mod tests;
