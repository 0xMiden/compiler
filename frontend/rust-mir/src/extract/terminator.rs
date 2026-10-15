//! Checks MIR terminators.

use rustc_public::mir::{Terminator, TerminatorKind};

use super::ExtractResult;

/// Checks that the frontend supports a MIR terminator.
///
/// The only supported terminator is `return`, thus the extracted body records no terminator.
pub(super) fn check_terminator(terminator: &Terminator) -> ExtractResult<()> {
    match &terminator.kind {
        TerminatorKind::Return => Ok(()),
        kind => Err(format!(
            "rust mir frontend: unsupported MIR terminator: {}",
            terminator_name(kind)
        )),
    }
}

/// Returns the name of a MIR terminator kind.
fn terminator_name(kind: &TerminatorKind) -> &'static str {
    match kind {
        TerminatorKind::Goto { .. } => "Goto",
        TerminatorKind::SwitchInt { .. } => "SwitchInt",
        TerminatorKind::Resume => "Resume",
        TerminatorKind::Abort => "Abort",
        TerminatorKind::Return => "Return",
        TerminatorKind::Unreachable => "Unreachable",
        TerminatorKind::Drop { .. } => "Drop",
        TerminatorKind::Call { .. } => "Call",
        TerminatorKind::Assert { .. } => "Assert",
        TerminatorKind::InlineAsm { .. } => "InlineAsm",
    }
}
