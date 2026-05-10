use midenc_hir::{
    derive::{EffectOpInterface, OpParser, OpPrinter, operation},
    dialects::builtin::attributes::{StringAttr, U32Attr},
    effects::*,
    traits::*,
    *,
};

use crate::HirDialect;

#[derive(EffectOpInterface, OpPrinter, OpParser)]
#[operation(
    dialect = HirDialect,
    implements(MemoryEffectOpInterface, OpPrinter)
)]
#[effects(MemoryEffect(MemoryEffect::Write))]
pub struct Assert {
    #[operand]
    value: Bool,
    #[attr]
    #[default]
    code: U32Attr,
    #[attr]
    #[default]
    message: StringAttr,
}

#[derive(EffectOpInterface, OpPrinter, OpParser)]
#[operation(
    dialect = HirDialect,
    implements(MemoryEffectOpInterface, OpPrinter)
)]
#[effects(MemoryEffect(MemoryEffect::Write))]
pub struct Assertz {
    #[operand]
    value: Bool,
    #[attr]
    #[default]
    code: U32Attr,
    #[attr]
    #[default]
    message: StringAttr,
}

#[derive(EffectOpInterface, OpPrinter, OpParser)]
#[operation(
    dialect = HirDialect,
    traits(BinaryOp, Commutative, SameTypeOperands),
    implements(MemoryEffectOpInterface, OpPrinter)
)]
#[effects(MemoryEffect(MemoryEffect::Write))]
pub struct AssertEq {
    #[operand]
    lhs: AnyInteger,
    #[operand]
    rhs: AnyInteger,
    #[attr]
    #[default]
    code: U32Attr,
    #[attr]
    #[default]
    message: StringAttr,
}

/// Assert that the operand is a valid u32 and refine its type on success.
#[derive(EffectOpInterface, OpPrinter, OpParser)]
#[operation(
    dialect = HirDialect,
    implements(InferTypeOpInterface, MemoryEffectOpInterface, OpPrinter)
)]
#[effects(MemoryEffect(MemoryEffect::Write))]
pub struct AssertU32 {
    #[operand]
    value: AnyInteger,
    #[attr]
    #[default]
    code: U32Attr,
    #[attr]
    #[default]
    message: StringAttr,
    #[result]
    result: AnyInteger,
}

impl InferTypeOpInterface for AssertU32 {
    fn infer_return_types(&mut self, _context: &Context) -> Result<(), Report> {
        self.result_mut().set_type(Type::U32);
        Ok(())
    }
}
