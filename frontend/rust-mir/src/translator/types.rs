//! Maps Rust MIR types to Miden IR types.

use midenc_hir::{Report, Type};
use rustc_public::ty::{IntTy, RigidTy, Ty, TyKind, UintTy};

/// Maps a Rust MIR type to a Miden IR type.
///
/// The frontend compiles for a 32-bit target, thus `isize` and `usize` are 32 bits wide.
pub(crate) fn translate_ty(ty: Ty) -> Result<Type, Report> {
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
        kind => Err(unsupported_type(kind)),
    }
}

/// Tells whether a Rust MIR type is the unit type.
///
/// A local of the unit type carries no data, thus it gets no local slot.
pub(crate) fn is_unit(ty: Ty) -> bool {
    ty.kind().is_unit()
}

/// Builds the error for a type that the frontend cannot map.
pub(crate) fn unsupported_type(kind: &TyKind) -> Report {
    Report::msg(format!("rust mir frontend: unsupported type in MIR: {kind:?}"))
}
