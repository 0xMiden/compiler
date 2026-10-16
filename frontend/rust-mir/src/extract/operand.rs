//! Extracts MIR operands.

use midenc_hir::{Immediate, Type};
use rustc_public::{
    mir::{ConstOperand, Operand},
    ty::{Allocation, ConstantKind},
};

use super::{ExtractResult, body, types};
use crate::extracted;

/// Extracts one MIR operand.
pub(super) fn extract_operand(operand: &Operand) -> ExtractResult<extracted::Operand> {
    match operand {
        Operand::Copy(place) | Operand::Move(place) => {
            Ok(extracted::Operand::Local(body::place_local(place)?))
        }
        Operand::Constant(constant) => {
            Ok(extracted::Operand::Constant(extract_constant(constant)?))
        }
        Operand::RuntimeChecks(check) => {
            Err(format!("rust mir frontend: unsupported MIR operand: RuntimeChecks({check:?})"))
        }
    }
}

/// Extracts the value of a MIR constant operand.
fn extract_constant(constant: &ConstOperand) -> ExtractResult<Immediate> {
    let ty = types::extract_ty(constant.const_.ty())?;
    match constant.const_.kind() {
        ConstantKind::Allocated(allocation) => immediate(allocation, &ty),
        kind => {
            Err(format!("rust mir frontend: unsupported MIR constant: {}", constant_name(kind)))
        }
    }
}

/// Reads a scalar constant of the given Miden IR type out of a MIR allocation.
fn immediate(allocation: &Allocation, ty: &Type) -> ExtractResult<Immediate> {
    let read_uint = || -> ExtractResult<u128> {
        allocation
            .read_uint()
            .map_err(|err| format!("rust mir frontend: cannot read an integer constant: {err:?}"))
    };
    let read_int = || -> ExtractResult<i128> {
        allocation
            .read_int()
            .map_err(|err| format!("rust mir frontend: cannot read an integer constant: {err:?}"))
    };

    match ty {
        Type::I1 => Ok(Immediate::I1(allocation.read_bool().map_err(|err| {
            format!("rust mir frontend: cannot read a boolean constant: {err:?}")
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
        ty => Err(format!("rust mir frontend: unsupported constant type: {ty}")),
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
