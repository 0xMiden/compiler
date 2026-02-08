mod builders;
mod ops;

pub use self::{builders::TestOpBuilder, ops::*};
use crate::{
    AttributeRef, Builder, BuilderExt, Dialect, DialectInfo, DialectRegistration, Immediate,
    OperationRef, SourceSpan, Type, attributes::IntegerLikeAttr,
};

#[derive(Debug)]
pub struct TestDialect {
    info: DialectInfo,
}

impl TestDialect {
    #[inline]
    pub fn num_registered(&self) -> usize {
        self.registered_ops().len()
    }
}

impl Dialect for TestDialect {
    #[inline]
    fn info(&self) -> &DialectInfo {
        &self.info
    }

    fn materialize_constant(
        &self,
        builder: &mut dyn Builder,
        attr: AttributeRef,
        ty: &Type,
        span: SourceSpan,
    ) -> Option<OperationRef> {
        use crate::Op;

        // Save the current insertion point
        let mut builder = crate::InsertionGuard::new(builder);

        // Only integer constants are supported for now
        if !ty.is_integer() {
            return None;
        }

        // Currently, we expect folds to produce integer-valued attributes
        let imm = attr
            .borrow()
            .as_attr()
            .as_trait::<dyn IntegerLikeAttr>()
            .map(|attr| (attr.as_immediate(), attr.ty().clone()));
        if let Some((imm, imm_ty)) = imm {
            // If the immediate value is of the same type as the expected result type, we're ready
            // to materialize the constant
            if &imm_ty == ty {
                let op_builder = builder.create::<Constant, _>(span);
                return op_builder(imm)
                    .ok()
                    .map(|op| op.borrow().as_operation().as_operation_ref());
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
            return op_builder(imm).ok().map(|op| op.borrow().as_operation().as_operation_ref());
        }

        None
    }
}

impl DialectRegistration for TestDialect {
    const NAMESPACE: &'static str = "test";

    #[inline]
    fn init(info: DialectInfo) -> Self {
        Self { info }
    }

    fn register_operations(info: &mut DialectInfo) {
        info.register_operation::<ops::Add>();
        info.register_operation::<ops::Mul>();
        info.register_operation::<ops::Shl>();
        info.register_operation::<ops::Ret>();
        info.register_operation::<ops::Constant>();
        info.register_operation::<ops::Eq>();
        info.register_operation::<ops::Neq>();
        info.register_operation::<ops::Store>();
    }
}
