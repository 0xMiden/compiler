//! Maps Rust MIR types to Miden IR types.

use midenc_hir::Type;
use rustc_public::ty::{IntTy, RigidTy, Ty, TyKind, UintTy};

use super::ExtractResult;

/// Maps a Rust MIR type to a Miden IR type.
///
/// The frontend compiles for a 32-bit target, thus `isize` and `usize` are 32 bits wide.
pub(super) fn extract_ty(ty: Ty) -> ExtractResult<Type> {
    let kind = ty.kind();
    match &kind {
        TyKind::RigidTy(RigidTy::Bool) => Ok(Type::I1),
        TyKind::RigidTy(RigidTy::Int(int_ty)) => Ok(match int_ty {
            IntTy::Isize => Type::I32,
            IntTy::I8 => Type::I8,
            IntTy::I16 => Type::I16,
            IntTy::I32 => Type::I32,
            IntTy::I64 => Type::I64,
            IntTy::I128 => Type::I128,
        }),
        TyKind::RigidTy(RigidTy::Uint(uint_ty)) => Ok(match uint_ty {
            UintTy::Usize => Type::U32,
            UintTy::U8 => Type::U8,
            UintTy::U16 => Type::U16,
            UintTy::U32 => Type::U32,
            UintTy::U64 => Type::U64,
            UintTy::U128 => Type::U128,
        }),
        kind => Err(format!("rust mir frontend: unsupported type in MIR: {kind:?}")),
    }
}

/// Maps the type of a value to a Miden IR type, or to `None` for the unit type.
///
/// A value of the unit type carries no data, thus it has no Miden IR type.
pub(super) fn extract_value_ty(ty: Ty) -> ExtractResult<Option<Type>> {
    if ty.kind().is_unit() {
        Ok(None)
    } else {
        extract_ty(ty).map(Some)
    }
}
