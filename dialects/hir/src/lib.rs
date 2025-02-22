#![feature(debug_closure_helpers)]
#![feature(unboxed_closures)]
#![feature(fn_traits)]
#![feature(ptr_metadata)]
#![feature(specialization)]
#![allow(incomplete_features)]
#![no_std]

extern crate alloc;

#[cfg(any(feature = "std", test))]
extern crate std;

mod builders;
mod canonicalization;
mod ops;
pub mod transforms;

use alloc::boxed::Box;

use midenc_hir2::{
    AttributeValue, Builder, BuilderExt, Dialect, DialectInfo, DialectRegistration, Immediate,
    OperationRef, SourceSpan, Type,
};

pub use self::{
    builders::{DefaultInstBuilder, FunctionBuilder, InstBuilder, InstBuilderBase},
    ops::*,
};

#[derive(Debug)]
pub struct HirDialect {
    info: DialectInfo,
}

impl HirDialect {
    #[inline]
    pub fn num_registered(&self) -> usize {
        self.registered_ops().len()
    }
}

impl Dialect for HirDialect {
    #[inline]
    fn info(&self) -> &DialectInfo {
        &self.info
    }

    fn materialize_constant(
        &self,
        builder: &mut dyn Builder,
        attr: Box<dyn AttributeValue>,
        ty: &Type,
        span: SourceSpan,
    ) -> Option<OperationRef> {
        // Save the current insertion point
        let mut builder = midenc_hir2::InsertionGuard::new(builder);

        // Only integer constants are supported for now
        if !ty.is_integer() {
            return None;
        }

        // Currently, we expect folds to produce `Immediate`-valued attributes
        if let Some(&imm) = attr.downcast_ref::<Immediate>() {
            // If the immediate value is of the same type as the expected result type, we're ready
            // to materialize the constant
            let imm_ty = imm.ty();
            if &imm_ty == ty {
                let op_builder = builder.create::<Constant, _>(span);
                return op_builder(imm).ok().map(|op| op.as_operation_ref());
            }

            // The immediate value has a different type than expected, but we can coerce types, so
            // long as the value fits in the target type
            if imm_ty.size_in_bits() > ty.size_in_bits() {
                return None;
            }

            let imm = match ty {
                Type::I8 => match imm {
                    Immediate::I1(value) => Immediate::I8(value as i8),
                    Immediate::U8(value) => Immediate::I8(i8::try_from(value).ok()?),
                    _ => return None,
                },
                Type::U8 => match imm {
                    Immediate::I1(value) => Immediate::U8(value as u8),
                    Immediate::I8(value) => Immediate::U8(u8::try_from(value).ok()?),
                    _ => return None,
                },
                Type::I16 => match imm {
                    Immediate::I1(value) => Immediate::I16(value as i16),
                    Immediate::I8(value) => Immediate::I16(value as i16),
                    Immediate::U8(value) => Immediate::I16(value.into()),
                    Immediate::U16(value) => Immediate::I16(i16::try_from(value).ok()?),
                    _ => return None,
                },
                Type::U16 => match imm {
                    Immediate::I1(value) => Immediate::U16(value as u16),
                    Immediate::I8(value) => Immediate::U16(u16::try_from(value).ok()?),
                    Immediate::U8(value) => Immediate::U16(value as u16),
                    Immediate::I16(value) => Immediate::U16(u16::try_from(value).ok()?),
                    _ => return None,
                },
                Type::I32 => Immediate::I32(imm.as_i32()?),
                Type::U32 => Immediate::U32(imm.as_u32()?),
                Type::I64 => Immediate::I64(imm.as_i64()?),
                Type::U64 => Immediate::U64(imm.as_u64()?),
                Type::I128 => Immediate::I128(imm.as_i128()?),
                Type::U128 => Immediate::U128(imm.as_u128()?),
                Type::Felt => Immediate::Felt(imm.as_felt()?),
                ty => unimplemented!("unrecognized integral type '{ty}'"),
            };

            let op_builder = builder.create::<Constant, _>(span);
            return op_builder(imm).ok().map(|op| op.as_operation_ref());
        }

        None
    }
}

impl DialectRegistration for HirDialect {
    const NAMESPACE: &'static str = "hir";

    #[inline]
    fn init(info: DialectInfo) -> Self {
        Self { info }
    }

    fn register_operations(info: &mut DialectInfo) {
        info.register_operation::<ops::Assert>();
        info.register_operation::<ops::Assertz>();
        info.register_operation::<ops::AssertEq>();
        info.register_operation::<ops::AssertEqImm>();
        info.register_operation::<ops::Unreachable>();
        info.register_operation::<ops::Poison>();
        info.register_operation::<ops::Add>();
        info.register_operation::<ops::AddOverflowing>();
        info.register_operation::<ops::Sub>();
        info.register_operation::<ops::SubOverflowing>();
        info.register_operation::<ops::Mul>();
        info.register_operation::<ops::MulOverflowing>();
        info.register_operation::<ops::Exp>();
        info.register_operation::<ops::Div>();
        info.register_operation::<ops::Sdiv>();
        info.register_operation::<ops::Mod>();
        info.register_operation::<ops::Smod>();
        info.register_operation::<ops::Divmod>();
        info.register_operation::<ops::Sdivmod>();
        info.register_operation::<ops::And>();
        info.register_operation::<ops::Or>();
        info.register_operation::<ops::Xor>();
        info.register_operation::<ops::Band>();
        info.register_operation::<ops::Bor>();
        info.register_operation::<ops::Bxor>();
        info.register_operation::<ops::Shl>();
        info.register_operation::<ops::ShlImm>();
        info.register_operation::<ops::Shr>();
        info.register_operation::<ops::Ashr>();
        info.register_operation::<ops::Rotl>();
        info.register_operation::<ops::Rotr>();
        info.register_operation::<ops::Eq>();
        info.register_operation::<ops::Neq>();
        info.register_operation::<ops::Gt>();
        info.register_operation::<ops::Gte>();
        info.register_operation::<ops::Lt>();
        info.register_operation::<ops::Lte>();
        info.register_operation::<ops::Min>();
        info.register_operation::<ops::Max>();
        info.register_operation::<ops::PtrToInt>();
        info.register_operation::<ops::IntToPtr>();
        info.register_operation::<ops::Cast>();
        info.register_operation::<ops::Bitcast>();
        info.register_operation::<ops::Trunc>();
        info.register_operation::<ops::Zext>();
        info.register_operation::<ops::Sext>();
        info.register_operation::<ops::Constant>();
        info.register_operation::<ops::ConstantBytes>();
        info.register_operation::<ops::Ret>();
        info.register_operation::<ops::RetImm>();
        info.register_operation::<ops::Br>();
        info.register_operation::<ops::CondBr>();
        info.register_operation::<ops::Switch>();
        info.register_operation::<ops::If>();
        info.register_operation::<ops::While>();
        info.register_operation::<ops::IndexSwitch>();
        info.register_operation::<ops::Condition>();
        info.register_operation::<ops::Yield>();
        info.register_operation::<ops::Exec>();
        info.register_operation::<ops::Store>();
        info.register_operation::<ops::Load>();
        info.register_operation::<ops::MemGrow>();
        info.register_operation::<ops::MemSize>();
        info.register_operation::<ops::MemSet>();
        info.register_operation::<ops::MemCpy>();
        info.register_operation::<ops::Select>();
        info.register_operation::<ops::Incr>();
        info.register_operation::<ops::Neg>();
        info.register_operation::<ops::Inv>();
        info.register_operation::<ops::Ilog2>();
        info.register_operation::<ops::Pow2>();
        info.register_operation::<ops::Not>();
        info.register_operation::<ops::Bnot>();
        info.register_operation::<ops::IsOdd>();
        info.register_operation::<ops::Popcnt>();
        info.register_operation::<ops::Clz>();
        info.register_operation::<ops::Ctz>();
        info.register_operation::<ops::Clo>();
        info.register_operation::<ops::Cto>();
    }
}
