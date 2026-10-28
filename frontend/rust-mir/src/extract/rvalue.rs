//! Extracts MIR rvalues.

use rustc_public::mir::{BinOp, Operand, Rvalue};

use super::{ExtractResult, operand};
use crate::extracted;

/// Extracts one MIR rvalue.
pub(super) fn extract_rvalue(rvalue: &Rvalue) -> ExtractResult<extracted::Rvalue> {
    match rvalue {
        // The retag marker is aliasing-model metadata. It has no runtime effect, thus the Miden IR
        // ignores it.
        Rvalue::Use(value, _) => Ok(extracted::Rvalue::Use(operand::extract_operand(value)?)),
        Rvalue::BinaryOp(op, lhs, rhs) => extract_binary_op(*op, lhs, rhs),
        rvalue => {
            Err(format!("rust mir frontend: unsupported MIR rvalue: {}", rvalue_name(rvalue)))
        }
    }
}

/// Extracts a MIR binary operation.
///
/// Rust checks arithmetic overflow with a separate `CheckedBinaryOp` and an assertion, thus a
/// plain `Add` wraps around.
fn extract_binary_op(op: BinOp, lhs: &Operand, rhs: &Operand) -> ExtractResult<extracted::Rvalue> {
    match op {
        BinOp::Add | BinOp::AddUnchecked => Ok(extracted::Rvalue::AddWrapping(
            operand::extract_operand(lhs)?,
            operand::extract_operand(rhs)?,
        )),
        op => Err(format!("rust mir frontend: unsupported MIR binary operator: {op:?}")),
    }
}

/// Returns the name of a MIR rvalue.
fn rvalue_name(rvalue: &Rvalue) -> &'static str {
    match rvalue {
        Rvalue::AddressOf(..) => "AddressOf",
        Rvalue::Aggregate(..) => "Aggregate",
        Rvalue::BinaryOp(..) => "BinaryOp",
        Rvalue::Cast(..) => "Cast",
        Rvalue::CheckedBinaryOp(..) => "CheckedBinaryOp",
        Rvalue::CopyForDeref(_) => "CopyForDeref",
        Rvalue::Discriminant(_) => "Discriminant",
        Rvalue::Len(_) => "Len",
        Rvalue::Reborrow(..) => "Reborrow",
        Rvalue::Ref(..) => "Ref",
        Rvalue::Repeat(..) => "Repeat",
        Rvalue::ThreadLocalRef(_) => "ThreadLocalRef",
        Rvalue::UnaryOp(..) => "UnaryOp",
        Rvalue::Use(..) => "Use",
    }
}
