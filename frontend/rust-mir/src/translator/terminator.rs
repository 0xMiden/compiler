//! Translates MIR terminators.

use midenc_dialect_hir::HirOpBuilder;
use midenc_hir::{Report, SourceSpan, dialects::builtin::BuiltinOpBuilder};
use rustc_public::mir::{RETURN_LOCAL, Terminator, TerminatorKind};

use super::body::BodyTranslator;

/// Translates one MIR terminator into Miden IR.
pub(crate) fn translate_terminator(
    translator: &mut BodyTranslator<'_>,
    terminator: &Terminator,
) -> Result<(), Report> {
    match &terminator.kind {
        TerminatorKind::Return => translate_return(translator),
        kind => Err(Report::msg(format!(
            "rust mir frontend: unsupported MIR terminator: {}",
            terminator_name(kind)
        ))),
    }
}

/// Translates the MIR `return` terminator.
///
/// MIR holds the result of a function in the return local `_0`. A function that returns the unit
/// type has no result value.
fn translate_return(translator: &mut BodyTranslator<'_>) -> Result<(), Report> {
    if translator.has_slot(RETURN_LOCAL) {
        let slot = translator.slot(RETURN_LOCAL)?;
        let value = translator.builder.load_local(slot, SourceSpan::UNKNOWN)?;
        translator.builder.ret([value], SourceSpan::UNKNOWN)?;
    } else {
        translator.builder.ret([], SourceSpan::UNKNOWN)?;
    }
    Ok(())
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
