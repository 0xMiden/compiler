//! Translates MIR rvalues.

use midenc_dialect_arith::ArithOpBuilder;
use midenc_hir::{Report, SourceSpan, ValueRef};
use rustc_public::mir::{BinOp, Operand, Rvalue};

use super::{body::BodyTranslator, operand};

/// Translates one MIR rvalue into Miden IR and returns the value it produces.
pub(crate) fn translate_rvalue(
    translator: &mut BodyTranslator<'_>,
    rvalue: &Rvalue,
) -> Result<ValueRef, Report> {
    match rvalue {
        Rvalue::Use(value) => operand::translate_operand(translator, value),
        Rvalue::BinaryOp(op, lhs, rhs) => translate_binary_op(translator, *op, lhs, rhs),
        rvalue => Err(Report::msg(format!(
            "rust mir frontend: unsupported MIR rvalue: {}",
            rvalue_name(rvalue)
        ))),
    }
}

/// Translates a MIR binary operation into Miden IR.
///
/// Rust checks arithmetic overflow with a separate `CheckedBinaryOp` and an assertion, thus a
/// plain `Add` wraps around.
fn translate_binary_op(
    translator: &mut BodyTranslator<'_>,
    op: BinOp,
    lhs: &Operand,
    rhs: &Operand,
) -> Result<ValueRef, Report> {
    match op {
        BinOp::Add | BinOp::AddUnchecked => {
            let lhs = operand::translate_operand(translator, lhs)?;
            let rhs = operand::translate_operand(translator, rhs)?;
            translator.builder.add_wrapping(lhs, rhs, SourceSpan::UNKNOWN)
        }
        op => Err(Report::msg(format!(
            "rust mir frontend: unsupported MIR binary operator: {op:?}"
        ))),
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
        Rvalue::Ref(..) => "Ref",
        Rvalue::Repeat(..) => "Repeat",
        Rvalue::ThreadLocalRef(_) => "ThreadLocalRef",
        Rvalue::UnaryOp(..) => "UnaryOp",
        Rvalue::Use(_) => "Use",
    }
}
