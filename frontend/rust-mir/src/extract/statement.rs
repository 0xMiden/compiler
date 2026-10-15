//! Extracts MIR statements.

use rustc_public::mir::{Statement, StatementKind};

use super::{ExtractResult, body, rvalue};
use crate::extracted::Assign;

/// Extracts one MIR statement.
///
/// Returns `None` for a statement that the Miden IR does not need.
pub(super) fn extract_statement(statement: &Statement) -> ExtractResult<Option<Assign>> {
    match &statement.kind {
        StatementKind::Assign(place, value) => Ok(Some(Assign {
            dest: body::place_local(place)?,
            value: rvalue::extract_rvalue(value)?,
        })),
        // The local slots live for the whole function, thus the storage markers say nothing that
        // the Miden IR needs.
        StatementKind::StorageLive(_) | StatementKind::StorageDead(_) | StatementKind::Nop => {
            Ok(None)
        }
        kind => Err(format!(
            "rust mir frontend: unsupported MIR statement: {}",
            statement_name(kind)
        )),
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
        StatementKind::PlaceMention(_) => "PlaceMention",
        StatementKind::AscribeUserType { .. } => "AscribeUserType",
        StatementKind::Coverage(_) => "Coverage",
        StatementKind::Intrinsic(_) => "Intrinsic",
        StatementKind::ConstEvalCounter => "ConstEvalCounter",
        StatementKind::Nop => "Nop",
    }
}
