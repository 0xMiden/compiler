//! Translates MIR statements.

use midenc_dialect_hir::HirOpBuilder;
use midenc_hir::{Report, SourceSpan};
use rustc_public::mir::{Statement, StatementKind};

use super::{body::BodyTranslator, rvalue};

/// Translates one MIR statement into Miden IR.
pub(crate) fn translate_statement(
    translator: &mut BodyTranslator<'_>,
    statement: &Statement,
) -> Result<(), Report> {
    match &statement.kind {
        StatementKind::Assign(place, value) => {
            let slot = translator.place_slot(place)?;
            let value = rvalue::translate_rvalue(translator, value)?;
            translator.builder.store_local(slot, value, SourceSpan::UNKNOWN)?;
            Ok(())
        }
        // The local slots live for the whole function, thus the storage markers say nothing that
        // the Miden IR needs.
        StatementKind::StorageLive(_) | StatementKind::StorageDead(_) | StatementKind::Nop => {
            Ok(())
        }
        kind => Err(Report::msg(format!(
            "rust mir frontend: unsupported MIR statement: {}",
            statement_name(kind)
        ))),
    }
}

/// Returns the name of a MIR statement kind.
fn statement_name(kind: &StatementKind) -> &'static str {
    match kind {
        StatementKind::Assign(..) => "Assign",
        StatementKind::FakeRead(..) => "FakeRead",
        StatementKind::SetDiscriminant { .. } => "SetDiscriminant",
        StatementKind::StorageLive(_) => "StorageLive",
        StatementKind::StorageDead(_) => "StorageDead",
        StatementKind::Retag(..) => "Retag",
        StatementKind::PlaceMention(_) => "PlaceMention",
        StatementKind::AscribeUserType { .. } => "AscribeUserType",
        StatementKind::Coverage(_) => "Coverage",
        StatementKind::Intrinsic(_) => "Intrinsic",
        StatementKind::ConstEvalCounter => "ConstEvalCounter",
        StatementKind::Nop => "Nop",
    }
}
