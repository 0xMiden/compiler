//! Helper functions and structures for the translation.

use midenc_hir::{
    diagnostics::{DiagnosticsHandler, SourceSpan},
    AbiParam, CallConv, Felt, FieldElement, InstBuilder, Linkage, Signature, Value,
};
use midenc_hir_type::{FunctionType, Type};

use crate::{
    error::WasmResult, module::function_builder_ext::FunctionBuilderExt, unsupported_diag,
};

/// Represents the possible sizes in bytes of the discriminant of a variant type in the component
/// model
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum DiscriminantSize {
    /// 8-bit discriminant
    Size1,
    /// 16-bit discriminant
    Size2,
    /// 32-bit discriminant
    Size4,
}

impl DiscriminantSize {
    /// Calculate the size of discriminant needed to represent a variant with the specified number
    /// of cases.
    pub const fn from_count(count: usize) -> Option<Self> {
        if count <= 0xff {
            Some(Self::Size1)
        } else if count <= 0xffff {
            Some(Self::Size2)
        } else if count <= 0xffff_ffff {
            Some(Self::Size4)
        } else {
            None
        }
    }

    /// Returns the size, in bytes, of this discriminant
    pub const fn byte_size(&self) -> u32 {
        match self {
            DiscriminantSize::Size1 => 1,
            DiscriminantSize::Size2 => 2,
            DiscriminantSize::Size4 => 4,
        }
    }
}

impl From<DiscriminantSize> for u32 {
    /// Size of the discriminant as a `u32`
    fn from(size: DiscriminantSize) -> u32 {
        size.byte_size()
    }
}

impl From<DiscriminantSize> for usize {
    /// Size of the discriminant as a `usize`
    fn from(size: DiscriminantSize) -> usize {
        match size {
            DiscriminantSize::Size1 => 1,
            DiscriminantSize::Size2 => 2,
            DiscriminantSize::Size4 => 4,
        }
    }
}

/// Represents the number of bytes required to store a flags value in the component model
pub enum FlagsSize {
    /// There are no flags
    Size0,
    /// Flags can fit in a u8
    Size1,
    /// Flags can fit in a u16
    Size2,
    /// Flags can fit in a specified number of u32 fields
    Size4Plus(u8),
}

impl FlagsSize {
    /// Calculate the size needed to represent a value with the specified number of flags.
    pub const fn from_count(count: usize) -> FlagsSize {
        if count == 0 {
            FlagsSize::Size0
        } else if count <= 8 {
            FlagsSize::Size1
        } else if count <= 16 {
            FlagsSize::Size2
        } else {
            let amt = ceiling_divide(count, 32);
            if amt > (u8::MAX as usize) {
                panic!("too many flags");
            }
            FlagsSize::Size4Plus(amt as u8)
        }
    }
}

/// Divide `n` by `d`, rounding up in the case of a non-zero remainder.
const fn ceiling_divide(n: usize, d: usize) -> usize {
    n.div_ceil(d)
}

/// Emit instructions to produce a zero value in the given type.
pub fn emit_zero(
    ty: &Type,
    builder: &mut FunctionBuilderExt,
    diagnostics: &DiagnosticsHandler,
) -> WasmResult<Value> {
    Ok(match ty {
        Type::I1 => builder.ins().i1(false, SourceSpan::default()),
        Type::I8 => builder.ins().i8(0, SourceSpan::default()),
        Type::I16 => builder.ins().i16(0, SourceSpan::default()),
        Type::I32 => builder.ins().i32(0, SourceSpan::default()),
        Type::I64 => builder.ins().i64(0, SourceSpan::default()),
        Type::U8 => builder.ins().u8(0, SourceSpan::default()),
        Type::U16 => builder.ins().u16(0, SourceSpan::default()),
        Type::U32 => builder.ins().u32(0, SourceSpan::default()),
        Type::U64 => builder.ins().u64(0, SourceSpan::default()),
        Type::F64 => builder.ins().f64(0.0, SourceSpan::default()),
        Type::Felt => builder.ins().felt(Felt::ZERO, SourceSpan::default()),
        Type::I128
        | Type::U128
        | Type::U256
        | Type::Ptr(_)
        | Type::NativePtr(..)
        | Type::Struct(_)
        | Type::Array(..)
        | Type::List(_)
        | Type::Unknown
        | Type::Unit
        | Type::Never => {
            unsupported_diag!(diagnostics, "cannot emit zero value for type: {:?}", ty);
        }
    })
}

pub fn sig_from_func_type(
    func_type: &FunctionType,
    call_conv: CallConv,
    linkage: Linkage,
) -> Signature {
    Signature {
        params: func_type.params.iter().map(|ty| AbiParam::new(ty.clone())).collect(),
        results: func_type.results.iter().map(|ty| AbiParam::new(ty.clone())).collect(),
        cc: call_conv,
        linkage,
    }
}
