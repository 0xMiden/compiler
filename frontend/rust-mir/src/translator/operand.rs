//! Translates MIR operands.

use midenc_dialect_arith::ArithOpBuilder;
use midenc_dialect_hir::HirOpBuilder;
use midenc_hir::{Immediate, Report, SourceSpan, Type, ValueRef};
use rustc_public::{
    mir::{ConstOperand, Operand},
    ty::{Allocation, ConstantKind},
};

use super::{body::BodyTranslator, types};

/// Translates one MIR operand into Miden IR and returns the value it produces.
///
/// `Copy` and `Move` read the same local slot. The difference between them says who owns the
/// value afterwards, which the Miden IR does not track.
pub(crate) fn translate_operand(
    translator: &mut BodyTranslator<'_>,
    operand: &Operand,
) -> Result<ValueRef, Report> {
    match operand {
        Operand::Copy(place) | Operand::Move(place) => {
            let slot = translator.place_slot(place)?;
            translator.builder.load_local(slot, SourceSpan::UNKNOWN)
        }
        Operand::Constant(constant) => translate_constant(translator, constant),
        Operand::RuntimeChecks(check) => Err(Report::msg(format!(
            "rust mir frontend: unsupported MIR operand: RuntimeChecks({check:?})"
        ))),
    }
}

/// Translates a MIR constant operand into a Miden IR constant.
fn translate_constant(
    translator: &mut BodyTranslator<'_>,
    constant: &ConstOperand,
) -> Result<ValueRef, Report> {
    let ty = types::translate_ty(constant.const_.ty())?;
    match constant.const_.kind() {
        ConstantKind::Allocated(allocation) => {
            let value = immediate(allocation, &ty)?;
            Ok(translator.builder.imm(value, SourceSpan::UNKNOWN))
        }
        kind => Err(Report::msg(format!(
            "rust mir frontend: unsupported MIR constant: {}",
            constant_name(kind)
        ))),
    }
}

/// Reads a scalar constant of the given Miden IR type out of a MIR allocation.
fn immediate(allocation: &Allocation, ty: &Type) -> Result<Immediate, Report> {
    let read_uint = || -> Result<u128, Report> {
        allocation.read_uint().map_err(|err| {
            Report::msg(format!("rust mir frontend: cannot read an integer constant: {err:?}"))
        })
    };
    let read_int = || -> Result<i128, Report> {
        allocation.read_int().map_err(|err| {
            Report::msg(format!("rust mir frontend: cannot read an integer constant: {err:?}"))
        })
    };

    match ty {
        Type::I1 => Ok(Immediate::I1(allocation.read_bool().map_err(|err| {
            Report::msg(format!("rust mir frontend: cannot read a boolean constant: {err:?}"))
        })?)),
        Type::I8 => Ok(Immediate::I8(read_int()? as i8)),
        Type::I16 => Ok(Immediate::I16(read_int()? as i16)),
        Type::I32 => Ok(Immediate::I32(read_int()? as i32)),
        Type::I64 => Ok(Immediate::I64(read_int()? as i64)),
        Type::I128 => Ok(Immediate::I128(read_int()?)),
        Type::U8 => Ok(Immediate::U8(read_uint()? as u8)),
        Type::U16 => Ok(Immediate::U16(read_uint()? as u16)),
        Type::U32 => Ok(Immediate::U32(read_uint()? as u32)),
        Type::U64 => Ok(Immediate::U64(read_uint()? as u64)),
        Type::U128 => Ok(Immediate::U128(read_uint()?)),
        ty => Err(Report::msg(format!("rust mir frontend: unsupported constant type: {ty}"))),
    }
}

/// Returns the name of a MIR constant kind.
fn constant_name(kind: &ConstantKind) -> &'static str {
    match kind {
        ConstantKind::Ty(_) => "Ty",
        ConstantKind::Allocated(_) => "Allocated",
        ConstantKind::Unevaluated(_) => "Unevaluated",
        ConstantKind::Param(_) => "Param",
        ConstantKind::ZeroSized => "ZeroSized",
    }
}
